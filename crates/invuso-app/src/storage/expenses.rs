use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use invuso_core::Decimal;
use invuso_core::domain::{
    CategoryId, Currency, Expense, ExpenseId, ExpensePayment, ExpenseSource, GroupId, Money,
    PaymentMethod, PaymentMethodId, Person, PersonId, local_date, validate_occurred_at,
    validate_payments, validate_split,
};
use invuso_core::fx;
use invuso_core::split::SplitMode;
use rusqlite::{Connection, OptionalExtension, params};

use super::db::{new_id, now_ms};
use super::exchange_rates::{RateQuote, rate_id_for_expense};
use super::settings::{HOME_CURRENCY, LAST_EXPENSE_CURRENCY, LAST_EXPENSE_GROUP};
use super::{Db, StorageError, categories, groups, payment_methods, people, settings};

/// One payer of a new expense (EXP-02, EXP-03); the amount is in minor
/// units of the expense's currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewExpensePayment {
    pub person_id: PersonId,
    pub payment_method_id: Option<PaymentMethodId>,
    pub amount_minor: i64,
}

/// Input for recording an expense by hand (EXP-01) or changing one
/// (EXP-05).
#[derive(Debug, Clone, PartialEq)]
pub struct NewExpense {
    /// `None` for a personal expense (EXP-06).
    pub group_id: Option<GroupId>,
    pub title: String,
    pub category_id: Option<CategoryId>,
    /// Local date and time with UTC offset (`validate_occurred_at`).
    pub occurred_at: String,
    pub total: Money,
    pub payments: Vec<NewExpensePayment>,
    /// Who carries the expense and how (EXP-04, idee.md 8.1); exact
    /// amounts are in the expense's currency.
    pub split: SplitMode,
}

/// One payer of an expense as the timeline shows it (GRP-21). Names of
/// people and methods deleted since stay readable (idee.md 1.4 "Nichts
/// geht verloren").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelinePayer {
    pub name: String,
    pub method: Option<String>,
}

/// An expense as the group's timeline shows it (GRP-20, GRP-21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineEntry {
    pub id: ExpenseId,
    pub title: String,
    pub category_id: Option<CategoryId>,
    pub occurred_at: String,
    pub total: Money,
    pub total_in_base: Money,
    /// In the order they were entered.
    pub payers: Vec<TimelinePayer>,
}

/// An expense in the list of latest expenses on Home (HOME-03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentExpense {
    pub id: ExpenseId,
    pub title: String,
    pub category_id: Option<CategoryId>,
    pub occurred_at: String,
    pub total: Money,
    pub total_in_base: Money,
    /// Name of its group; `None` for a personal expense (EXP-06).
    pub group_name: Option<String>,
}

/// The people and payment methods an expense refers to, by id, including
/// those deleted since it was saved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExpenseParties {
    pub people: BTreeMap<PersonId, Person>,
    pub methods: BTreeMap<PaymentMethodId, PaymentMethod>,
}

/// One stored `expense_share` row: person, weight, percent, amount.
type ShareRow = (String, Option<String>, Option<String>, Option<i64>);

/// The columns of a share row to insert, in the order of [`ShareRow`].
type ShareValues<'a> = (&'a PersonId, Option<String>, Option<String>, Option<i64>);

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
        let title = validate_new(&new)?;
        let id = ExpenseId::new(new_id());
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            let (base, fx_rate_id) = prepare(&tx, self.device_id(), &new, rate)?;
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
                    base.amount_minor(),
                    base.currency().code(),
                    new.split.code(),
                    ExpenseSource::Manual.code(),
                    now,
                    self.device_id()
                ],
            )?;
            insert_parts(&tx, self.device_id(), &id, &new, now)?;
            // Preselection of the next expense form.
            settings::set(
                &tx,
                LAST_EXPENSE_GROUP,
                new.group_id.as_ref().map_or("", GroupId::as_str),
            )?;
            settings::set(&tx, LAST_EXPENSE_CURRENCY, new.total.currency().code())?;
            tx.commit()?;
            Ok(saved(
                id.clone(),
                title,
                new,
                fx_rate_id,
                base,
                ExpenseSource::Manual,
            ))
        })
    }

    /// Replaces everything about an expense except its group (moving is
    /// EXP-11) and its source (EXP-05). The old payments and shares are
    /// soft-deleted, so history and sync keep them. `rate` works as in
    /// [`Db::create_expense`]; passing the expense's own archived rate keeps
    /// it (FX-04).
    pub fn update_expense(
        &self,
        id: &ExpenseId,
        new: NewExpense,
        rate: &RateQuote,
    ) -> Result<Expense, StorageError> {
        let title = validate_new(&new)?;
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            let (group_id, source): (Option<String>, String) = tx
                .query_row(
                    "SELECT group_id, source FROM expense WHERE id = ?1 AND deleted_at IS NULL",
                    [id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
                .ok_or(StorageError::NotFound)?;
            if group_id.as_deref() != new.group_id.as_ref().map(GroupId::as_str) {
                return Err(StorageError::InvalidInput(
                    "an expense cannot change its group",
                ));
            }
            let source = ExpenseSource::from_code(&source)?;
            let (base, fx_rate_id) = prepare(&tx, self.device_id(), &new, rate)?;
            let now = now_ms();
            tx.execute(
                "UPDATE expense
                 SET title = ?2, category_id = ?3, occurred_at = ?4, occurred_date = ?5,
                     total_minor = ?6, currency = ?7, fx_rate_id = ?8, total_base_minor = ?9,
                     base_currency = ?10, split_mode = ?11, updated_at = ?12
                 WHERE id = ?1",
                params![
                    id.as_str(),
                    title,
                    new.category_id.as_ref().map(CategoryId::as_str),
                    new.occurred_at,
                    local_date(&new.occurred_at),
                    new.total.amount_minor(),
                    new.total.currency().code(),
                    fx_rate_id,
                    base.amount_minor(),
                    base.currency().code(),
                    new.split.code(),
                    now
                ],
            )?;
            for table in ["expense_payment", "expense_share"] {
                tx.execute(
                    &format!(
                        "UPDATE {table} SET deleted_at = ?2, updated_at = ?2
                         WHERE expense_id = ?1 AND deleted_at IS NULL"
                    ),
                    params![id.as_str(), now],
                )?;
            }
            insert_parts(&tx, self.device_id(), id, &new, now)?;
            tx.commit()?;
            Ok(saved(id.clone(), title, new, fx_rate_id, base, source))
        })
    }

    /// Soft delete (EXP-05): payments and shares stay attached to the row,
    /// so restoring brings the whole expense back (idee.md 4).
    pub fn delete_expense(&self, id: &ExpenseId) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            Ok(conn.execute(
                "UPDATE expense SET deleted_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id.as_str(), now_ms()],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Undoes [`Db::delete_expense`] (undo toast, UI-11).
    pub fn restore_expense(&self, id: &ExpenseId) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            Ok(conn.execute(
                "UPDATE expense SET deleted_at = NULL, updated_at = ?2
                 WHERE id = ?1 AND deleted_at IS NOT NULL",
                params![id.as_str(), now_ms()],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// The group's expenses for the timeline (GRP-20): newest day first,
    /// within a day the latest local time first. An expense added later
    /// with an earlier date sorts in by that date (GRP-23).
    pub fn group_timeline(&self, group: &GroupId) -> Result<Vec<TimelineEntry>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(
                "SELECT id, title, category_id, occurred_at, total_minor, currency,
                        total_base_minor, base_currency
                 FROM expense WHERE group_id = ?1 AND deleted_at IS NULL
                 ORDER BY occurred_date DESC, substr(occurred_at, 12, 8) DESC,
                          created_at DESC, id",
            )?;
            let rows = statement
                .query_map([group.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            // All payers of the group in one query instead of one per row.
            let mut statement = conn.prepare(
                "SELECT ep.expense_id, p.name, pm.name
                 FROM expense_payment ep
                 JOIN expense e ON e.id = ep.expense_id
                 JOIN person p ON p.id = ep.person_id
                 LEFT JOIN payment_method pm ON pm.id = ep.payment_method_id
                 WHERE e.group_id = ?1 AND e.deleted_at IS NULL AND ep.deleted_at IS NULL
                 ORDER BY ep.created_at, ep.id",
            )?;
            let mut payers: BTreeMap<String, Vec<TimelinePayer>> = BTreeMap::new();
            for row in statement.query_map([group.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    TimelinePayer {
                        name: row.get(1)?,
                        method: row.get(2)?,
                    },
                ))
            })? {
                let (expense, payer) = row?;
                payers.entry(expense).or_default().push(payer);
            }

            rows.into_iter()
                .map(
                    |(id, title, category, occurred_at, total, currency, base, base_currency)| {
                        Ok(TimelineEntry {
                            payers: payers.remove(&id).unwrap_or_default(),
                            id: ExpenseId::new(id),
                            title,
                            category_id: category.map(CategoryId::new),
                            occurred_at,
                            total: Money::new(total, stored_currency(&currency)?),
                            total_in_base: Money::new(base, stored_currency(&base_currency)?),
                        })
                    },
                )
                .collect()
        })
    }

    /// The latest `limit` expenses of all groups and personal ones (HOME-03),
    /// in the order of [`Db::group_timeline`]. Expenses of deleted groups
    /// are left out, like the groups themselves.
    pub fn recent_expenses(&self, limit: u32) -> Result<Vec<RecentExpense>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(
                "SELECT e.id, e.title, e.category_id, e.occurred_at, e.total_minor, e.currency,
                        e.total_base_minor, e.base_currency, g.name
                 FROM expense e LEFT JOIN expense_group g ON g.id = e.group_id
                 WHERE e.deleted_at IS NULL AND (e.group_id IS NULL OR g.deleted_at IS NULL)
                 ORDER BY e.occurred_date DESC, substr(e.occurred_at, 12, 8) DESC,
                          e.created_at DESC, e.id
                 LIMIT ?1",
            )?;
            let rows = statement
                .query_map([limit], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(
                    |(
                        id,
                        title,
                        category,
                        occurred_at,
                        total,
                        currency,
                        base,
                        base_currency,
                        group,
                    )| {
                        Ok(RecentExpense {
                            id: ExpenseId::new(id),
                            title,
                            category_id: category.map(CategoryId::new),
                            occurred_at,
                            total: Money::new(total, stored_currency(&currency)?),
                            total_in_base: Money::new(base, stored_currency(&base_currency)?),
                            group_name: group,
                        })
                    },
                )
                .collect()
        })
    }

    /// How many expenses the group has, for the link to its timeline.
    pub fn group_expense_count(&self, group: &GroupId) -> Result<u32, StorageError> {
        self.with(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM expense WHERE group_id = ?1 AND deleted_at IS NULL",
                [group.as_str()],
                |row| row.get(0),
            )?)
        })
    }

    /// Whether the expense exists but is deleted, so a screen showing it
    /// can tell "just deleted" from "never existed".
    pub fn is_expense_deleted(&self, id: &ExpenseId) -> Result<bool, StorageError> {
        self.with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM expense WHERE id = ?1 AND deleted_at IS NOT NULL",
                    [id.as_str()],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })
    }

    /// Everyone who paid or shares the expense and the methods they paid
    /// with (GRP-22), also if deleted since.
    pub fn expense_parties(&self, expense: &Expense) -> Result<ExpenseParties, StorageError> {
        let participants = expense.split.participants();
        let person_ids: BTreeSet<&PersonId> = expense
            .payments
            .iter()
            .map(|p| &p.person_id)
            .chain(participants.iter())
            .collect();
        let method_ids: BTreeSet<&PaymentMethodId> = expense
            .payments
            .iter()
            .filter_map(|p| p.payment_method_id.as_ref())
            .collect();
        self.with(|conn| {
            let mut parties = ExpenseParties::default();
            for id in person_ids {
                if let Some(person) = people::person_any(conn, id)? {
                    parties.people.insert(id.clone(), person);
                }
            }
            for id in method_ids {
                if let Some(method) = payment_methods::method_any(conn, id)? {
                    parties.methods.insert(id.clone(), method);
                }
            }
            Ok(parties)
        })
    }

    /// One expense with its payments and split.
    pub fn expense(&self, id: &ExpenseId) -> Result<Option<Expense>, StorageError> {
        self.with(|conn| Ok(load_expenses(conn, "e.id = ?1", id.as_str())?.pop()))
    }

    /// Every expense of the group with payments and split, for its totals
    /// and balances (GRP-10..14). Three queries, however many expenses.
    pub fn group_expenses(&self, group: &GroupId) -> Result<Vec<Expense>, StorageError> {
        self.with(|conn| load_expenses(conn, "e.group_id = ?1", group.as_str()))
    }
}

/// Loads the expenses matching `filter` (a condition on `expense e` with
/// one parameter) with their payments and splits, oldest first.
fn load_expenses(
    conn: &Connection,
    filter: &str,
    param: &str,
) -> Result<Vec<Expense>, StorageError> {
    let mut statement = conn.prepare(&format!(
        "SELECT e.id, e.group_id, e.title, e.category_id, e.occurred_at, e.total_minor,
                e.currency, e.fx_rate_id, e.total_base_minor, e.base_currency, e.split_mode,
                e.source
         FROM expense e WHERE {filter} AND e.deleted_at IS NULL
         ORDER BY e.occurred_date, substr(e.occurred_at, 12, 8), e.created_at, e.id"
    ))?;
    let rows = statement
        .query_map([param], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut statement = conn.prepare(&format!(
        "SELECT s.expense_id, s.person_id, s.weight, s.percent, s.amount_minor
         FROM expense_share s JOIN expense e ON e.id = s.expense_id
         WHERE {filter} AND e.deleted_at IS NULL AND s.deleted_at IS NULL
         ORDER BY s.person_id"
    ))?;
    let mut shares: BTreeMap<String, Vec<ShareRow>> = BTreeMap::new();
    for row in statement.query_map([param], |row| {
        Ok((
            row.get::<_, String>(0)?,
            (row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?),
        ))
    })? {
        let (expense, share) = row?;
        shares.entry(expense).or_default().push(share);
    }

    let mut statement = conn.prepare(&format!(
        "SELECT p.expense_id, p.person_id, p.payment_method_id, p.amount_minor
         FROM expense_payment p JOIN expense e ON e.id = p.expense_id
         WHERE {filter} AND e.deleted_at IS NULL AND p.deleted_at IS NULL
         ORDER BY p.created_at, p.id"
    ))?;
    // Amounts stay bare until the expense's currency is known.
    let mut payments: BTreeMap<String, Vec<(PersonId, Option<PaymentMethodId>, i64)>> =
        BTreeMap::new();
    for row in statement.query_map([param], |row| {
        Ok((
            row.get::<_, String>(0)?,
            (
                PersonId::new(row.get::<_, String>(1)?),
                row.get::<_, Option<String>>(2)?.map(PaymentMethodId::new),
                row.get::<_, i64>(3)?,
            ),
        ))
    })? {
        let (expense, payment) = row?;
        payments.entry(expense).or_default().push(payment);
    }

    rows.into_iter()
        .map(
            |(
                id,
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
            )| {
                let currency = stored_currency(&currency)?;
                let split = stored_split(&split_mode, shares.remove(&id).unwrap_or_default())?;
                let payments = payments
                    .remove(&id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(person_id, payment_method_id, amount)| ExpensePayment {
                        person_id,
                        payment_method_id,
                        amount: Money::new(amount, currency),
                    })
                    .collect();
                Ok(Expense {
                    id: ExpenseId::new(id),
                    group_id: group_id.map(GroupId::new),
                    title,
                    category_id: category_id.map(CategoryId::new),
                    occurred_at,
                    total: Money::new(total_minor, currency),
                    fx_rate_id,
                    total_in_base: Money::new(total_base_minor, stored_currency(&base_currency)?),
                    split,
                    source: ExpenseSource::from_code(&source)?,
                    payments,
                })
            },
        )
        .collect()
}

/// Checks what needs no database and returns the trimmed title.
fn validate_new(new: &NewExpense) -> Result<String, StorageError> {
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
    validate_split(new.total.amount_minor(), &new.split)?;
    Ok(title)
}

/// Checks the references of `new` and converts its total: returns the
/// total in the base currency and the id of the rate it was converted with.
fn prepare(
    conn: &Connection,
    device_id: &str,
    new: &NewExpense,
    rate: &RateQuote,
) -> Result<(Money, Option<String>), StorageError> {
    let base = base_currency(conn, new.group_id.as_ref())?;
    if rate.rate.base() != new.total.currency() || rate.rate.quote() != base {
        return Err(StorageError::InvalidInput("rate does not fit the expense"));
    }
    check_people(conn, new.group_id.as_ref(), new)?;
    for payment in &new.payments {
        if let Some(method) = &payment.payment_method_id {
            check_payment_method(conn, method)?;
        }
    }
    if let Some(category) = &new.category_id {
        categories::check_category(conn, category)?;
    }
    let total_in_base = fx::convert(new.total, &rate.rate)
        .map_err(|_| StorageError::InvalidInput("amount cannot be converted"))?;
    let fx_rate_id = rate_id_for_expense(conn, device_id, rate)?;
    Ok((total_in_base, fx_rate_id))
}

/// Inserts the payments and the shares of `new` for expense `id`.
fn insert_parts(
    conn: &Connection,
    device_id: &str,
    id: &ExpenseId,
    new: &NewExpense,
    now: i64,
) -> Result<(), StorageError> {
    for payment in &new.payments {
        conn.execute(
            "INSERT INTO expense_payment
                 (id, expense_id, person_id, payment_method_id, amount_minor,
                  created_at, updated_at, origin_device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7)",
            params![
                new_id(),
                id.as_str(),
                payment.person_id.as_str(),
                payment
                    .payment_method_id
                    .as_ref()
                    .map(PaymentMethodId::as_str),
                payment.amount_minor,
                now,
                device_id
            ],
        )?;
    }
    // The inputs of the mode, not the computed shares: those follow from
    // the total and stay exact when it changes (idee.md 4.1 ExpenseShare).
    let decimal = |value: &Decimal| Some(value.normalize().to_string());
    let rows: Vec<ShareValues<'_>> = match &new.split {
        // Equal split: everyone weighs 1.
        SplitMode::Equal(people) => people
            .iter()
            .map(|p| (p, Some("1".to_string()), None, None))
            .collect(),
        SplitMode::Weights(weights) => weights
            .iter()
            .map(|(p, w)| (p, decimal(w), None, None))
            .collect(),
        SplitMode::Percent(percents) => percents
            .iter()
            .map(|(p, v)| (p, None, decimal(v), None))
            .collect(),
        SplitMode::Exact(amounts) => amounts
            .iter()
            .map(|(p, a)| (p, None, None, Some(*a)))
            .collect(),
    };
    for (person, weight, percent, amount) in rows {
        conn.execute(
            "INSERT INTO expense_share
                 (id, expense_id, person_id, weight, percent, amount_minor,
                  created_at, updated_at, origin_device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)",
            params![
                new_id(),
                id.as_str(),
                person.as_str(),
                weight,
                percent,
                amount,
                now,
                device_id
            ],
        )?;
    }
    Ok(())
}

/// The expense as just written.
fn saved(
    id: ExpenseId,
    title: String,
    new: NewExpense,
    fx_rate_id: Option<String>,
    total_in_base: Money,
    source: ExpenseSource,
) -> Expense {
    let currency = new.total.currency();
    Expense {
        id,
        group_id: new.group_id,
        title,
        category_id: new.category_id,
        occurred_at: new.occurred_at,
        total: new.total,
        fx_rate_id,
        total_in_base,
        split: new.split,
        source,
        payments: new
            .payments
            .into_iter()
            .map(|p| ExpensePayment {
                person_id: p.person_id,
                payment_method_id: p.payment_method_id,
                amount: Money::new(p.amount_minor, currency),
            })
            .collect(),
    }
}

/// Rebuilds the split from its stored code and share rows.
fn stored_split(code: &str, rows: Vec<ShareRow>) -> Result<SplitMode, StorageError> {
    let mismatch = || StorageError::InvalidInput("stored share does not fit its split mode");
    let decimal = |text: Option<String>| {
        text.and_then(|t| Decimal::from_str(&t).ok())
            .ok_or_else(mismatch)
    };
    let people = rows
        .into_iter()
        .map(|(person, weight, percent, amount)| (PersonId::new(person), weight, percent, amount));
    Ok(match code {
        "equal" => SplitMode::Equal(people.map(|row| row.0).collect()),
        "weights" => SplitMode::Weights(
            people
                .map(|(person, weight, _, _)| Ok((person, decimal(weight)?)))
                .collect::<Result<_, StorageError>>()?,
        ),
        "percent" => SplitMode::Percent(
            people
                .map(|(person, _, percent, _)| Ok((person, decimal(percent)?)))
                .collect::<Result<_, StorageError>>()?,
        ),
        "exact" => SplitMode::Exact(
            people
                .map(|(person, _, _, amount)| Ok((person, amount.ok_or_else(mismatch)?)))
                .collect::<Result<_, StorageError>>()?,
        ),
        _ => return Err(StorageError::InvalidInput("stored split mode is unknown")),
    })
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
    let participants = new.split.participants();
    let everyone = new
        .payments
        .iter()
        .map(|p| &p.person_id)
        .chain(participants.iter());
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
    use std::collections::BTreeMap;

    use invuso_core::domain::{ExpenseError, PaymentMethodKind, Person};
    use invuso_core::fx::Rate;
    use invuso_core::split::{ExpenseEntry, SplitError, balances};

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
            split: SplitMode::Equal(BTreeSet::from([s.me.id.clone(), s.anna.id.clone()])),
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
            split: SplitMode::Equal(BTreeSet::from([s.me.id.clone()])),
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
                    split: SplitMode::Equal(BTreeSet::new()),
                    ..ramen(&s)
                },
                &rate,
            ),
            (
                NewExpense {
                    split: SplitMode::Equal(BTreeSet::from([stranger.id.clone()])),
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

    fn d(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    /// 40 € paid by "Ich", in the EUR group, split by `split`.
    fn hotel(s: &Setup, split: SplitMode) -> NewExpense {
        NewExpense {
            title: "Hotel".into(),
            total: Money::new(4_000, cur("EUR")),
            payments: vec![NewExpensePayment {
                person_id: s.me.id.clone(),
                payment_method_id: None,
                amount_minor: 4_000,
            }],
            split,
            ..ramen(s)
        }
    }

    fn add_ben(s: &Setup) -> Person {
        let ben =
            s.db.create_person(NewPerson {
                name: "Ben".into(),
                color: "pale-oak".into(),
                is_me: false,
                note: None,
            })
            .unwrap();
        s.db.add_group_member(&s.group, &ben.id).unwrap();
        ben
    }

    #[test]
    fn every_split_mode_is_read_back_as_saved() {
        let s = setup("EUR");
        let ben = add_ben(&s);
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let (me, anna) = (s.me.id.clone(), s.anna.id.clone());
        let modes = [
            SplitMode::Equal(BTreeSet::from([me.clone(), anna.clone()])),
            SplitMode::Weights(BTreeMap::from([
                (me.clone(), d("2")),
                (anna.clone(), d("1.0")),
                (ben.id.clone(), d("0.5")),
            ])),
            SplitMode::Percent(BTreeMap::from([
                (me.clone(), d("70")),
                (anna.clone(), d("30.00")),
            ])),
            SplitMode::Exact(BTreeMap::from([
                (me.clone(), 2_550),
                (anna.clone(), 1_450),
                (ben.id.clone(), 0),
            ])),
        ];
        for mode in modes {
            let saved = s.db.create_expense(hotel(&s, mode.clone()), &rate).unwrap();
            let read = s.db.expense(&saved.id).unwrap().unwrap();
            assert_eq!(read.split, mode);
            assert_eq!(read, saved);
        }
    }

    #[test]
    fn invalid_splits_are_rejected() {
        let s = setup("EUR");
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let (me, anna) = (s.me.id.clone(), s.anna.id.clone());
        let percent = SplitMode::Percent(BTreeMap::from([
            (me.clone(), d("60")),
            (anna.clone(), d("30")),
        ]));
        assert!(matches!(
            s.db.create_expense(hotel(&s, percent), &rate),
            Err(StorageError::Expense(ExpenseError::Split(
                SplitError::PercentNot100(_)
            )))
        ));
        let exact = SplitMode::Exact(BTreeMap::from([(me, 2_000), (anna, 1_999)]));
        assert!(matches!(
            s.db.create_expense(hotel(&s, exact), &rate),
            Err(StorageError::Expense(ExpenseError::Split(
                SplitError::ExactSumMismatch { .. }
            )))
        ));
    }

    /// Balances of the group computed from what the database holds.
    fn group_balances(s: &Setup) -> BTreeMap<PersonId, i64> {
        let entries: Vec<ExpenseEntry> = s
            .db
            .group_timeline(&s.group)
            .unwrap()
            .into_iter()
            .map(|entry| {
                let expense = s.db.expense(&entry.id).unwrap().unwrap();
                ExpenseEntry {
                    payments: expense
                        .payments
                        .iter()
                        .map(|p| (p.person_id.clone(), p.amount.amount_minor()))
                        .collect(),
                    shares: validate_split(expense.total.amount_minor(), &expense.split).unwrap(),
                }
            })
            .collect();
        balances(&entries, &[])
            .unwrap()
            .into_iter()
            .map(|(person, totals)| (person, totals.balance))
            .collect()
    }

    #[test]
    fn editing_replaces_payments_and_shares() {
        let s = setup("EUR");
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let (me, anna) = (s.me.id.clone(), s.anna.id.clone());
        let equal = SplitMode::Equal(BTreeSet::from([me.clone(), anna.clone()]));
        let saved = s.db.create_expense(hotel(&s, equal), &rate).unwrap();
        assert_eq!(
            group_balances(&s),
            BTreeMap::from([(me.clone(), 2_000), (anna.clone(), -2_000)])
        );

        // Now Anna paid 50 € and carries 70 %.
        let changed = NewExpense {
            title: "Hotel Kyoto".into(),
            total: Money::new(5_000, cur("EUR")),
            payments: vec![NewExpensePayment {
                person_id: anna.clone(),
                payment_method_id: None,
                amount_minor: 5_000,
            }],
            split: SplitMode::Percent(BTreeMap::from([
                (me.clone(), d("30")),
                (anna.clone(), d("70")),
            ])),
            ..hotel(&s, SplitMode::Equal(BTreeSet::new()))
        };
        let updated = s.db.update_expense(&saved.id, changed, &rate).unwrap();
        assert_eq!(updated.id, saved.id);
        assert_eq!(updated.total_in_base, Money::new(5_000, cur("EUR")));
        assert_eq!(s.db.expense(&saved.id).unwrap(), Some(updated));
        assert_eq!(
            group_balances(&s),
            BTreeMap::from([(me, -1_500), (anna, 1_500)])
        );

        // The old rows stay as soft-deleted history.
        let (active, deleted): (i64, i64) =
            s.db.with(|c| {
                Ok(c.query_row(
                    "SELECT count(*) FILTER (WHERE deleted_at IS NULL),
                            count(*) FILTER (WHERE deleted_at IS NOT NULL)
                     FROM expense_share WHERE expense_id = ?1",
                    [saved.id.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!((active, deleted), (2, 2));
    }

    #[test]
    fn editing_keeps_group_and_rejects_invalid_input() {
        let s = setup("EUR");
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let equal = SplitMode::Equal(BTreeSet::from([s.me.id.clone()]));
        let saved =
            s.db.create_expense(hotel(&s, equal.clone()), &rate)
                .unwrap();
        let personal = NewExpense {
            group_id: None,
            ..hotel(&s, equal.clone())
        };
        assert!(matches!(
            s.db.update_expense(&saved.id, personal, &rate),
            Err(StorageError::InvalidInput(_))
        ));
        let empty = hotel(&s, SplitMode::Equal(BTreeSet::new()));
        assert!(s.db.update_expense(&saved.id, empty, &rate).is_err());
        assert_eq!(s.db.expense(&saved.id).unwrap(), Some(saved));
        assert!(matches!(
            s.db.update_expense(&ExpenseId::new("nope"), hotel(&s, equal), &rate),
            Err(StorageError::NotFound)
        ));
    }

    #[test]
    fn delete_hides_and_restore_brings_back() {
        let s = setup("EUR");
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let equal = SplitMode::Equal(BTreeSet::from([s.me.id.clone(), s.anna.id.clone()]));
        let older = NewExpense {
            occurred_at: "2026-10-01T12:00:00+09:00".into(),
            ..hotel(&s, equal.clone())
        };
        let first = s.db.create_expense(older, &rate).unwrap();
        let second = s.db.create_expense(hotel(&s, equal), &rate).unwrap();
        let ids = |s: &Setup| -> Vec<ExpenseId> {
            s.db.group_timeline(&s.group)
                .unwrap()
                .into_iter()
                .map(|e| e.id)
                .collect()
        };
        assert_eq!(ids(&s), [second.id.clone(), first.id.clone()]);

        assert!(!s.db.is_expense_deleted(&second.id).unwrap());
        s.db.delete_expense(&second.id).unwrap();
        assert_eq!(ids(&s), std::slice::from_ref(&first.id));
        assert_eq!(s.db.group_expense_count(&s.group).unwrap(), 1);
        assert_eq!(s.db.expense(&second.id).unwrap(), None);
        assert!(s.db.is_expense_deleted(&second.id).unwrap());
        assert!(!s.db.is_expense_deleted(&ExpenseId::new("never")).unwrap());
        assert!(matches!(
            s.db.delete_expense(&second.id),
            Err(StorageError::NotFound)
        ));

        s.db.restore_expense(&second.id).unwrap();
        assert_eq!(s.db.expense(&second.id).unwrap(), Some(second.clone()));
        assert_eq!(ids(&s).len(), 2);
        assert!(!s.db.is_expense_deleted(&second.id).unwrap());
        assert!(matches!(
            s.db.restore_expense(&second.id),
            Err(StorageError::NotFound)
        ));
    }

    #[test]
    fn timeline_sorts_by_day_and_time_and_names_payers() {
        let s = setup("EUR");
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
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
        let equal = SplitMode::Equal(BTreeSet::from([s.me.id.clone(), s.anna.id.clone()]));
        let at = |title: &str, occurred_at: &str| NewExpense {
            title: title.into(),
            occurred_at: occurred_at.into(),
            ..hotel(&s, equal.clone())
        };
        s.db.create_expense(at("Lunch", "2026-10-04T12:00:00+09:00"), &rate)
            .unwrap();
        s.db.create_expense(at("Dinner", "2026-10-04T19:00:00+09:00"), &rate)
            .unwrap();
        // Same local day in another time zone: sorts by local time.
        s.db.create_expense(at("Breakfast", "2026-10-04T08:00:00+02:00"), &rate)
            .unwrap();
        // Added last, but happened the day before (GRP-23).
        let shared = NewExpense {
            payments: vec![
                NewExpensePayment {
                    person_id: s.me.id.clone(),
                    payment_method_id: None,
                    amount_minor: 1_000,
                },
                NewExpensePayment {
                    person_id: s.anna.id.clone(),
                    payment_method_id: Some(card.id.clone()),
                    amount_minor: 3_000,
                },
            ],
            ..at("Late entry", "2026-10-03T21:00:00+09:00")
        };
        s.db.create_expense(shared, &rate).unwrap();

        let timeline = s.db.group_timeline(&s.group).unwrap();
        let titles: Vec<_> = timeline.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Dinner", "Lunch", "Breakfast", "Late entry"]);
        assert_eq!(
            timeline[3].payers,
            [
                TimelinePayer {
                    name: "Ich".into(),
                    method: None
                },
                TimelinePayer {
                    name: "Anna".into(),
                    method: Some("Visa".into())
                },
            ]
        );
        assert_eq!(timeline[3].total_in_base, Money::new(4_000, cur("EUR")));
        assert_eq!(s.db.group_expense_count(&s.group).unwrap(), 4);

        // Names stay readable after the method is deleted.
        s.db.delete_payment_method(&card.id).unwrap();
        let timeline = s.db.group_timeline(&s.group).unwrap();
        assert_eq!(timeline[3].payers[1].method.as_deref(), Some("Visa"));
    }

    #[test]
    fn recent_expenses_span_groups_and_skip_deleted_ones() {
        let s = setup("EUR");
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let other =
            s.db.create_group(NewGroup {
                name: "WG".into(),
                icon: "home".into(),
                color: "cerulean".into(),
                base_currency: cur("EUR"),
                start_date: None,
                end_date: None,
            })
            .unwrap();
        let equal = SplitMode::Equal(BTreeSet::from([s.me.id.clone()]));
        let at = |title: &str, group: Option<&GroupId>, occurred_at: &str| NewExpense {
            group_id: group.cloned(),
            title: title.into(),
            occurred_at: occurred_at.into(),
            ..hotel(&s, equal.clone())
        };
        s.db.create_expense(
            at("Hotel", Some(&s.group), "2026-10-02T15:00:00+09:00"),
            &rate,
        )
        .unwrap();
        s.db.create_expense(at("Kaffee", None, "2026-10-04T09:00:00+02:00"), &rate)
            .unwrap();
        s.db.create_expense(
            at("Miete", Some(&other.id), "2026-10-03T10:00:00+02:00"),
            &rate,
        )
        .unwrap();
        let gone =
            s.db.create_expense(
                at("Taxi", Some(&s.group), "2026-10-05T10:00:00+09:00"),
                &rate,
            )
            .unwrap();
        s.db.delete_expense(&gone.id).unwrap();

        let recent = s.db.recent_expenses(10).unwrap();
        let rows: Vec<_> = recent
            .iter()
            .map(|e| (e.title.as_str(), e.group_name.as_deref()))
            .collect();
        assert_eq!(
            rows,
            [
                ("Kaffee", None),
                ("Miete", Some("WG")),
                ("Hotel", Some("Japan"))
            ]
        );
        assert_eq!(s.db.recent_expenses(1).unwrap().len(), 1);

        s.db.delete_group(&other.id).unwrap();
        let titles: Vec<_> =
            s.db.recent_expenses(10)
                .unwrap()
                .into_iter()
                .map(|e| e.title)
                .collect();
        assert_eq!(titles, ["Kaffee", "Hotel"]);
    }

    #[test]
    fn parties_include_deleted_people_and_methods() {
        let s = setup("EUR");
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let card =
            s.db.create_payment_method(NewPaymentMethod {
                name: "Visa".into(),
                kind: PaymentMethodKind::CreditCard,
                owner_person_id: Some(s.me.id.clone()),
                last4: None,
                color: "cerulean".into(),
                icon: "credit-card".into(),
            })
            .unwrap();
        let mut new = hotel(
            &s,
            SplitMode::Equal(BTreeSet::from([s.me.id.clone(), s.anna.id.clone()])),
        );
        new.payments[0].payment_method_id = Some(card.id.clone());
        let expense = s.db.create_expense(new, &rate).unwrap();

        s.db.delete_payment_method(&card.id).unwrap();
        s.db.delete_person(&s.anna.id).unwrap();
        let parties = s.db.expense_parties(&expense).unwrap();
        assert_eq!(
            parties.people.keys().collect::<BTreeSet<_>>(),
            BTreeSet::from([&s.me.id, &s.anna.id])
        );
        assert_eq!(parties.people[&s.anna.id].name, "Anna");
        assert_eq!(parties.methods[&card.id].name, "Visa");
    }
}
