use std::collections::BTreeSet;

use invuso_core::domain::{
    CategoryId, Currency, Expense, ExpenseId, ExpensePayment, ExpenseSource, GroupId, Money,
    PaymentMethodId, PersonId, local_date, validate_occurred_at, validate_participants,
    validate_payments,
};
use invuso_core::fx;
use invuso_core::split::SplitMode;
use rusqlite::{Connection, OptionalExtension, params};

use super::db::{new_id, now_ms};
use super::exchange_rates::{RateQuote, rate_id_for_expense};
use super::settings::{HOME_CURRENCY, LAST_EXPENSE_CURRENCY, LAST_EXPENSE_GROUP};
use super::{Db, StorageError, categories, groups, settings};

/// One payer of a new expense (EXP-02, EXP-03); the amount is in minor
/// units of the expense's currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewExpensePayment {
    pub person_id: PersonId,
    pub payment_method_id: Option<PaymentMethodId>,
    pub amount_minor: i64,
}

/// Input for recording an expense by hand (EXP-01). Split equally between
/// `participants` (EXP-04, other modes follow in AP-12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewExpense {
    /// `None` for a personal expense (EXP-06).
    pub group_id: Option<GroupId>,
    pub title: String,
    pub category_id: Option<CategoryId>,
    /// Local date and time with UTC offset (`validate_occurred_at`).
    pub occurred_at: String,
    pub total: Money,
    pub payments: Vec<NewExpensePayment>,
    pub participants: BTreeSet<PersonId>,
}

impl Db {
    /// Currency an expense is converted into: the group's base currency, or
    /// the home currency for a personal expense (user decision in AP-11).
    pub fn expense_base_currency(&self, group: Option<&GroupId>) -> Result<Currency, StorageError> {
        self.with(|conn| base_currency(conn, group))
    }

    /// Saves the expense with its payments and shares in one transaction.
    /// `rate` converts the total into the base currency (EXP-07); the
    /// expense keeps the id of the archived rate, so later rates never
    /// change it (FX-04).
    pub fn create_expense(
        &self,
        new: NewExpense,
        rate: &RateQuote,
    ) -> Result<Expense, StorageError> {
        let title = new.title.trim().to_string();
        if title.is_empty() {
            return Err(StorageError::InvalidInput("title must not be empty"));
        }
        validate_occurred_at(&new.occurred_at)?;
        let payments: Vec<(PersonId, i64)> = new
            .payments
            .iter()
            .map(|p| (p.person_id.clone(), p.amount_minor))
            .collect();
        validate_payments(new.total.amount_minor(), &payments)?;
        validate_participants(&new.participants)?;

        let id = ExpenseId::new(new_id());
        let expense = self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            let base = base_currency(&tx, new.group_id.as_ref())?;
            if rate.rate.base() != new.total.currency() || rate.rate.quote() != base {
                return Err(StorageError::InvalidInput("rate does not fit the expense"));
            }
            check_people(&tx, new.group_id.as_ref(), &new)?;
            for payment in &new.payments {
                if let Some(method) = &payment.payment_method_id {
                    check_payment_method(&tx, method)?;
                }
            }
            if let Some(category) = &new.category_id {
                categories::check_category(&tx, category)?;
            }
            let total_in_base = fx::convert(new.total, &rate.rate)
                .map_err(|_| StorageError::InvalidInput("amount cannot be converted"))?;
            let fx_rate_id = rate_id_for_expense(&tx, self.device_id(), rate)?;
            let split = SplitMode::Equal(new.participants.clone());
            let now = now_ms();

            tx.execute(
                "INSERT INTO expense
                     (id, group_id, title, category_id, occurred_at, occurred_date,
                      total_minor, currency, fx_rate_id, total_base_minor, base_currency,
                      split_mode, source, reviewed, created_at, updated_at, origin_device_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1, ?14, ?14, ?15)",
                params![
                    id.as_str(),
                    new.group_id.as_ref().map(GroupId::as_str),
                    title,
                    new.category_id.as_ref().map(CategoryId::as_str),
                    new.occurred_at,
                    local_date(&new.occurred_at),
                    new.total.amount_minor(),
                    new.total.currency().code(),
                    fx_rate_id,
                    total_in_base.amount_minor(),
                    base.code(),
                    split.code(),
                    ExpenseSource::Manual.code(),
                    now,
                    self.device_id()
                ],
            )?;
            for payment in &new.payments {
                tx.execute(
                    "INSERT INTO expense_payment
                         (id, expense_id, person_id, payment_method_id, amount_minor,
                          created_at, updated_at, origin_device_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7)",
                    params![
                        new_id(),
                        id.as_str(),
                        payment.person_id.as_str(),
                        payment.payment_method_id.as_ref().map(PaymentMethodId::as_str),
                        payment.amount_minor,
                        now,
                        self.device_id()
                    ],
                )?;
            }
            // Equal split: everyone weighs 1.
            for person in &new.participants {
                tx.execute(
                    "INSERT INTO expense_share
                         (id, expense_id, person_id, weight, created_at, updated_at, origin_device_id)
                     VALUES (?1, ?2, ?3, '1', ?4, ?4, ?5)",
                    params![new_id(), id.as_str(), person.as_str(), now, self.device_id()],
                )?;
            }
            // Preselection of the next expense form.
            settings::set(
                &tx,
                LAST_EXPENSE_GROUP,
                new.group_id.as_ref().map_or("", GroupId::as_str),
            )?;
            settings::set(&tx, LAST_EXPENSE_CURRENCY, new.total.currency().code())?;
            tx.commit()?;

            Ok(Expense {
                id: id.clone(),
                group_id: new.group_id.clone(),
                title,
                category_id: new.category_id.clone(),
                occurred_at: new.occurred_at.clone(),
                total: new.total,
                fx_rate_id,
                total_in_base,
                split,
                source: ExpenseSource::Manual,
                payments: new
                    .payments
                    .iter()
                    .map(|p| ExpensePayment {
                        person_id: p.person_id.clone(),
                        payment_method_id: p.payment_method_id.clone(),
                        amount: Money::new(p.amount_minor, new.total.currency()),
                    })
                    .collect(),
            })
        })?;
        Ok(expense)
    }

    /// One expense with its payments. Only equal splits exist so far; the
    /// other modes are read once AP-12 can save them.
    pub fn expense(&self, id: &ExpenseId) -> Result<Option<Expense>, StorageError> {
        self.with(|conn| {
            let row = conn
                .query_row(
                    "SELECT group_id, title, category_id, occurred_at, total_minor, currency,
                            fx_rate_id, total_base_minor, base_currency, split_mode, source
                     FROM expense WHERE id = ?1 AND deleted_at IS NULL",
                    [id.as_str()],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, i64>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, String>(9)?,
                            row.get::<_, String>(10)?,
                        ))
                    },
                )
                .optional()?;
            let Some((
                group_id,
                title,
                category_id,
                occurred_at,
                total_minor,
                currency,
                fx_rate_id,
                total_base_minor,
                base_currency,
                split_mode,
                source,
            )) = row
            else {
                return Ok(None);
            };
            let currency = stored_currency(&currency)?;
            let base_currency = stored_currency(&base_currency)?;
            if split_mode != "equal" {
                return Err(StorageError::InvalidInput("split mode not supported yet"));
            }

            let mut statement = conn.prepare(
                "SELECT person_id FROM expense_share
                 WHERE expense_id = ?1 AND deleted_at IS NULL ORDER BY person_id",
            )?;
            let participants = statement
                .query_map([id.as_str()], |row| {
                    Ok(PersonId::new(row.get::<_, String>(0)?))
                })?
                .collect::<Result<BTreeSet<_>, _>>()?;

            let mut statement = conn.prepare(
                "SELECT person_id, payment_method_id, amount_minor FROM expense_payment
                 WHERE expense_id = ?1 AND deleted_at IS NULL ORDER BY created_at, id",
            )?;
            let payments = statement
                .query_map([id.as_str()], |row| {
                    Ok(ExpensePayment {
                        person_id: PersonId::new(row.get::<_, String>(0)?),
                        payment_method_id: row
                            .get::<_, Option<String>>(1)?
                            .map(PaymentMethodId::new),
                        amount: Money::new(row.get(2)?, currency),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(Some(Expense {
                id: id.clone(),
                group_id: group_id.map(GroupId::new),
                title,
                category_id: category_id.map(CategoryId::new),
                occurred_at,
                total: Money::new(total_minor, currency),
                fx_rate_id,
                total_in_base: Money::new(total_base_minor, base_currency),
                split: SplitMode::Equal(participants),
                source: ExpenseSource::from_code(&source)?,
                payments,
            }))
        })
    }
}

fn base_currency(conn: &Connection, group: Option<&GroupId>) -> Result<Currency, StorageError> {
    let code: String = match group {
        Some(group) => {
            groups::check_group(conn, group)?;
            conn.query_row(
                "SELECT base_currency FROM expense_group WHERE id = ?1",
                [group.as_str()],
                |row| row.get(0),
            )?
        }
        None => settings::get(conn, HOME_CURRENCY)?
            .ok_or(StorageError::InvalidInput("home currency missing"))?,
    };
    stored_currency(&code)
}

/// Payers and participants of a group expense must be members of the group;
/// those of a personal expense must at least exist.
fn check_people(
    conn: &Connection,
    group: Option<&GroupId>,
    new: &NewExpense,
) -> Result<(), StorageError> {
    let allowed: BTreeSet<String> = match group {
        Some(group) => {
            let mut statement = conn.prepare(
                "SELECT gm.person_id FROM group_member gm JOIN person p ON p.id = gm.person_id
                 WHERE gm.group_id = ?1 AND gm.deleted_at IS NULL AND p.deleted_at IS NULL",
            )?;
            statement
                .query_map([group.as_str()], |row| row.get(0))?
                .collect::<Result<_, _>>()?
        }
        None => {
            let mut statement = conn.prepare("SELECT id FROM person WHERE deleted_at IS NULL")?;
            statement
                .query_map([], |row| row.get(0))?
                .collect::<Result<_, _>>()?
        }
    };
    let everyone = new
        .payments
        .iter()
        .map(|p| &p.person_id)
        .chain(new.participants.iter());
    for person in everyone {
        if !allowed.contains(person.as_str()) {
            return Err(StorageError::InvalidInput(
                "payers and participants must be members of the group",
            ));
        }
    }
    Ok(())
}

fn check_payment_method(conn: &Connection, id: &PaymentMethodId) -> Result<(), StorageError> {
    conn.query_row(
        "SELECT 1 FROM payment_method WHERE id = ?1 AND deleted_at IS NULL",
        [id.as_str()],
        |_| Ok(()),
    )
    .optional()?
    .ok_or(StorageError::InvalidInput("payment method does not exist"))
}

fn stored_currency(code: &str) -> Result<Currency, StorageError> {
    Currency::from_code(code).map_err(|_| StorageError::InvalidInput("stored currency is unknown"))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use invuso_core::Decimal;
    use invuso_core::domain::{ExpenseError, PaymentMethodKind, Person};
    use invuso_core::fx::Rate;

    use super::*;
    use crate::storage::exchange_rates::CROSS_SOURCE;
    use crate::storage::{NewExchangeRate, NewGroup, NewPaymentMethod, NewPerson, Profile};

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    struct Setup {
        db: Db,
        me: Person,
        anna: Person,
        group: GroupId,
    }

    /// "Ich" (home currency EUR), Anna, and a EUR group with both.
    fn setup(base: &str) -> Setup {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&Profile {
            name: "Ich".into(),
            home_currency: cur("EUR"),
            target_language: "de".into(),
        })
        .unwrap();
        let me = db.me().unwrap().unwrap();
        let anna = db
            .create_person(NewPerson {
                name: "Anna".into(),
                color: "thistle".into(),
                is_me: false,
                note: None,
            })
            .unwrap();
        let group = db
            .create_group(NewGroup {
                name: "Japan".into(),
                icon: "plane".into(),
                color: "cerulean".into(),
                base_currency: cur(base),
                start_date: None,
                end_date: None,
            })
            .unwrap();
        db.add_group_member(&group.id, &anna.id).unwrap();
        Setup {
            db,
            me,
            anna,
            group: group.id,
        }
    }

    fn eur_to(quote: &str, value: &str, date: &str) -> NewExchangeRate {
        NewExchangeRate {
            rate: Rate::new(cur("EUR"), cur(quote), Decimal::from_str(value).unwrap()).unwrap(),
            rate_date: date.into(),
        }
    }

    fn ramen(s: &Setup) -> NewExpense {
        NewExpense {
            group_id: Some(s.group.clone()),
            title: " Ramen ".into(),
            category_id: Some(CategoryId::new("default-food")),
            occurred_at: "2026-10-03T19:30:00+09:00".into(),
            total: Money::new(3_000, cur("JPY")),
            payments: vec![NewExpensePayment {
                person_id: s.me.id.clone(),
                payment_method_id: None,
                amount_minor: 3_000,
            }],
            participants: BTreeSet::from([s.me.id.clone(), s.anna.id.clone()]),
        }
    }

    #[test]
    fn yen_expense_in_euro_group_keeps_its_rate() {
        let s = setup("EUR");
        s.db.archive_rates("frankfurter", 1, &[eur_to("JPY", "160", "2026-10-02")])
            .unwrap();
        let rate =
            s.db.rate_on(cur("JPY"), cur("EUR"), "2026-10-03")
                .unwrap()
                .unwrap();
        let saved = s.db.create_expense(ramen(&s), &rate).unwrap();
        // 3 000 / 160 = 18.75 € exactly.
        assert_eq!(saved.total_in_base, Money::new(1_875, cur("EUR")));
        assert_eq!(saved.title, "Ramen");
        assert_eq!(saved.fx_rate_id.as_deref(), Some(rate.legs[0].id.as_str()));

        // A newer rate changes nothing about the stored expense (FX-04).
        s.db.archive_rates("frankfurter", 2, &[eur_to("JPY", "150", "2026-10-03")])
            .unwrap();
        assert_eq!(s.db.expense(&saved.id).unwrap(), Some(saved));
    }

    #[test]
    fn two_payers_with_their_own_methods() {
        let s = setup("EUR");
        let card =
            s.db.create_payment_method(NewPaymentMethod {
                name: "Visa".into(),
                kind: PaymentMethodKind::CreditCard,
                owner_person_id: Some(s.anna.id.clone()),
                last4: None,
                color: "cerulean".into(),
                icon: "credit-card".into(),
            })
            .unwrap();
        let cash =
            s.db.create_payment_method(NewPaymentMethod {
                name: "Bargeld".into(),
                kind: PaymentMethodKind::Cash,
                owner_person_id: Some(s.me.id.clone()),
                last4: None,
                color: "muted-teal".into(),
                icon: "banknote".into(),
            })
            .unwrap();
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let taxi = NewExpense {
            title: "Taxi".into(),
            category_id: None,
            total: Money::new(4_000, cur("EUR")),
            payments: vec![
                NewExpensePayment {
                    person_id: s.me.id.clone(),
                    payment_method_id: Some(cash.id.clone()),
                    amount_minor: 2_000,
                },
                NewExpensePayment {
                    person_id: s.anna.id.clone(),
                    payment_method_id: Some(card.id.clone()),
                    amount_minor: 2_000,
                },
            ],
            ..ramen(&s)
        };
        let saved = s.db.create_expense(taxi, &rate).unwrap();
        assert_eq!(saved.fx_rate_id, None);
        assert_eq!(saved.total_in_base, Money::new(4_000, cur("EUR")));
        let read = s.db.expense(&saved.id).unwrap().unwrap();
        let methods: Vec<_> = read
            .payments
            .iter()
            .map(|p| p.payment_method_id.clone())
            .collect();
        assert_eq!(methods, [Some(cash.id), Some(card.id)]);
        assert_eq!(
            read.split,
            SplitMode::Equal(BTreeSet::from([s.me.id.clone(), s.anna.id.clone()]))
        );
    }

    #[test]
    fn cross_rate_is_archived_for_the_expense_only() {
        let s = setup("CHF");
        s.db.archive_rates(
            "frankfurter",
            5,
            &[
                eur_to("JPY", "176.99", "2026-10-02"),
                eur_to("CHF", "0.9279", "2026-10-03"),
            ],
        )
        .unwrap();
        let rate =
            s.db.rate_on(cur("JPY"), cur("CHF"), "2026-10-03")
                .unwrap()
                .unwrap();
        assert_eq!(rate.legs.len(), 2);
        let saved = s.db.create_expense(ramen(&s), &rate).unwrap();
        // 3 000 ¥ / 176.99 × 0.9279 = 15.728… CHF
        assert_eq!(saved.total_in_base, Money::new(1_573, cur("CHF")));

        let (source, value): (String, String) =
            s.db.with(|c| {
                Ok(c.query_row(
                    "SELECT source, rate FROM exchange_rate WHERE id = ?1",
                    [saved.fx_rate_id.as_deref().unwrap()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(source, CROSS_SOURCE);
        assert_eq!(Decimal::from_str(&value).unwrap(), rate.rate.value());

        // Newer leg rates win over the archived cross rate afterwards.
        s.db.archive_rates("frankfurter", 6, &[eur_to("CHF", "0.95", "2026-10-04")])
            .unwrap();
        let latest = s.db.latest_rate(cur("JPY"), cur("CHF")).unwrap().unwrap();
        assert_eq!(latest.legs.len(), 2);
        assert_eq!(latest.legs[1].rate_date, "2026-10-04");
    }

    #[test]
    fn personal_expense_uses_home_currency() {
        let s = setup("JPY");
        assert_eq!(s.db.expense_base_currency(None).unwrap(), cur("EUR"));
        assert_eq!(
            s.db.expense_base_currency(Some(&s.group)).unwrap(),
            cur("JPY")
        );
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let coffee = NewExpense {
            group_id: None,
            title: "Kaffee".into(),
            total: Money::new(350, cur("EUR")),
            payments: vec![NewExpensePayment {
                person_id: s.me.id.clone(),
                payment_method_id: None,
                amount_minor: 350,
            }],
            participants: BTreeSet::from([s.me.id.clone()]),
            ..ramen(&s)
        };
        let saved = s.db.create_expense(coffee, &rate).unwrap();
        assert_eq!(saved.group_id, None);
        assert_eq!(
            s.db.setting(LAST_EXPENSE_GROUP).unwrap().as_deref(),
            Some("")
        );
        assert_eq!(
            s.db.setting(LAST_EXPENSE_CURRENCY).unwrap().as_deref(),
            Some("EUR")
        );
    }

    #[test]
    fn rejects_invalid_expenses_and_writes_nothing() {
        let s = setup("EUR");
        s.db.archive_rates("frankfurter", 1, &[eur_to("JPY", "160", "2026-10-02")])
            .unwrap();
        let rate =
            s.db.rate_on(cur("JPY"), cur("EUR"), "2026-10-03")
                .unwrap()
                .unwrap();
        let eur_rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let stranger =
            s.db.create_person(NewPerson {
                name: "Ben".into(),
                color: "pale-oak".into(),
                is_me: false,
                note: None,
            })
            .unwrap();

        let cases: Vec<(NewExpense, &RateQuote)> = vec![
            (
                NewExpense {
                    title: "  ".into(),
                    ..ramen(&s)
                },
                &rate,
            ),
            (
                NewExpense {
                    occurred_at: "2026-10-03".into(),
                    ..ramen(&s)
                },
                &rate,
            ),
            (
                NewExpense {
                    participants: BTreeSet::new(),
                    ..ramen(&s)
                },
                &rate,
            ),
            (
                NewExpense {
                    participants: BTreeSet::from([stranger.id.clone()]),
                    ..ramen(&s)
                },
                &rate,
            ),
            (
                NewExpense {
                    category_id: Some(CategoryId::new("nope")),
                    ..ramen(&s)
                },
                &rate,
            ),
            // EUR → EUR rate for a JPY expense.
            (ramen(&s), &eur_rate),
        ];
        for (new, rate) in cases {
            assert!(s.db.create_expense(new, rate).is_err());
        }
        assert!(matches!(
            s.db.create_expense(
                NewExpense {
                    payments: vec![NewExpensePayment {
                        person_id: s.me.id.clone(),
                        payment_method_id: None,
                        amount_minor: 2_999,
                    }],
                    ..ramen(&s)
                },
                &rate
            ),
            Err(StorageError::Expense(ExpenseError::PaymentsMismatch { .. }))
        ));
        let count: i64 =
            s.db.with(|c| {
                Ok(c.query_row(
                    "SELECT (SELECT count(*) FROM expense) + (SELECT count(*) FROM expense_payment)
                          + (SELECT count(*) FROM expense_share)",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}
