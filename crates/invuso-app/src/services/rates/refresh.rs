use std::collections::BTreeSet;
use std::time::Duration;

use dioxus::prelude::*;
use invuso_core::domain::Currency;

use super::{CurrencyApi, Frankfurter, RateError, RateProvider};
use crate::state::{DataRevision, RateStatus, Toaster};
use crate::storage::{Db, NewExchangeRate, StorageError, now_ms};

/// Result of one refresh: how many rates were archived and which
/// providers failed. A failure is a hint, never fatal (FX-03).
#[derive(Debug, Default)]
pub struct RefreshOutcome {
    pub archived: usize,
    pub failures: Vec<(&'static str, RateError)>,
    /// How many providers were asked.
    pub attempted: usize,
}

impl RefreshOutcome {
    /// No provider answered, e.g. without internet.
    pub fn reached_none(&self) -> bool {
        self.failures.len() == self.attempted
    }
}

/// Fetches the newest rates from both providers and archives them (FX-01,
/// FX-02).
pub fn refresh_latest(
    db: &Db,
    primary: &dyn RateProvider,
    fallback: &dyn RateProvider,
) -> Result<RefreshOutcome, StorageError> {
    fetch_and_archive(db, primary, fallback, |provider| provider.latest())
}

/// Fetches and archives the rates of a past day, for back-dated expenses
/// (FX-09).
pub fn refresh_on_date(
    db: &Db,
    primary: &dyn RateProvider,
    fallback: &dyn RateProvider,
    date: &str,
) -> Result<RefreshOutcome, StorageError> {
    fetch_and_archive(db, primary, fallback, |provider| provider.on_date(date))
}

/// The fallback only contributes currencies the primary provider does not
/// carry, so each currency keeps a single source and rates do not jump
/// between providers. While the primary is unreachable, the currencies it
/// delivered before still count as its own.
fn fetch_and_archive(
    db: &Db,
    primary: &dyn RateProvider,
    fallback: &dyn RateProvider,
    fetch: impl Fn(&dyn RateProvider) -> Result<Vec<NewExchangeRate>, RateError>,
) -> Result<RefreshOutcome, StorageError> {
    let mut outcome = RefreshOutcome {
        attempted: 2,
        ..RefreshOutcome::default()
    };
    let fetched_at = now_ms();

    let covered: BTreeSet<Currency> = match fetch(primary) {
        Ok(rates) => {
            outcome.archived += db.archive_rates(primary.source(), fetched_at, &rates)?;
            rates.iter().map(|r| r.rate.quote()).collect()
        }
        Err(error) => {
            outcome.failures.push((primary.source(), error));
            db.archived_quotes(primary.source())?.into_iter().collect()
        }
    };

    match fetch(fallback) {
        Ok(rates) => {
            let missing: Vec<_> = rates
                .into_iter()
                .filter(|r| !covered.contains(&r.rate.quote()))
                .collect();
            outcome.archived += db.archive_rates(fallback.source(), fetched_at, &missing)?;
        }
        Err(error) => outcome.failures.push((fallback.source(), error)),
    }
    Ok(outcome)
}

const HOUR_MS: i64 = 60 * 60 * 1000;
/// How often the running app checks whether a refresh is due.
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Refresh at start unless the last fetch is less than an hour old (quick
/// restarts would only archive the same rates again), then once a day
/// (FX-11). After a failed attempt it retries hourly until rates arrive.
fn refresh_due(last_fetch: Option<i64>, now: i64, eager: bool) -> bool {
    let Some(last_fetch) = last_fetch else {
        return true;
    };
    let min_age = if eager { HOUR_MS } else { 24 * HOUR_MS };
    now - last_fetch >= min_age
}

/// Keeps the rate archive fresh while the app runs. Call once, above the
/// router. Failures show one toast per streak and never stop the app.
pub fn use_rate_refresh() {
    let db = use_context::<Db>();
    let mut toaster = use_context::<Toaster>();
    let mut revision = use_context::<DataRevision>();
    let mut status = use_context::<RateStatus>();

    use_hook(move || {
        spawn(async move {
            let mut eager = true;
            let mut warned = false;
            loop {
                let due = db
                    .last_rate_fetch()
                    .map(|last| refresh_due(last, now_ms(), eager))
                    .unwrap_or(false);
                if due {
                    let worker_db = db.clone();
                    // Network calls block; keep them off the UI thread.
                    let outcome = tokio::task::spawn_blocking(move || {
                        refresh_latest(&worker_db, &Frankfurter, &CurrencyApi)
                    })
                    .await;
                    let (archived, failed, offline) = match outcome {
                        Ok(Ok(outcome)) => (
                            outcome.archived,
                            !outcome.failures.is_empty(),
                            outcome.reached_none(),
                        ),
                        _ => (0, true, true),
                    };
                    status.set_offline(offline);
                    if archived > 0 {
                        revision.bump();
                    }
                    if failed && !warned {
                        toaster.show(t!("rates.refresh_failed").to_string(), None);
                    }
                    warned = failed;
                    eager = archived == 0;
                } else {
                    eager = false;
                }
                tokio::time::sleep(CHECK_INTERVAL).await;
            }
        })
    });
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use invuso_core::Decimal;
    use invuso_core::fx::Rate;

    use super::*;

    /// Provider with canned answers; `None` simulates being offline.
    struct Fake {
        source: &'static str,
        rates: Option<Vec<(&'static str, &'static str)>>,
    }

    impl Fake {
        fn answer(&self) -> Result<Vec<NewExchangeRate>, RateError> {
            let rates = self
                .rates
                .as_ref()
                .ok_or_else(|| RateError::Format("offline".into()))?;
            Ok(rates
                .iter()
                .map(|(code, value)| NewExchangeRate {
                    rate: Rate::new(
                        Currency::from_code("EUR").unwrap(),
                        Currency::from_code(code).unwrap(),
                        Decimal::from_str(value).unwrap(),
                    )
                    .unwrap(),
                    rate_date: "2026-10-04".into(),
                })
                .collect())
        }
    }

    impl RateProvider for Fake {
        fn source(&self) -> &'static str {
            self.source
        }
        fn latest(&self) -> Result<Vec<NewExchangeRate>, RateError> {
            self.answer()
        }
        fn on_date(&self, _date: &str) -> Result<Vec<NewExchangeRate>, RateError> {
            self.answer()
        }
    }

    fn primary(online: bool) -> Fake {
        Fake {
            source: "frankfurter",
            rates: online.then(|| vec![("JPY", "178.23"), ("USD", "1.1287")]),
        }
    }

    fn fallback(online: bool) -> Fake {
        Fake {
            source: "currency-api",
            rates: online.then(|| vec![("JPY", "177.71"), ("BGN", "1.95583"), ("VED", "973.69")]),
        }
    }

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn source_of(db: &Db, code: &str) -> String {
        db.latest_rate(cur("EUR"), cur(code)).unwrap().unwrap().legs[0]
            .source
            .clone()
    }

    #[test]
    fn fallback_only_fills_the_gaps() {
        let db = Db::open_in_memory().unwrap();
        let outcome = refresh_latest(&db, &primary(true), &fallback(true)).unwrap();
        assert_eq!(outcome.archived, 4);
        assert!(outcome.failures.is_empty());
        assert_eq!(source_of(&db, "JPY"), "frankfurter");
        assert_eq!(source_of(&db, "BGN"), "currency-api");
    }

    #[test]
    fn primary_offline_keeps_its_currencies() {
        let db = Db::open_in_memory().unwrap();
        refresh_latest(&db, &primary(true), &fallback(false)).unwrap();
        let outcome = refresh_latest(&db, &primary(false), &fallback(true)).unwrap();
        assert_eq!(outcome.archived, 2);
        assert_eq!(outcome.failures.len(), 1);
        assert!(!outcome.reached_none());
        assert_eq!(source_of(&db, "JPY"), "frankfurter");
        assert_eq!(source_of(&db, "VED"), "currency-api");
    }

    #[test]
    fn fully_offline_archives_nothing_and_keeps_old_rates() {
        let db = Db::open_in_memory().unwrap();
        refresh_latest(&db, &primary(true), &fallback(true)).unwrap();
        let outcome = refresh_latest(&db, &primary(false), &fallback(false)).unwrap();
        assert_eq!(outcome.archived, 0);
        assert_eq!(outcome.failures.len(), 2);
        assert!(outcome.reached_none());
        let quote = db.latest_rate(cur("JPY"), cur("EUR")).unwrap().unwrap();
        assert_eq!(quote.rate_date.as_deref(), Some("2026-10-04"));
    }

    #[test]
    fn historical_day_is_archived_too() {
        let db = Db::open_in_memory().unwrap();
        let outcome = refresh_on_date(&db, &primary(true), &fallback(true), "2026-10-04").unwrap();
        assert_eq!(outcome.archived, 4);
        assert!(
            db.rate_on(cur("EUR"), cur("USD"), "2026-10-04")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn refresh_schedule() {
        let now = 100 * HOUR_MS;
        assert!(refresh_due(None, now, false));
        // At start: only if the last fetch is at least an hour old.
        assert!(!refresh_due(Some(now - HOUR_MS / 2), now, true));
        assert!(refresh_due(Some(now - HOUR_MS), now, true));
        // While running: once a day.
        assert!(!refresh_due(Some(now - 23 * HOUR_MS), now, false));
        assert!(refresh_due(Some(now - 24 * HOUR_MS), now, false));
    }
}
