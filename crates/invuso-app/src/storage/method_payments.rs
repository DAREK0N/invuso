//! Everything paid with one payment method, across groups (PAY-05).

use std::collections::BTreeMap;

use invuso_core::domain::{
    CashMovementKind, Currency, ExpenseId, Money, PaymentMethod, PaymentMethodId,
};
use rusqlite::{Connection, params};

use super::cash::charged;
use super::payment_methods::method_any;
use super::{Db, StorageError};
use crate::clock;

/// What a payment with a method was for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodPaymentSource {
    /// A payer's part of an expense.
    Expense {
        id: ExpenseId,
        title: String,
        /// `None` for a personal expense (EXP-06).
        group_name: Option<String>,
    },
    /// Cash taken out at an ATM with the card (CASH-03); its fee is an
    /// expense of its own and listed as such.
    Withdrawal { cash: Money },
}

/// One payment with a method (PAY-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodPayment {
    pub source: MethodPaymentSource,
    /// Who paid, also if deleted since.
    pub person_name: String,
    /// In the currency it was paid in: the payer's part of an expense in
    /// the expense's currency, for a withdrawal what the card was charged
    /// or, if that is not known, the cash taken out.
    pub amount: Money,
    pub occurred_at: String,
}

impl Db {
    /// Every payment with the method, latest first: payers' parts of group
    /// and personal expenses and withdrawals charged to it. Deleted
    /// expenses, withdrawals and groups are left out.
    pub fn method_payments(
        &self,
        method: &PaymentMethodId,
    ) -> Result<Vec<MethodPayment>, StorageError> {
        self.with(|conn| {
            let mut payments = expense_payments(conn, method)?;
            payments.extend(withdrawals(conn, method)?);
            let mut keyed = payments
                .into_iter()
                .map(|payment| {
                    let at = clock::instant(&payment.occurred_at)
                        .ok_or(StorageError::InvalidInput("invalid stored time"))?;
                    Ok((at, payment))
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
            // Stable: the same time keeps the order of the queries.
            keyed.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
            Ok(keyed.into_iter().map(|(_, payment)| payment).collect())
        })
    }

    /// The methods with these ids, also archived or deleted ones, for
    /// naming what past expenses were paid with (GRP-17).
    pub fn payment_methods_any<'a>(
        &self,
        ids: impl IntoIterator<Item = &'a PaymentMethodId>,
    ) -> Result<BTreeMap<PaymentMethodId, PaymentMethod>, StorageError> {
        self.with(|conn| {
            let mut methods = BTreeMap::new();
            for id in ids {
                if let Some(method) = method_any(conn, id)? {
                    methods.insert(id.clone(), method);
                }
            }
            Ok(methods)
        })
    }
}

fn expense_payments(
    conn: &Connection,
    method: &PaymentMethodId,
) -> Result<Vec<MethodPayment>, StorageError> {
    let mut statement = conn.prepare(
        "SELECT e.id, e.title, g.name, p.amount_minor, e.currency, e.occurred_at, pe.name
         FROM expense_payment p
         JOIN expense e ON e.id = p.expense_id
         JOIN person pe ON pe.id = p.person_id
         LEFT JOIN expense_group g ON g.id = e.group_id
         WHERE p.payment_method_id = ?1
           AND p.deleted_at IS NULL AND e.deleted_at IS NULL
           AND (e.group_id IS NULL OR g.deleted_at IS NULL)
         ORDER BY p.created_at DESC, p.rowid DESC",
    )?;
    let rows = statement
        .query_map([method.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(id, title, group_name, amount, currency, occurred_at, person_name)| {
                Ok(MethodPayment {
                    source: MethodPaymentSource::Expense {
                        id: ExpenseId::new(id),
                        title,
                        group_name,
                    },
                    person_name,
                    amount: Money::new(amount, stored_currency(&currency)?),
                    occurred_at,
                })
            },
        )
        .collect()
}

/// Cash taken out, its currency, when, by whom and the rate archived with
/// it (base, quote, rate).
type WithdrawalRow = (
    i64,
    String,
    String,
    String,
    Option<(String, String, String)>,
);

fn withdrawals(
    conn: &Connection,
    method: &PaymentMethodId,
) -> Result<Vec<MethodPayment>, StorageError> {
    let mut statement = conn.prepare(
        "SELECT m.amount_minor, a.currency, m.occurred_at, pe.name, r.base, r.quote, r.rate
         FROM cash_movement m
         JOIN cash_account a ON a.id = m.cash_account_id
         JOIN person pe ON pe.id = a.owner_person_id
         LEFT JOIN exchange_rate r ON r.id = m.fx_rate_id
         WHERE m.payment_method_id = ?1 AND m.kind = ?2
           AND m.deleted_at IS NULL AND a.deleted_at IS NULL
         ORDER BY m.created_at DESC, m.rowid DESC",
    )?;
    let rows = statement
        .query_map(
            params![method.as_str(), CashMovementKind::Withdrawal.code()],
            |row| {
                let rate = match (row.get(4)?, row.get(5)?, row.get(6)?) {
                    (Some(base), Some(quote), Some(value)) => Some((base, quote, value)),
                    _ => None,
                };
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    rate,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(amount, currency, occurred_at, person_name, rate): WithdrawalRow| {
                let cash = Money::new(amount, stored_currency(&currency)?);
                let amount = match &rate {
                    Some((base, quote, value)) => charged(cash, base, quote, value)?,
                    None => None,
                };
                Ok(MethodPayment {
                    source: MethodPaymentSource::Withdrawal { cash },
                    person_name,
                    amount: amount.unwrap_or(cash),
                    occurred_at,
                })
            },
        )
        .collect()
}

fn stored_currency(code: &str) -> Result<Currency, StorageError> {
    Currency::from_code(code).map_err(|_| StorageError::InvalidInput("unknown stored currency"))
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::{ExpenseSource, Group, PaymentMethodKind, PersonId};
    use invuso_core::split::SplitMode;

    use super::*;
    use crate::storage::{
        NewExpense, NewExpensePayment, NewGroup, NewPaymentMethod, NewPerson, NewWithdrawal,
        Profile,
    };

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn setup() -> (Db, PersonId, PersonId) {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&Profile {
            name: "Ich".into(),
            home_currency: cur("EUR"),
            target_language: "de".into(),
        })
        .unwrap();
        let me = db.me().unwrap().unwrap().id;
        let anna = db
            .create_person(NewPerson {
                name: "Anna".into(),
                color: "thistle".into(),
                is_me: false,
                note: None,
            })
            .unwrap()
            .id;
        (db, me, anna)
    }

    fn method(db: &Db, name: &str, owner: &PersonId) -> PaymentMethodId {
        db.create_payment_method(NewPaymentMethod {
            name: name.into(),
            kind: PaymentMethodKind::CreditCard,
            owner_person_id: Some(owner.clone()),
            last4: None,
            color: "cerulean".into(),
            icon: "credit-card".into(),
        })
        .unwrap()
        .id
    }

    fn group(db: &Db, name: &str) -> Group {
        db.create_group(NewGroup {
            name: name.into(),
            icon: "plane".into(),
            color: "cerulean".into(),
            base_currency: cur("EUR"),
            start_date: None,
            end_date: None,
            target_language: None,
        })
        .unwrap()
    }

    /// `total` in the base currency, `payments` as (person, method, amount).
    fn add(
        db: &Db,
        group: Option<&Group>,
        title: &str,
        total: Money,
        at: &str,
        payments: &[(&PersonId, Option<&PaymentMethodId>, i64)],
    ) -> ExpenseId {
        let base = group.map_or(cur("EUR"), |g| g.base_currency);
        let rate = db.latest_rate(total.currency(), base).unwrap().unwrap();
        let first = payments[0].0.clone();
        db.create_expense(
            NewExpense {
                group_id: group.map(|g| g.id.clone()),
                title: title.into(),
                category_id: None,
                occurred_at: at.into(),
                total,
                payments: payments
                    .iter()
                    .map(|(person, method, amount)| NewExpensePayment {
                        person_id: (*person).clone(),
                        payment_method_id: method.cloned(),
                        amount_minor: *amount,
                    })
                    .collect(),
                split: SplitMode::Equal([first].into()),
                receipt_id: None,
                line_items: Vec::new(),
                source: ExpenseSource::Manual,
                note: None,
                location: None,
                coordinates: None,
                own_rate: None,
            },
            &rate,
        )
        .unwrap()
        .id
    }

    fn titles(payments: &[MethodPayment]) -> Vec<String> {
        payments
            .iter()
            .map(|p| match &p.source {
                MethodPaymentSource::Expense { title, .. } => title.clone(),
                MethodPaymentSource::Withdrawal { .. } => "ATM".into(),
            })
            .collect()
    }

    #[test]
    fn lists_group_and_personal_expenses_and_withdrawals_latest_first() {
        let (db, me, anna) = setup();
        let visa = method(&db, "Visa", &me);
        let amex = method(&db, "Amex", &anna);
        let trip = group(&db, "Japan Reise");
        db.add_group_member(&trip.id, &anna).unwrap();

        // Anna pays her part of the hotel with another card.
        add(
            &db,
            Some(&trip),
            "Hotel",
            Money::new(9_000, cur("EUR")),
            "2026-10-02T20:00:00+09:00",
            &[(&me, Some(&visa), 6_000), (&anna, Some(&amex), 3_000)],
        );
        add(
            &db,
            None,
            "Buch",
            Money::new(1_500, cur("EUR")),
            "2026-10-03T10:00:00+02:00",
            &[(&me, Some(&visa), 1_500)],
        );
        db.record_withdrawal(
            NewWithdrawal {
                person: me.clone(),
                amount: Money::new(10_000, cur("JPY")),
                card: Some(visa.clone()),
                charged: Some(Money::new(6_250, cur("EUR"))),
                occurred_at: "2026-10-01T09:00:00+09:00".into(),
            },
            None,
        )
        .unwrap();

        let payments = db.method_payments(&visa).unwrap();
        assert_eq!(titles(&payments), ["Buch", "Hotel", "ATM"]);
        assert_eq!(payments[1].amount, Money::new(6_000, cur("EUR")));
        assert_eq!(payments[1].person_name, "Ich");
        assert_eq!(
            payments[1].source,
            MethodPaymentSource::Expense {
                id: db.group_expenses(&trip.id).unwrap()[0].id.clone(),
                title: "Hotel".into(),
                group_name: Some("Japan Reise".into()),
            }
        );
        assert!(matches!(
            &payments[0].source,
            MethodPaymentSource::Expense {
                group_name: None,
                ..
            }
        ));
        // The card was charged 62.50 € for 10,000 ¥.
        assert_eq!(payments[2].amount, Money::new(6_250, cur("EUR")));
        assert_eq!(
            payments[2].source,
            MethodPaymentSource::Withdrawal {
                cash: Money::new(10_000, cur("JPY"))
            }
        );

        let other = db.method_payments(&amex).unwrap();
        assert_eq!(titles(&other), ["Hotel"]);
        assert_eq!(other[0].amount, Money::new(3_000, cur("EUR")));
    }

    #[test]
    fn deleted_expenses_and_groups_are_left_out() {
        let (db, me, _) = setup();
        let visa = method(&db, "Visa", &me);
        let trip = group(&db, "Japan Reise");
        let gone = add(
            &db,
            None,
            "Gelöscht",
            Money::new(100, cur("EUR")),
            "2026-10-03T10:00:00+02:00",
            &[(&me, Some(&visa), 100)],
        );
        add(
            &db,
            Some(&trip),
            "In gelöschter Gruppe",
            Money::new(200, cur("EUR")),
            "2026-10-03T11:00:00+02:00",
            &[(&me, Some(&visa), 200)],
        );
        add(
            &db,
            None,
            "Bleibt",
            Money::new(300, cur("EUR")),
            "2026-10-03T12:00:00+02:00",
            &[(&me, Some(&visa), 300)],
        );
        db.delete_expense(&gone).unwrap();
        db.delete_group(&trip.id).unwrap();

        assert_eq!(titles(&db.method_payments(&visa).unwrap()), ["Bleibt"]);
    }

    #[test]
    fn withdrawal_without_charge_shows_the_cash() {
        let (db, me, _) = setup();
        let visa = method(&db, "Visa", &me);
        db.record_withdrawal(
            NewWithdrawal {
                person: me.clone(),
                amount: Money::new(5_000, cur("EUR")),
                card: Some(visa.clone()),
                charged: None,
                occurred_at: "2026-10-01T09:00:00+02:00".into(),
            },
            None,
        )
        .unwrap();
        let payments = db.method_payments(&visa).unwrap();
        assert_eq!(payments.len(), 1);
        assert_eq!(payments[0].amount, Money::new(5_000, cur("EUR")));
    }

    #[test]
    fn deleted_methods_keep_their_names() {
        let (db, me, _) = setup();
        let visa = method(&db, "Visa", &me);
        db.delete_payment_method(&visa).unwrap();
        let methods = db.payment_methods_any([&visa]).unwrap();
        assert_eq!(methods[&visa].name, "Visa");
    }
}
