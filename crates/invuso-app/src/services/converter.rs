//! Rates for the currency converter: the newest one or that of a chosen
//! day (FX-09), and a rate the user typed in (FX-10).

use invuso_core::Decimal;
use invuso_core::domain::Currency;
use invuso_core::fx::Rate;
use thiserror::Error;

use super::rates::{CurrencyApi, Frankfurter, RateProvider, refresh_on_date};
use crate::clock::previous_day;
use crate::storage::{CONVERTER_MANUAL_RATE, Db, MANUAL_SOURCE, RateQuote, StorageError};

/// A rate up to this many days before the chosen day still counts as that
/// day's: providers publish no rates on weekends and holidays.
const DAY_TOLERANCE: usize = 4;

#[derive(Debug, Error)]
pub enum ManualRateError {
    #[error("the rate must be greater than 0")]
    NotPositive,
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// The archived rate `from → to`: the newest one, or for `day`
/// (`YYYY-MM-DD`) that day's or the closest earlier one (idee.md 8.4).
pub fn archived_quote(
    db: &Db,
    from: Currency,
    to: Currency,
    day: Option<&str>,
) -> Result<Option<RateQuote>, StorageError> {
    match day {
        None => db.latest_rate(from, to),
        Some(day) => db.rate_on(from, to, day),
    }
}

/// The manual rate with this id, read `from → to`. `None` if it is no
/// manual rate or belongs to another pair, so the converter falls back to
/// the archived rate.
pub fn manual_quote(
    db: &Db,
    id: &str,
    from: Currency,
    to: Currency,
) -> Result<Option<RateQuote>, StorageError> {
    Ok(db
        .archived_rate(id, from, to)?
        .filter(|quote| quote.legs.iter().all(|leg| leg.source == MANUAL_SOURCE)))
}

/// Whether the rates of `day` should be fetched: the archive has none for
/// the pair from that day or the few days before it.
pub fn day_missing(quote: Option<&RateQuote>, day: &str) -> bool {
    let Some(rate_date) = quote.and_then(|q| q.rate_date.as_deref()) else {
        // No quote, or the same currency on both sides (no date).
        return quote.is_none();
    };
    let mut earliest = day.to_string();
    for _ in 0..DAY_TOLERANCE {
        match previous_day(&earliest) {
            Some(before) => earliest = before,
            None => return false,
        }
    }
    rate_date < earliest.as_str()
}

/// Fetches and archives the rates of a past day (FX-09). Blocks on the
/// network; call it off the UI thread. `Ok(false)` when no provider was
/// reachable or none had rates for that day.
pub fn fetch_day(db: &Db, day: &str) -> Result<bool, StorageError> {
    fetch_day_with(db, &Frankfurter, &CurrencyApi, day)
}

fn fetch_day_with(
    db: &Db,
    primary: &dyn RateProvider,
    fallback: &dyn RateProvider,
    day: &str,
) -> Result<bool, StorageError> {
    Ok(refresh_on_date(db, primary, fallback, day)?.archived > 0)
}

/// Archives `1 base = value quote` as a manual rate of `date` and makes the
/// converter use it while that pair is shown (FX-10). Returns its id.
pub fn save_manual_rate(
    db: &Db,
    base: Currency,
    quote: Currency,
    value: Decimal,
    date: &str,
) -> Result<String, ManualRateError> {
    let rate = Rate::new(base, quote, value).map_err(|_| ManualRateError::NotPositive)?;
    let id = db.add_manual_rate(&rate, date)?;
    db.set_setting(CONVERTER_MANUAL_RATE, &id)?;
    Ok(id)
}

/// Id of the manual rate the converter uses, if any.
pub fn selected_manual_rate(db: &Db) -> Result<Option<String>, StorageError> {
    Ok(db
        .setting(CONVERTER_MANUAL_RATE)?
        .filter(|id| !id.is_empty()))
}

/// Back to the archived rates.
pub fn clear_manual_rate(db: &Db) -> Result<(), StorageError> {
    db.set_setting(CONVERTER_MANUAL_RATE, "")
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use invuso_core::domain::Money;
    use invuso_core::fx;

    use super::*;
    use crate::services::rates::RateError;
    use crate::storage::NewExchangeRate;

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn d(text: &str) -> Decimal {
        Decimal::from_str(text).unwrap()
    }

    fn eur_to(quote: &str, value: &str, date: &str) -> NewExchangeRate {
        NewExchangeRate {
            rate: Rate::new(cur("EUR"), cur(quote), d(value)).unwrap(),
            rate_date: date.into(),
        }
    }

    struct Fake(Option<Vec<NewExchangeRate>>);

    impl RateProvider for Fake {
        fn source(&self) -> &'static str {
            "frankfurter"
        }
        fn latest(&self) -> Result<Vec<NewExchangeRate>, RateError> {
            unreachable!("the converter only fetches days")
        }
        fn on_date(&self, _date: &str) -> Result<Vec<NewExchangeRate>, RateError> {
            self.0
                .clone()
                .ok_or_else(|| RateError::Format("offline".into()))
        }
    }

    #[test]
    fn a_day_is_fetched_only_without_a_rate_close_to_it() {
        let db = Db::open_in_memory().unwrap();
        db.archive_rates("frankfurter", 1, &[eur_to("JPY", "170", "2026-09-01")])
            .unwrap();
        let on = |day| archived_quote(&db, cur("JPY"), cur("EUR"), Some(day)).unwrap();
        // Weeks old: fetch. Friday's rate on Monday: fine.
        assert!(day_missing(on("2026-10-05").as_ref(), "2026-10-05"));
        assert!(day_missing(on("2026-08-01").as_ref(), "2026-08-01"));
        db.archive_rates("frankfurter", 2, &[eur_to("JPY", "171", "2026-10-02")])
            .unwrap();
        assert!(!day_missing(on("2026-10-05").as_ref(), "2026-10-05"));
        assert!(!day_missing(on("2026-10-06").as_ref(), "2026-10-06"));
        assert!(day_missing(on("2026-10-07").as_ref(), "2026-10-07"));
        // Same currency never needs a rate.
        let same = archived_quote(&db, cur("EUR"), cur("EUR"), Some("2026-10-07")).unwrap();
        assert!(!day_missing(same.as_ref(), "2026-10-07"));
    }

    #[test]
    fn a_fetched_day_is_used_for_that_day_and_offline_keeps_the_old_rate() {
        let db = Db::open_in_memory().unwrap();
        db.archive_rates("frankfurter", 1, &[eur_to("JPY", "170", "2026-10-05")])
            .unwrap();
        let offline = Fake(None);
        assert!(!fetch_day_with(&db, &offline, &offline, "2026-03-02").unwrap());
        assert_eq!(
            archived_quote(&db, cur("JPY"), cur("EUR"), Some("2026-03-02")).unwrap(),
            None
        );

        let march = Fake(Some(vec![eur_to("JPY", "160", "2026-03-02")]));
        assert!(fetch_day_with(&db, &march, &Fake(None), "2026-03-02").unwrap());
        let quote = archived_quote(&db, cur("JPY"), cur("EUR"), Some("2026-03-02"))
            .unwrap()
            .unwrap();
        assert_eq!(quote.rate_date.as_deref(), Some("2026-03-02"));
        // The newest rate is still today's.
        let latest = archived_quote(&db, cur("JPY"), cur("EUR"), None)
            .unwrap()
            .unwrap();
        assert_eq!(latest.rate_date.as_deref(), Some("2026-10-05"));
    }

    #[test]
    fn manual_rate_is_used_only_for_its_pair_and_is_archived_as_manual() {
        let db = Db::open_in_memory().unwrap();
        db.archive_rates("frankfurter", 1, &[eur_to("JPY", "170", "2026-10-05")])
            .unwrap();
        // Exchange office: 1 EUR = 155 JPY.
        let id = save_manual_rate(&db, cur("EUR"), cur("JPY"), d("155"), "2026-10-06").unwrap();
        assert_eq!(selected_manual_rate(&db).unwrap(), Some(id.clone()));

        let manual = manual_quote(&db, &id, cur("JPY"), cur("EUR"))
            .unwrap()
            .unwrap();
        let eur = fx::convert(Money::new(15_500, cur("JPY")), &manual.rate).unwrap();
        assert_eq!(eur, Money::new(10_000, cur("EUR")));
        assert_eq!(
            manual_quote(&db, &id, cur("JPY"), cur("USD")).unwrap(),
            None
        );

        // It shows up in the archive with source "manual" …
        let history = db.rate_history(cur("EUR"), cur("JPY")).unwrap();
        assert_eq!(history[0].source, MANUAL_SOURCE);
        assert_eq!(history[0].rate.value(), d("155"));
        // … but never replaces the fetched rate elsewhere.
        let latest = archived_quote(&db, cur("JPY"), cur("EUR"), None)
            .unwrap()
            .unwrap();
        assert_eq!(latest.legs[0].source, "frankfurter");

        // A fetched rate's id is no manual rate.
        assert_eq!(
            manual_quote(&db, &latest.legs[0].id, cur("JPY"), cur("EUR")).unwrap(),
            None
        );
        clear_manual_rate(&db).unwrap();
        assert_eq!(selected_manual_rate(&db).unwrap(), None);
        assert!(matches!(
            save_manual_rate(&db, cur("EUR"), cur("JPY"), Decimal::ZERO, "2026-10-06"),
            Err(ManualRateError::NotPositive)
        ));
    }
}
