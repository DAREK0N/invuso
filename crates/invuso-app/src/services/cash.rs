//! Booking cash movements that need more than the repository: the fee of
//! a withdrawal becomes an expense with the rate of its day (CASH-03, user
//! decision in AP-22), and the cash is valued in the home currency
//! (CASH-07).

use std::collections::BTreeSet;

use invuso_core::domain::{
    AccountTerms, CashMovementId, CategoryId, Currency, ExpenseSource, Money, PaymentMethod,
    PaymentMethodId, PersonId,
};
use invuso_core::fx;
use invuso_core::split::SplitMode;

use super::expenses::{SaveExpenseError, rate_for_day};
use super::rates::RateProvider;
use crate::storage::{
    Db, NewExpense, NewExpensePayment, NewWithdrawal, StorageError, WithdrawalFee,
};

/// Category of the fee expense: the default "Sonstiges".
const FEE_CATEGORY: &str = "default-other";

/// What a withdrawal with a card starts with (CASH-03, PAY-04).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawalTerms {
    /// "Belastet" is in the card's account currency, else in the home
    /// currency.
    pub charged_in: Currency,
    /// The card's fees; none without a card.
    pub account: AccountTerms,
}

impl WithdrawalTerms {
    pub fn of(card: Option<&PaymentMethod>, home: Currency) -> Self {
        Self {
            charged_in: card.map_or(home, |c| c.charge_currency(home)),
            account: card.map(|c| c.account.clone()).unwrap_or_default(),
        }
    }

    /// The fee field's currency: that of the fixed fee, else the one the
    /// card is charged in, which the foreign fee is a part of.
    pub fn fee_currency(&self) -> Currency {
        self.account
            .fixed_fee
            .map_or(self.charged_in, |f| f.currency())
    }

    /// Whether the card suggests a fee at all.
    pub fn has_fees(&self) -> bool {
        self.account.fixed_fee.is_some() || self.account.foreign_fee_percent.is_some()
    }

    /// The fee suggested for a withdrawal of cash in `cash` currency that
    /// was `charged` (user decision in AP-31): fixed fee plus foreign fee
    /// in percent. Cash in the card's own currency is no foreign
    /// transaction, so only the fixed fee applies.
    pub fn suggested_fee(&self, cash: Currency, charged: Option<Money>) -> Option<Money> {
        let foreign = (cash != self.charged_in)
            .then_some(charged)
            .flatten()
            .filter(|c| c.currency() == self.charged_in);
        self.account.withdrawal_fee(foreign).ok().flatten()
    }
}

/// Books a withdrawal; a fee becomes a personal expense of the person,
/// paid with the card, converted into the home currency like any personal
/// expense. May block on the network for the fee's rate, so it must run on
/// a background thread.
pub fn record_withdrawal(
    db: &Db,
    primary: &dyn RateProvider,
    fallback: &dyn RateProvider,
    new: NewWithdrawal,
    fee: Option<Money>,
    fee_title: &str,
) -> Result<CashMovementId, SaveExpenseError> {
    let fee = match fee.filter(|f| f.amount_minor() > 0) {
        Some(fee) => {
            let home = db.expense_base_currency(None)?;
            let (rate, _) = rate_for_day(
                db,
                primary,
                fallback,
                fee.currency(),
                &new.occurred_at,
                home,
            )?;
            Some(WithdrawalFee {
                expense: fee_expense(
                    &new.person,
                    new.card.clone(),
                    fee,
                    &new.occurred_at,
                    fee_title,
                ),
                rate,
            })
        }
        None => None,
    };
    Ok(db.record_withdrawal(new, fee)?)
}

fn fee_expense(
    person: &PersonId,
    card: Option<PaymentMethodId>,
    fee: Money,
    occurred_at: &str,
    title: &str,
) -> NewExpense {
    NewExpense {
        group_id: None,
        title: title.to_string(),
        category_id: Some(CategoryId::new(FEE_CATEGORY)),
        occurred_at: occurred_at.to_string(),
        total: fee,
        payments: vec![NewExpensePayment {
            person_id: person.clone(),
            payment_method_id: card,
            amount_minor: fee.amount_minor(),
        }],
        split: SplitMode::Equal(BTreeSet::from([person.clone()])),
        receipt_id: None,
        line_items: Vec::new(),
        source: ExpenseSource::Manual,
        note: None,
        location: None,
        coordinates: None,
        own_rate: None,
    }
}

/// All cash of a person in one currency (CASH-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashValue {
    /// Sum of every balance the archive has a rate for.
    pub total: Money,
    /// Currencies left out for want of a rate.
    pub missing: Vec<Currency>,
}

/// Values `balances` in `home` with the newest archived rates (FX-03: the
/// last known rate while offline).
pub fn cash_value(db: &Db, balances: &[Money], home: Currency) -> Result<CashValue, StorageError> {
    let mut total = Money::zero(home);
    let mut missing = Vec::new();
    for balance in balances.iter().filter(|b| !b.is_zero()) {
        let converted = db
            .latest_rate(balance.currency(), home)?
            .and_then(|quote| fx::convert(*balance, &quote.rate).ok());
        match converted.and_then(|money| total.checked_add(money).ok()) {
            Some(sum) => total = sum,
            None => missing.push(balance.currency()),
        }
    }
    Ok(CashValue { total, missing })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use invuso_core::Decimal;
    use invuso_core::domain::PaymentMethodKind;
    use invuso_core::fx::Rate;

    use super::*;
    use crate::services::rates::RateError;
    use crate::storage::{NewExchangeRate, NewPaymentMethod, Profile};

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn eur_to(quote: &str, value: &str, date: &str) -> NewExchangeRate {
        NewExchangeRate {
            rate: Rate::new(cur("EUR"), cur(quote), Decimal::from_str(value).unwrap()).unwrap(),
            rate_date: date.into(),
        }
    }

    struct Offline;

    impl RateProvider for Offline {
        fn source(&self) -> &'static str {
            "offline"
        }

        fn latest(&self) -> Result<Vec<NewExchangeRate>, RateError> {
            Err(RateError::Format("offline".into()))
        }

        fn on_date(&self, _date: &str) -> Result<Vec<NewExchangeRate>, RateError> {
            Err(RateError::Format("offline".into()))
        }
    }

    fn db() -> (Db, PersonId) {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&Profile {
            name: "Ich".into(),
            home_currency: cur("EUR"),
            target_language: "de".into(),
        })
        .unwrap();
        let me = db.me().unwrap().unwrap().id;
        (db, me)
    }

    #[test]
    fn fee_becomes_a_personal_expense_paid_with_the_card() {
        let (db, me) = db();
        let card = db
            .create_payment_method(NewPaymentMethod {
                name: "Visa".into(),
                kind: PaymentMethodKind::CreditCard,
                owner_person_id: Some(me.clone()),
                last4: None,
                account: Default::default(),
                color: "cerulean".into(),
                icon: "credit-card".into(),
            })
            .unwrap()
            .id;
        db.archive_rates("frankfurter", 1, &[eur_to("JPY", "160", "2026-10-03")])
            .unwrap();
        let new = NewWithdrawal {
            person: me.clone(),
            amount: Money::new(30_000, cur("JPY")),
            card: Some(card.clone()),
            charged: None,
            occurred_at: "2026-10-03T10:00:00+09:00".into(),
        };
        // An ATM fee in yen, converted into the home currency.
        record_withdrawal(
            &db,
            &Offline,
            &Offline,
            new.clone(),
            Some(Money::new(220, cur("JPY"))),
            "Abhebegebühr",
        )
        .unwrap();
        let entry = db.cash_entries(&me).unwrap().remove(0);
        let fee = db.expense(&entry.expense_id.unwrap()).unwrap().unwrap();
        assert_eq!(fee.title, "Abhebegebühr");
        assert_eq!(fee.group_id, None);
        assert_eq!(fee.total_in_base, Money::new(138, cur("EUR")));
        assert_eq!(fee.payments[0].payment_method_id, Some(card));
        assert_eq!(
            db.cash_balances(&me).unwrap(),
            [Money::new(30_000, cur("JPY"))]
        );

        // A zero fee books no expense.
        record_withdrawal(
            &db,
            &Offline,
            &Offline,
            new,
            Some(Money::new(0, cur("EUR"))),
            "x",
        )
        .unwrap();
        assert_eq!(db.cash_entries(&me).unwrap()[0].expense_id, None);
    }

    #[test]
    fn withdrawal_with_a_usd_card_charges_and_suggests_the_fee_in_usd() {
        let (db, me) = db();
        let usd = cur("USD");
        let card = db
            .create_payment_method(NewPaymentMethod {
                name: "Chase".into(),
                kind: PaymentMethodKind::CreditCard,
                owner_person_id: Some(me.clone()),
                last4: Some("4242".into()),
                account: AccountTerms {
                    currency: Some(usd),
                    foreign_fee_percent: Some(Decimal::from_str("1.75").unwrap()),
                    fixed_fee: Some(Money::new(500, usd)),
                },
                color: "cerulean".into(),
                icon: "credit-card".into(),
            })
            .unwrap();
        let terms = WithdrawalTerms::of(Some(&card), cur("EUR"));
        assert_eq!(terms.charged_in, usd);
        assert_eq!(terms.fee_currency(), usd);
        let charged = Money::new(20_150, terms.charged_in);
        // 5,00 + 1,75 % of 201,50 (3,52625 → 3,53) = 8,53 USD.
        let fee = terms.suggested_fee(cur("JPY"), Some(charged));
        assert_eq!(fee, Some(Money::new(853, usd)));
        // Dollars from a dollar card: no foreign fee.
        assert_eq!(
            terms.suggested_fee(usd, Some(charged)),
            Some(Money::new(500, usd))
        );
        // Charged amount not known yet: the fixed fee only.
        assert_eq!(
            terms.suggested_fee(cur("JPY"), None),
            Some(Money::new(500, usd))
        );

        db.archive_rates("frankfurter", 1, &[eur_to("USD", "1.25", "2026-10-03")])
            .unwrap();
        let new = NewWithdrawal {
            person: me.clone(),
            amount: Money::new(30_000, cur("JPY")),
            card: Some(card.id.clone()),
            charged: Some(charged),
            occurred_at: "2026-10-03T10:00:00+09:00".into(),
        };
        record_withdrawal(&db, &Offline, &Offline, new, fee, "Abhebegebühr").unwrap();
        let entry = db.cash_entries(&me).unwrap().remove(0);
        assert_eq!(entry.counterpart, Some(charged));
        assert_eq!(entry.fee, Some(Money::new(853, usd)));
        // The fee expense is in USD, converted into the home currency:
        // 8,53 / 1,25 = 6,824 → 6,82 €.
        let fee = db.expense(&entry.expense_id.unwrap()).unwrap().unwrap();
        assert_eq!(fee.total, Money::new(853, usd));
        assert_eq!(fee.total_in_base, Money::new(682, cur("EUR")));
    }

    #[test]
    fn withdrawal_without_card_currency_is_charged_in_the_home_currency() {
        let eur = cur("EUR");
        let no_card = WithdrawalTerms::of(None, eur);
        assert_eq!(no_card.charged_in, eur);
        assert!(!no_card.has_fees());
        assert_eq!(no_card.suggested_fee(cur("JPY"), None), None);
        let card = PaymentMethod {
            id: PaymentMethodId::new("visa"),
            name: "Visa".into(),
            kind: PaymentMethodKind::CreditCard,
            owner_person_id: None,
            last4: None,
            account: AccountTerms {
                currency: None,
                foreign_fee_percent: None,
                fixed_fee: Some(Money::new(250, cur("JPY"))),
            },
            color: "cerulean".into(),
            icon: "credit-card".into(),
            archived: false,
        };
        let terms = WithdrawalTerms::of(Some(&card), eur);
        assert_eq!(terms.charged_in, eur);
        // A fee kept without an account currency stays in its own currency.
        assert_eq!(terms.fee_currency(), cur("JPY"));
    }

    #[test]
    fn fee_without_any_rate_fails_and_books_nothing() {
        let (db, me) = db();
        let new = NewWithdrawal {
            person: me.clone(),
            amount: Money::new(30_000, cur("JPY")),
            card: None,
            charged: None,
            occurred_at: "2026-10-03T10:00:00+09:00".into(),
        };
        assert!(matches!(
            record_withdrawal(
                &db,
                &Offline,
                &Offline,
                new,
                Some(Money::new(220, cur("JPY"))),
                "x"
            ),
            Err(SaveExpenseError::NoRate { .. })
        ));
        assert_eq!(db.cash_entries(&me).unwrap(), []);
    }

    #[test]
    fn values_cash_with_the_newest_rates_and_names_what_is_missing() {
        let (db, _) = db();
        db.archive_rates("frankfurter", 1, &[eur_to("JPY", "160", "2026-10-01")])
            .unwrap();
        db.archive_rates("frankfurter", 2, &[eur_to("JPY", "150", "2026-10-03")])
            .unwrap();
        let value = cash_value(
            &db,
            &[
                Money::new(4_500, cur("EUR")),
                Money::new(30_000, cur("JPY")),
                Money::new(0, cur("USD")),
                Money::new(1_000, cur("THB")),
            ],
            cur("EUR"),
        )
        .unwrap();
        // 45 € + 30 000 / 150 = 245 €; no rate for THB, USD is empty.
        assert_eq!(value.total, Money::new(24_500, cur("EUR")));
        assert_eq!(value.missing, [cur("THB")]);
    }
}
