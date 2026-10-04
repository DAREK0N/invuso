use std::str::FromStr;

use invuso_core::Decimal;
use invuso_core::domain::{Currency, is_iso_date};
use invuso_core::fx::{self, Rate};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::db::{new_id, now_ms};
use super::{Db, StorageError};

/// Currency every archived rate is quoted against; other pairs are crossed
/// through it (idee.md 10.3: Frankfurter and its fallback quote EUR).
const PIVOT: &str = "EUR";

/// `source` of a cross rate an expense was converted with (AP-11). Such
/// rows only keep the exact rate of that expense (FX-04); lookups skip
/// them, so they never hide newer rates of the legs they were made of.
pub const CROSS_SOURCE: &str = "cross";

/// A rate as a provider reported it: `1 base = value quote` on `rate_date`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewExchangeRate {
    pub rate: Rate,
    /// `YYYY-MM-DD`.
    pub rate_date: String,
}

/// An archived rate (idee.md 4.1 `ExchangeRate`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeRate {
    pub id: String,
    pub rate: Rate,
    pub rate_date: String,
    /// Unix milliseconds.
    pub fetched_at: i64,
    pub source: String,
}

/// The rate for a currency pair, picked from the archive by idee.md 8.4:
/// stored directly, the inverse of a stored rate, or crossed through EUR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateQuote {
    pub rate: Rate,
    /// Day the rate belongs to; the older day of both legs for a cross
    /// rate. `None` when both currencies are the same.
    pub rate_date: Option<String>,
    /// When it was fetched (Unix ms); the older fetch for a cross rate.
    pub fetched_at: Option<i64>,
    /// The archived rates it is made of: none for the same currency, two
    /// for a cross rate.
    pub legs: Vec<ExchangeRate>,
}

/// A rate picked for a day by [`Db::rate_near`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NearRate {
    pub quote: RateQuote,
    /// At least one leg is from a day after the requested one, because the
    /// archive had nothing on or before it.
    pub later: bool,
}

/// Which archived rate a lookup wants.
#[derive(Debug, Clone, Copy)]
enum Pick<'a> {
    /// That day's, otherwise the closest earlier one (idee.md 8.4).
    OnOrBefore(&'a str),
    /// Like `OnOrBefore`, otherwise the closest later one.
    Near(&'a str),
    Latest,
}

const COLUMNS: &str = "id, base, quote, rate, rate_date, fetched_at, source";

impl Db {
    /// Archives every rate of one fetch (FX-02); nothing is ever overwritten,
    /// so expenses keep pointing at the rate they used (FX-04).
    pub fn archive_rates(
        &self,
        source: &str,
        fetched_at: i64,
        rates: &[NewExchangeRate],
    ) -> Result<usize, StorageError> {
        if rates.iter().any(|r| !is_iso_date(&r.rate_date)) {
            return Err(StorageError::InvalidInput("rate date must be YYYY-MM-DD"));
        }
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            let now = now_ms();
            {
                let mut insert = tx.prepare(
                    "INSERT INTO exchange_rate
                         (id, base, quote, rate, rate_date, fetched_at, source,
                          created_at, updated_at, origin_device_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)",
                )?;
                for new in rates {
                    insert.execute(params![
                        new_id(),
                        new.rate.base().code(),
                        new.rate.quote().code(),
                        new.rate.value().to_string(),
                        new.rate_date,
                        fetched_at,
                        source,
                        now,
                        self.device_id()
                    ])?;
                }
            }
            tx.commit()?;
            Ok(rates.len())
        })
    }

    /// Time of the newest archived fetch (Unix ms), if any.
    pub fn last_rate_fetch(&self) -> Result<Option<i64>, StorageError> {
        self.with(|conn| {
            Ok(conn.query_row(
                "SELECT MAX(fetched_at) FROM exchange_rate WHERE deleted_at IS NULL",
                [],
                |row| row.get(0),
            )?)
        })
    }

    /// Currencies a source has ever delivered, e.g. to know which ones the
    /// fallback provider must cover while the main one is unreachable.
    pub fn archived_quotes(&self, source: &str) -> Result<Vec<Currency>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(
                "SELECT DISTINCT quote FROM exchange_rate
                 WHERE source = ?1 AND deleted_at IS NULL ORDER BY quote",
            )?;
            let codes = statement
                .query_map([source], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(codes
                .iter()
                .filter_map(|code| Currency::from_code(code).ok())
                .collect())
        })
    }

    /// Rate for `date` (`YYYY-MM-DD`): that day's, otherwise the closest
    /// earlier one (idee.md 8.4). `None` if the archive has nothing that old.
    pub fn rate_on(
        &self,
        base: Currency,
        quote: Currency,
        date: &str,
    ) -> Result<Option<RateQuote>, StorageError> {
        if !is_iso_date(date) {
            return Err(StorageError::InvalidInput("rate date must be YYYY-MM-DD"));
        }
        self.with(|conn| pick_quote(conn, base, quote, Pick::OnOrBefore(date)))
    }

    /// Like [`Db::rate_on`], but when the archive has nothing that old, each
    /// leg falls back to the closest later rate (user decision in AP-11 for
    /// back-dated expenses while offline).
    pub fn rate_near(
        &self,
        base: Currency,
        quote: Currency,
        date: &str,
    ) -> Result<Option<NearRate>, StorageError> {
        if !is_iso_date(date) {
            return Err(StorageError::InvalidInput("rate date must be YYYY-MM-DD"));
        }
        let picked = self.with(|conn| pick_quote(conn, base, quote, Pick::Near(date)))?;
        Ok(picked.map(|quote| NearRate {
            later: quote.legs.iter().any(|leg| leg.rate_date.as_str() > date),
            quote,
        }))
    }

    /// The newest archived rate, e.g. while offline (FX-03); its date tells
    /// how old it is.
    pub fn latest_rate(
        &self,
        base: Currency,
        quote: Currency,
    ) -> Result<Option<RateQuote>, StorageError> {
        self.with(|conn| pick_quote(conn, base, quote, Pick::Latest))
    }

    /// The archived rate with this id (e.g. `Expense.fx_rate_id`), read
    /// `base → quote`. `None` if it does not exist or is for another pair.
    /// Lets an edited expense keep the rate it was saved with (FX-04).
    pub fn archived_rate(
        &self,
        id: &str,
        base: Currency,
        quote: Currency,
    ) -> Result<Option<RateQuote>, StorageError> {
        let stored = self.with(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {COLUMNS} FROM exchange_rate WHERE id = ?1"),
                    [id],
                    rate_from_row,
                )
                .optional()?)
        })?;
        Ok(stored.and_then(|stored| {
            let rate = if stored.rate.base() == base && stored.rate.quote() == quote {
                stored.rate
            } else if stored.rate.base() == quote && stored.rate.quote() == base {
                stored.rate.inverse()
            } else {
                return None;
            };
            Some(RateQuote {
                rate,
                rate_date: Some(stored.rate_date.clone()),
                fetched_at: Some(stored.fetched_at),
                legs: vec![stored],
            })
        }))
    }
}

/// The id of the archived rate an expense converted with: none for the
/// same currency, the stored row for a direct rate (read in either
/// direction), and a new [`CROSS_SOURCE`] row for a cross rate, because an
/// expense can point at only one rate.
pub(super) fn rate_id_for_expense(
    conn: &Connection,
    device_id: &str,
    quote: &RateQuote,
) -> Result<Option<String>, StorageError> {
    match quote.legs.as_slice() {
        [] => Ok(None),
        [leg] => Ok(Some(leg.id.clone())),
        _ => {
            let id = new_id();
            let now = now_ms();
            conn.execute(
                "INSERT INTO exchange_rate
                     (id, base, quote, rate, rate_date, fetched_at, source,
                      created_at, updated_at, origin_device_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)",
                params![
                    id,
                    quote.rate.base().code(),
                    quote.rate.quote().code(),
                    quote.rate.value().to_string(),
                    quote.rate_date,
                    quote.fetched_at,
                    CROSS_SOURCE,
                    now,
                    device_id
                ],
            )?;
            Ok(Some(id))
        }
    }
}

fn pick_quote(
    conn: &Connection,
    base: Currency,
    quote: Currency,
    pick: Pick<'_>,
) -> Result<Option<RateQuote>, StorageError> {
    if base == quote {
        return Ok(Some(RateQuote {
            rate: Rate::new(base, quote, Decimal::ONE).map_err(fx_error)?,
            rate_date: None,
            fetched_at: None,
            legs: Vec::new(),
        }));
    }
    if let Some((rate, stored)) = pick_leg(conn, base, quote, pick)? {
        return Ok(Some(RateQuote {
            rate,
            rate_date: Some(stored.rate_date.clone()),
            fetched_at: Some(stored.fetched_at),
            legs: vec![stored],
        }));
    }
    let pivot = Currency::from_code(PIVOT).map_err(|_| StorageError::InvalidInput("pivot"))?;
    if base == pivot || quote == pivot {
        return Ok(None);
    }
    let (Some((to_pivot, first)), Some((from_pivot, second))) = (
        pick_leg(conn, base, pivot, pick)?,
        pick_leg(conn, pivot, quote, pick)?,
    ) else {
        return Ok(None);
    };
    let rate = fx::chain(&to_pivot, &from_pivot).map_err(fx_error)?;
    Ok(Some(RateQuote {
        rate,
        rate_date: Some(first.rate_date.clone().min(second.rate_date.clone())),
        fetched_at: Some(first.fetched_at.min(second.fetched_at)),
        legs: vec![first, second],
    }))
}

/// Best archived rate between two currencies in either direction, turned
/// to read `from → to`. Ties on the day go to the newer fetch.
fn pick_leg(
    conn: &Connection,
    from: Currency,
    to: Currency,
    pick: Pick<'_>,
) -> Result<Option<(Rate, ExchangeRate)>, StorageError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM exchange_rate
         WHERE deleted_at IS NULL AND source <> ?3
           AND ((base = ?1 AND quote = ?2) OR (base = ?2 AND quote = ?1))"
    ))?;
    let rows = statement
        .query_map([from.code(), to.code(), CROSS_SOURCE], rate_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    let keyed: Vec<_> = rows
        .into_iter()
        .map(|r| ((r.rate_date.clone(), r.fetched_at), r))
        .collect();
    let on_or_before = |date: &str| fx::rate_for_date(&keyed, &(date.to_string(), i64::MAX));
    let best = match pick {
        Pick::OnOrBefore(date) => on_or_before(date),
        // Closest later day; on that day the newest fetch.
        Pick::Near(date) => on_or_before(date).or_else(|| {
            keyed
                .iter()
                .filter(|((day, _), _)| day.as_str() > date)
                .min_by(|((a_day, a_at), _), ((b_day, b_at), _)| {
                    a_day.cmp(b_day).then(b_at.cmp(a_at))
                })
                .map(|(_, r)| r)
        }),
        Pick::Latest => keyed.iter().max_by(|a, b| a.0.cmp(&b.0)).map(|(_, r)| r),
    };
    Ok(best.map(|stored| {
        let rate = if stored.rate.base() == from {
            stored.rate
        } else {
            stored.rate.inverse()
        };
        (rate, stored.clone())
    }))
}

fn rate_from_row(row: &Row<'_>) -> rusqlite::Result<ExchangeRate> {
    let invalid = |index: usize, e: Box<dyn std::error::Error + Send + Sync>| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, e)
    };
    let base = Currency::from_code(&row.get::<_, String>(1)?).map_err(|e| invalid(1, e.into()))?;
    let quote = Currency::from_code(&row.get::<_, String>(2)?).map_err(|e| invalid(2, e.into()))?;
    let value = Decimal::from_str(&row.get::<_, String>(3)?).map_err(|e| invalid(3, e.into()))?;
    Ok(ExchangeRate {
        id: row.get(0)?,
        rate: Rate::new(base, quote, value).map_err(|e| invalid(3, e.into()))?,
        rate_date: row.get(4)?,
        fetched_at: row.get(5)?,
        source: row.get(6)?,
    })
}

fn fx_error(_: fx::FxError) -> StorageError {
    StorageError::InvalidInput("archived rates do not combine")
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::Money;

    use super::*;

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn eur_to(quote: &str, value: &str, date: &str) -> NewExchangeRate {
        NewExchangeRate {
            rate: Rate::new(cur("EUR"), cur(quote), Decimal::from_str(value).unwrap()).unwrap(),
            rate_date: date.into(),
        }
    }

    fn archive(db: &Db, fetched_at: i64, rates: &[NewExchangeRate]) {
        db.archive_rates("frankfurter", fetched_at, rates).unwrap();
    }

    #[test]
    fn archives_every_fetch_with_exact_decimals() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.last_rate_fetch().unwrap(), None);
        archive(&db, 1_000, &[eur_to("JPY", "177.71378084", "2026-10-04")]);
        archive(&db, 2_000, &[eur_to("JPY", "177.71378084", "2026-10-04")]);
        assert_eq!(db.last_rate_fetch().unwrap(), Some(2_000));

        let quote = db.latest_rate(cur("EUR"), cur("JPY")).unwrap().unwrap();
        assert_eq!(
            quote.rate.value(),
            Decimal::from_str("177.71378084").unwrap()
        );
        assert_eq!(quote.fetched_at, Some(2_000));
        let count: i64 = db
            .with(|c| Ok(c.query_row("SELECT COUNT(*) FROM exchange_rate", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn picks_rate_of_the_day_or_closest_earlier() {
        let db = Db::open_in_memory().unwrap();
        archive(
            &db,
            1,
            &[
                eur_to("JPY", "170", "2026-10-01"),
                eur_to("JPY", "171", "2026-10-02"),
                eur_to("JPY", "175", "2026-10-05"),
            ],
        );
        let on = |date| {
            db.rate_on(cur("EUR"), cur("JPY"), date)
                .unwrap()
                .map(|q| (q.rate.value(), q.rate_date.unwrap()))
        };
        assert_eq!(
            on("2026-10-02"),
            Some((Decimal::from(171), "2026-10-02".into()))
        );
        assert_eq!(
            on("2026-10-04"),
            Some((Decimal::from(171), "2026-10-02".into()))
        );
        assert_eq!(on("2026-09-30"), None);
        let latest = db.latest_rate(cur("EUR"), cur("JPY")).unwrap().unwrap();
        assert_eq!(latest.rate_date.as_deref(), Some("2026-10-05"));
    }

    #[test]
    fn newer_fetch_wins_on_the_same_day() {
        let db = Db::open_in_memory().unwrap();
        archive(&db, 1, &[eur_to("USD", "1.10", "2026-10-04")]);
        archive(&db, 2, &[eur_to("USD", "1.12", "2026-10-04")]);
        let quote = db
            .rate_on(cur("EUR"), cur("USD"), "2026-10-04")
            .unwrap()
            .unwrap();
        assert_eq!(quote.rate.value(), Decimal::from_str("1.12").unwrap());
        assert_eq!(quote.legs.len(), 1);
    }

    #[test]
    fn inverts_for_the_opposite_direction() {
        let db = Db::open_in_memory().unwrap();
        archive(&db, 1, &[eur_to("JPY", "176.99", "2026-10-02")]);
        let quote = db.latest_rate(cur("JPY"), cur("EUR")).unwrap().unwrap();
        assert_eq!(quote.rate.base(), cur("JPY"));
        // 1 000 ¥ = 5.65 € (5.6500…)
        let eur = fx::convert(Money::new(1_000, cur("JPY")), &quote.rate).unwrap();
        assert_eq!(eur, Money::new(565, cur("EUR")));
    }

    #[test]
    fn crosses_other_pairs_through_eur() {
        let db = Db::open_in_memory().unwrap();
        archive(
            &db,
            5,
            &[
                eur_to("JPY", "176.99", "2026-10-02"),
                eur_to("CHF", "0.9279", "2026-10-03"),
            ],
        );
        let quote = db.latest_rate(cur("JPY"), cur("CHF")).unwrap().unwrap();
        assert_eq!(quote.legs.len(), 2);
        assert_eq!(quote.rate_date.as_deref(), Some("2026-10-02"));
        let chf = fx::convert(Money::new(10_000, cur("JPY")), &quote.rate).unwrap();
        assert_eq!(chf, Money::new(5_243, cur("CHF")));
        assert_eq!(db.latest_rate(cur("JPY"), cur("USD")).unwrap(), None);
    }

    #[test]
    fn same_currency_needs_no_archive() {
        let db = Db::open_in_memory().unwrap();
        let quote = db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        assert_eq!(quote.rate.value(), Decimal::ONE);
        assert!(quote.legs.is_empty());
        assert_eq!(quote.rate_date, None);
    }

    #[test]
    fn rejects_bad_dates() {
        let db = Db::open_in_memory().unwrap();
        assert!(matches!(
            db.archive_rates("x", 1, &[eur_to("JPY", "1", "2026-13-01")]),
            Err(StorageError::InvalidInput(_))
        ));
        assert!(matches!(
            db.rate_on(cur("EUR"), cur("JPY"), "yesterday"),
            Err(StorageError::InvalidInput(_))
        ));
    }

    #[test]
    fn lists_currencies_per_source() {
        let db = Db::open_in_memory().unwrap();
        archive(
            &db,
            1,
            &[
                eur_to("USD", "1.1", "2026-10-04"),
                eur_to("JPY", "170", "2026-10-04"),
            ],
        );
        db.archive_rates("currency-api", 1, &[eur_to("BGN", "1.95583", "2026-10-04")])
            .unwrap();
        assert_eq!(
            db.archived_quotes("frankfurter").unwrap(),
            vec![cur("JPY"), cur("USD")]
        );
    }

    #[test]
    fn near_falls_back_to_the_closest_later_rate_per_leg() {
        let db = Db::open_in_memory().unwrap();
        archive(
            &db,
            1,
            &[
                eur_to("JPY", "170", "2026-10-02"),
                eur_to("CHF", "0.93", "2026-10-05"),
                eur_to("CHF", "0.94", "2026-10-07"),
            ],
        );
        archive(&db, 2, &[eur_to("CHF", "0.95", "2026-10-05")]);

        let on_day = db
            .rate_near(cur("JPY"), cur("EUR"), "2026-10-03")
            .unwrap()
            .unwrap();
        assert!(!on_day.later);
        assert_eq!(on_day.quote.rate_date.as_deref(), Some("2026-10-02"));

        // CHF has nothing on or before the 3rd: the 5th, newest fetch.
        let near = db
            .rate_near(cur("EUR"), cur("CHF"), "2026-10-03")
            .unwrap()
            .unwrap();
        assert!(near.later);
        assert_eq!(near.quote.rate.value(), Decimal::from_str("0.95").unwrap());
        assert_eq!(
            db.rate_on(cur("EUR"), cur("CHF"), "2026-10-03").unwrap(),
            None
        );

        // Cross rate: the JPY leg from before, the CHF leg from after.
        let cross = db
            .rate_near(cur("JPY"), cur("CHF"), "2026-10-03")
            .unwrap()
            .unwrap();
        assert!(cross.later);
        assert_eq!(cross.quote.legs[0].rate_date, "2026-10-02");
        assert_eq!(cross.quote.legs[1].rate_date, "2026-10-05");

        assert_eq!(
            db.rate_near(cur("EUR"), cur("USD"), "2026-10-03").unwrap(),
            None
        );
    }
}
