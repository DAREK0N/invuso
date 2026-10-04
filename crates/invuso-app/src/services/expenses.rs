//! Saving an expense with the exchange rate of its day (EXP-07, idee.md 8.4).

use invuso_core::domain::{Currency, Expense, local_date, validate_occurred_at};
use thiserror::Error;

use super::rates::{RateProvider, refresh_on_date};
use crate::storage::{Db, NewExpense, StorageError};

#[derive(Debug, Error)]
pub enum SaveExpenseError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// Neither the archive nor the providers know a rate for the pair.
    #[error("no exchange rate {from} → {to}")]
    NoRate { from: Currency, to: Currency },
}

/// A saved expense and how its rate was found.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedExpense {
    pub expense: Expense,
    /// Converted with a rate from a later day, because no rate of the day
    /// or before could be found (offline fallback, user decision in AP-11).
    pub later_rate: bool,
}

/// Picks the rate for the expense's day and saves it. Blocks on the network
/// when the day has to be fetched, so it must run on a background thread.
///
/// Order: the archived rate of the day or the closest earlier one
/// (idee.md 8.4); if the archive has none, the day is fetched and archived
/// (FX-09); if that fails too, the closest later archived rate.
pub fn save_expense(
    db: &Db,
    primary: &dyn RateProvider,
    fallback: &dyn RateProvider,
    new: NewExpense,
) -> Result<SavedExpense, SaveExpenseError> {
    validate_occurred_at(&new.occurred_at).map_err(StorageError::from)?;
    let base = db.expense_base_currency(new.group_id.as_ref())?;
    let currency = new.total.currency();
    let date = local_date(&new.occurred_at).to_string();

    let mut quote = db.rate_on(currency, base, &date)?;
    if quote.is_none() {
        // Network failures only mean "offline"; the fallback below decides.
        refresh_on_date(db, primary, fallback, &date)?;
        quote = db.rate_on(currency, base, &date)?;
    }
    let (quote, later_rate) = match quote {
        Some(quote) => (quote, false),
        None => {
            let near = db
                .rate_near(currency, base, &date)?
                .ok_or(SaveExpenseError::NoRate {
                    from: currency,
                    to: base,
                })?;
            (near.quote, near.later)
        }
    };
    let expense = db.create_expense(new, &quote)?;
    Ok(SavedExpense {
        expense,
        later_rate,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use invuso_core::Decimal;
    use invuso_core::domain::{Money, Person};
    use invuso_core::fx::Rate;

    use super::*;
    use crate::services::rates::RateError;
    use crate::storage::{NewExchangeRate, NewExpensePayment, Profile};

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn eur_to(quote: &str, value: &str, date: &str) -> NewExchangeRate {
        NewExchangeRate {
            rate: Rate::new(cur("EUR"), cur(quote), Decimal::from_str(value).unwrap()).unwrap(),
            rate_date: date.into(),
        }
    }

    /// Answers `on_date` with fixed rates, or fails like a missing network.
    struct Fake {
        rates: Option<Vec<NewExchangeRate>>,
        calls: AtomicUsize,
    }

    impl Fake {
        fn offline() -> Self {
            Self {
                rates: None,
                calls: AtomicUsize::new(0),
            }
        }

        fn with(rates: Vec<NewExchangeRate>) -> Self {
            Self {
                rates: Some(rates),
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl RateProvider for Fake {
        fn source(&self) -> &'static str {
            "fake"
        }

        fn latest(&self) -> Result<Vec<NewExchangeRate>, RateError> {
            unreachable!("saving never asks for the latest rates")
        }

        fn on_date(&self, _date: &str) -> Result<Vec<NewExchangeRate>, RateError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.rates
                .clone()
                .ok_or_else(|| RateError::Format("offline".into()))
        }
    }

    fn setup() -> (Db, Person) {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&Profile {
            name: "Ich".into(),
            home_currency: cur("EUR"),
            target_language: "de".into(),
        })
        .unwrap();
        let me = db.me().unwrap().unwrap();
        (db, me)
    }

    /// 3 000 ¥ on 3 October, personal (EUR).
    fn ramen(me: &Person) -> NewExpense {
        NewExpense {
            group_id: None,
            title: "Ramen".into(),
            category_id: None,
            occurred_at: "2026-10-03T19:30:00+09:00".into(),
            total: Money::new(3_000, cur("JPY")),
            payments: vec![NewExpensePayment {
                person_id: me.id.clone(),
                payment_method_id: None,
                amount_minor: 3_000,
            }],
            participants: BTreeSet::from([me.id.clone()]),
        }
    }

    #[test]
    fn archived_rate_of_an_earlier_day_needs_no_network() {
        let (db, me) = setup();
        db.archive_rates("frankfurter", 1, &[eur_to("JPY", "160", "2026-10-01")])
            .unwrap();
        let (primary, fallback) = (Fake::offline(), Fake::offline());
        let saved = save_expense(&db, &primary, &fallback, ramen(&me)).unwrap();
        assert_eq!(saved.expense.total_in_base, Money::new(1_875, cur("EUR")));
        assert!(!saved.later_rate);
        assert_eq!(
            primary.calls.load(Ordering::Relaxed) + fallback.calls.load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn missing_day_is_fetched_and_archived() {
        let (db, me) = setup();
        // Only a later rate is archived, e.g. right after installing.
        db.archive_rates("frankfurter", 1, &[eur_to("JPY", "150", "2026-10-04")])
            .unwrap();
        let primary = Fake::with(vec![eur_to("JPY", "160", "2026-10-03")]);
        let saved = save_expense(&db, &primary, &Fake::offline(), ramen(&me)).unwrap();
        assert_eq!(primary.calls.load(Ordering::Relaxed), 1);
        assert!(!saved.later_rate);
        assert_eq!(saved.expense.total_in_base, Money::new(1_875, cur("EUR")));
    }

    #[test]
    fn offline_falls_back_to_a_later_rate() {
        let (db, me) = setup();
        db.archive_rates("frankfurter", 1, &[eur_to("JPY", "150", "2026-10-04")])
            .unwrap();
        let saved = save_expense(&db, &Fake::offline(), &Fake::offline(), ramen(&me)).unwrap();
        assert!(saved.later_rate);
        assert_eq!(saved.expense.total_in_base, Money::new(2_000, cur("EUR")));
    }

    #[test]
    fn no_rate_at_all_is_reported() {
        let (db, me) = setup();
        assert!(matches!(
            save_expense(&db, &Fake::offline(), &Fake::offline(), ramen(&me)),
            Err(SaveExpenseError::NoRate { .. })
        ));
        // The same currency never needs a rate.
        let coffee = NewExpense {
            total: Money::new(350, cur("EUR")),
            payments: vec![NewExpensePayment {
                person_id: me.id.clone(),
                payment_method_id: None,
                amount_minor: 350,
            }],
            ..ramen(&me)
        };
        let saved = save_expense(&db, &Fake::offline(), &Fake::offline(), coffee).unwrap();
        assert_eq!(saved.expense.fx_rate_id, None);
    }
}
