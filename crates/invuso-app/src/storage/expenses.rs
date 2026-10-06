use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use invuso_core::Decimal;
use invuso_core::domain::{
    CategoryId, Currency, Expense, ExpenseError, ExpenseId, ExpensePayment, ExpenseSource,
    GeoPoint, GroupId, LineItem, LineItemKind, Money, PaymentMethod, PaymentMethodId, Person,
    PersonId, item_lines, local_date, validate_occurred_at, validate_payments, validate_split,
};
use invuso_core::fx::{self, Rate};
use invuso_core::split::SplitMode;
use rusqlite::{Connection, OptionalExtension, params};

use super::db::{new_id, now_ms};
use super::exchange_rates::{RateQuote, insert_manual_rate, rate_id_for_expense};
use super::settings::{HOME_CURRENCY, LAST_EXPENSE_CURRENCY, LAST_EXPENSE_GROUP};
use super::{Db, StorageError, categories, groups, payment_methods, people, receipts, settings};

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
    /// Archived receipt to attach (RCP-03); only read when creating, an
    /// edit keeps the expense's receipt (replacing it is RCP-08).
    pub receipt_id: Option<String>,
    /// Positions in order (idee.md 4.1 `LineItem`), stored with any split;
    /// `SplitMode::Items` must be built from them (`item_lines`).
    pub line_items: Vec<LineItem>,
    /// Only read when creating; an edit keeps how the expense was entered.
    pub source: ExpenseSource,
    /// Free text; blank counts as none (EXP-10).
    pub note: Option<String>,
    /// Place as typed; blank counts as none (EXP-10).
    pub location: Option<String>,
    pub coordinates: Option<GeoPoint>,
    /// A rate the user typed in, e.g. the card statement's (EXP-08),
    /// between the expense's currency and the base currency, in the
    /// direction it was typed (`1 EUR = 166.67 JPY`), so saving it again
    /// gives the very same rate. It is archived as "manual" with the
    /// expense and replaces the rate passed to [`Db::create_expense`].
    pub own_rate: Option<Rate>,
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
    /// Receipt thumbnail, relative to the data directory (GRP-21).
    pub thumbnail_path: Option<String>,
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
    /// Receipt thumbnail, relative to the data directory.
    pub thumbnail_path: Option<String>,
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
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            // Preselection of the next expense form.
            settings::set(
                &tx,
                LAST_EXPENSE_GROUP,
                new.group_id.as_ref().map_or("", GroupId::as_str),
            )?;
            settings::set(&tx, LAST_EXPENSE_CURRENCY, new.total.currency().code())?;
            let expense = insert_expense(&tx, self.device_id(), new, rate)?;
            tx.commit()?;
            Ok(expense)
        })
    }

    /// Replaces everything about an expense except its source (EXP-05),
    /// including its group (moving it, EXP-11; payers and participants
    /// must be members of the new one). The old payments and shares are
    /// soft-deleted, so history and sync keep them. `rate` works as in
    /// [`Db::create_expense`]; passing the expense's own archived rate keeps
    /// it (FX-04).
    pub fn update_expense(
        &self,
        id: &ExpenseId,
        new: NewExpense,
        rate: &RateQuote,
    ) -> Result<Expense, StorageError> {
        let new = tidy(new);
        let title = validate_new(&new)?;
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            let (source, receipt_id, category): (String, Option<String>, Option<String>) = tx
                .query_row(
                    "SELECT source, receipt_id, category_id FROM expense
                     WHERE id = ?1 AND deleted_at IS NULL",
                    [id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?
                .ok_or(StorageError::NotFound)?;
            let source = ExpenseSource::from_code(&source)?;
            let (base, fx_rate_id) =
                prepare(&tx, self.device_id(), &new, rate, category.as_deref())?;
            let now = now_ms();
            tx.execute(
                "UPDATE expense
                 SET title = ?2, category_id = ?3, occurred_at = ?4, occurred_date = ?5,
                     total_minor = ?6, currency = ?7, fx_rate_id = ?8, total_base_minor = ?9,
                     base_currency = ?10, split_mode = ?11, updated_at = ?12, group_id = ?13,
                     note = ?14, location = ?15, latitude = ?16, longitude = ?17
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
                    now,
                    new.group_id.as_ref().map(GroupId::as_str),
                    new.note,
                    new.location,
                    new.coordinates.map(GeoPoint::latitude),
                    new.coordinates.map(GeoPoint::longitude)
                ],
            )?;
            tx.execute(
                "UPDATE line_item_assignment SET deleted_at = ?2, updated_at = ?2
                 WHERE deleted_at IS NULL AND line_item_id IN
                       (SELECT id FROM line_item WHERE expense_id = ?1 AND deleted_at IS NULL)",
                params![id.as_str(), now],
            )?;
            for table in ["expense_payment", "expense_share", "line_item"] {
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
            let new = NewExpense { receipt_id, ..new };
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
                "SELECT e.id, e.title, e.category_id, e.occurred_at, e.total_minor, e.currency,
                        e.total_base_minor, e.base_currency, r.thumbnail_path
                 FROM expense e
                 LEFT JOIN receipt r ON r.id = e.receipt_id AND r.deleted_at IS NULL
                 WHERE e.group_id = ?1 AND e.deleted_at IS NULL
                 ORDER BY e.occurred_date DESC, substr(e.occurred_at, 12, 8) DESC,
                          e.created_at DESC, e.id",
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
                        row.get::<_, Option<String>>(8)?,
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
                    |(
                        id,
                        title,
                        category,
                        occurred_at,
                        total,
                        currency,
                        base,
                        base_currency,
                        thumbnail_path,
                    )| {
                        Ok(TimelineEntry {
                            thumbnail_path,
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
                        e.total_base_minor, e.base_currency, g.name, r.thumbnail_path
                 FROM expense e LEFT JOIN expense_group g ON g.id = e.group_id
                 LEFT JOIN receipt r ON r.id = e.receipt_id AND r.deleted_at IS NULL
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
                        row.get::<_, Option<String>>(9)?,
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
                        thumbnail_path,
                    )| {
                        Ok(RecentExpense {
                            id: ExpenseId::new(id),
                            title,
                            category_id: category.map(CategoryId::new),
                            occurred_at,
                            total: Money::new(total, stored_currency(&currency)?),
                            total_in_base: Money::new(base, stored_currency(&base_currency)?),
                            group_name: group,
                            thumbnail_path,
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

    /// The payment method each person paid an expense with last (PAY-06),
    /// as long as it is neither archived nor deleted. Payments of edited
    /// expenses count from the edit, since editing writes them anew.
    pub fn last_payment_methods(
        &self,
    ) -> Result<BTreeMap<PersonId, PaymentMethodId>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(
                "SELECT p.person_id, p.payment_method_id
                 FROM expense_payment p
                 JOIN expense e ON e.id = p.expense_id
                 JOIN payment_method m ON m.id = p.payment_method_id
                 WHERE p.deleted_at IS NULL AND e.deleted_at IS NULL
                   AND m.deleted_at IS NULL AND m.archived = 0
                 ORDER BY p.created_at, p.rowid",
            )?;
            let mut last = BTreeMap::new();
            for row in statement.query_map([], |row| {
                Ok((
                    PersonId::new(row.get::<_, String>(0)?),
                    PaymentMethodId::new(row.get::<_, String>(1)?),
                ))
            })? {
                let (person, method) = row?;
                // Ordered oldest first, so the newest one stays.
                last.insert(person, method);
            }
            Ok(last)
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
                e.source, e.receipt_id, e.note, e.location, e.latitude, e.longitude
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
                row.get::<_, Option<String>>(12)?,
                (
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<f64>>(15)?,
                    row.get::<_, Option<f64>>(16)?,
                ),
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

    let mut line_items = load_line_items(conn, filter, param)?;

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
                receipt_id,
                (note, location, latitude, longitude),
            )| {
                let currency = stored_currency(&currency)?;
                let coordinates = match (latitude, longitude) {
                    (Some(lat), Some(lon)) => Some(GeoPoint::new(lat, lon)?),
                    _ => None,
                };
                let items = line_items.remove(&id).unwrap_or_default();
                let split =
                    stored_split(&split_mode, shares.remove(&id).unwrap_or_default(), &items)?;
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
                    receipt_id,
                    note,
                    location,
                    coordinates,
                    payments,
                    line_items: items,
                })
            },
        )
        .collect()
}

/// One stored `line_item` row: id, expense, texts (original, translated,
/// user), quantity, unit and total price, kind, confidence, edited.
type LineItemRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    Option<i64>,
    i64,
    String,
    Option<f64>,
    bool,
);

/// The line items of the expenses matching `filter`, by expense id, in
/// receipt order, with their assignments.
fn load_line_items(
    conn: &Connection,
    filter: &str,
    param: &str,
) -> Result<BTreeMap<String, Vec<LineItem>>, StorageError> {
    let number = |text: &str| {
        Decimal::from_str(text).map_err(|_| StorageError::InvalidInput("stored number is invalid"))
    };
    let mut statement = conn.prepare(&format!(
        "SELECT a.line_item_id, a.person_id, a.weight
         FROM line_item_assignment a
         JOIN line_item l ON l.id = a.line_item_id
         JOIN expense e ON e.id = l.expense_id
         WHERE {filter} AND e.deleted_at IS NULL AND l.deleted_at IS NULL
               AND a.deleted_at IS NULL"
    ))?;
    let mut assignments: BTreeMap<String, BTreeMap<PersonId, Decimal>> = BTreeMap::new();
    for row in statement.query_map([param], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (item, person, weight) = row?;
        assignments
            .entry(item)
            .or_default()
            .insert(PersonId::new(person), number(&weight)?);
    }

    let mut statement = conn.prepare(&format!(
        "SELECT l.id, l.expense_id, l.original_text, l.translated_text, l.user_text,
                l.quantity, l.unit_price_minor, l.total_price_minor, l.kind, l.ocr_confidence,
                l.edited_by_user
         FROM line_item l JOIN expense e ON e.id = l.expense_id
         WHERE {filter} AND e.deleted_at IS NULL AND l.deleted_at IS NULL
         ORDER BY l.position, l.id"
    ))?;
    let rows = statement
        .query_map([param], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
            ))
        })?
        .collect::<Result<Vec<LineItemRow>, _>>()?;

    let mut items: BTreeMap<String, Vec<LineItem>> = BTreeMap::new();
    for (
        id,
        expense,
        original,
        translated,
        user,
        quantity,
        unit,
        total,
        kind,
        confidence,
        edited,
    ) in rows
    {
        let kind = LineItemKind::from_code(&kind)
            .map_err(|_| StorageError::InvalidInput("stored line item kind is unknown"))?;
        items.entry(expense).or_default().push(LineItem {
            original_text: original,
            translated_text: translated,
            user_text: user,
            quantity: number(&quantity)?,
            unit_price_minor: unit,
            total_minor: total,
            kind,
            assigned_to: assignments.remove(&id).unwrap_or_default(),
            ocr_confidence: confidence.map(|c| c as f32),
            edited_by_user: edited,
        });
    }
    Ok(items)
}

/// Trims the free texts of `new`; blank ones become `None` (EXP-10).
fn tidy(new: NewExpense) -> NewExpense {
    let tidy = |text: Option<String>| text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    NewExpense {
        note: tidy(new.note),
        location: tidy(new.location),
        ..new
    }
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
    for item in &new.line_items {
        item.validate().map_err(ExpenseError::from)?;
    }
    if let SplitMode::Items { items, .. } = &new.split
        && *items != item_lines(&new.line_items)
    {
        return Err(StorageError::InvalidInput(
            "the split does not match the line items",
        ));
    }
    validate_split(new.total.amount_minor(), &new.split)?;
    Ok(title)
}

/// Inserts a new expense with its parts, like [`Db::create_expense`] but
/// inside the caller's transaction and without changing the preselection
/// of the expense form (e.g. the fee of a cash withdrawal, CASH-03).
pub(super) fn insert_expense(
    conn: &Connection,
    device_id: &str,
    new: NewExpense,
    rate: &RateQuote,
) -> Result<Expense, StorageError> {
    let new = tidy(new);
    let title = validate_new(&new)?;
    let id = ExpenseId::new(new_id());
    let (base, fx_rate_id) = prepare(conn, device_id, &new, rate, None)?;
    if let Some(receipt) = &new.receipt_id {
        receipts::check_unattached(conn, receipt)?;
    }
    let now = now_ms();
    conn.execute(
        "INSERT INTO expense
             (id, group_id, title, category_id, occurred_at, occurred_date,
              total_minor, currency, fx_rate_id, total_base_minor, base_currency,
              split_mode, source, reviewed, created_at, updated_at, origin_device_id,
              receipt_id, note, location, latitude, longitude)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1, ?14, ?14, ?15,
                 ?16, ?17, ?18, ?19, ?20)",
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
            new.source.code(),
            now,
            device_id,
            new.receipt_id,
            new.note,
            new.location,
            new.coordinates.map(GeoPoint::latitude),
            new.coordinates.map(GeoPoint::longitude)
        ],
    )?;
    insert_parts(conn, device_id, &id, &new, now)?;
    if let Some(receipt) = &new.receipt_id {
        // Saving the expense is the user's check of what was read
        // (idee.md 4.1 `Receipt.status`).
        receipts::mark_reviewed(conn, receipt, now)?;
    }
    let source = new.source;
    Ok(saved(id, title, new, fx_rate_id, base, source))
}

/// Checks the references of `new` and converts its total: returns the
/// total in the base currency and the id of the rate it was converted with.
/// `kept_category` is the category an edited expense had so far, which it
/// may keep even if deleted since (EXP-09).
fn prepare(
    conn: &Connection,
    device_id: &str,
    new: &NewExpense,
    rate: &RateQuote,
    kept_category: Option<&str>,
) -> Result<(Money, Option<String>), StorageError> {
    let base = base_currency(conn, new.group_id.as_ref())?;
    let currency = new.total.currency();
    let own = new.own_rate.as_ref();
    let used = match own {
        Some(own) if own.base() == base && own.quote() == currency => own.inverse(),
        Some(own) => *own,
        None => rate.rate,
    };
    if used.base() != currency || used.quote() != base {
        return Err(StorageError::InvalidInput("rate does not fit the expense"));
    }
    check_people(conn, new.group_id.as_ref(), new)?;
    for payment in &new.payments {
        if let Some(method) = &payment.payment_method_id {
            check_payment_method(conn, method)?;
        }
    }
    if let Some(category) = &new.category_id {
        categories::check_category(conn, category, kept_category)?;
    }
    let total_in_base = fx::convert(new.total, &used)
        .map_err(|_| StorageError::InvalidInput("amount cannot be converted"))?;
    let fx_rate_id = match own {
        // Archived with the expense, in the same transaction, for the day
        // it happened; lookups never pick it (FX-10).
        Some(own) => Some(insert_manual_rate(
            conn,
            device_id,
            own,
            local_date(&new.occurred_at),
        )?),
        None => rate_id_for_expense(conn, device_id, rate)?,
    };
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
        // Who shares the unassigned lines; the lines follow below.
        SplitMode::Items { participants, .. } => participants
            .iter()
            .map(|(p, w)| (p, decimal(w), None, None))
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
    for (position, item) in (0_i64..).zip(&new.line_items) {
        let item_id = new_id();
        conn.execute(
            "INSERT INTO line_item
                 (id, expense_id, position, original_text, translated_text, user_text, quantity,
                  unit_price_minor, total_price_minor, kind, ocr_confidence, edited_by_user,
                  created_at, updated_at, origin_device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, ?14)",
            params![
                item_id,
                id.as_str(),
                position,
                item.original_text,
                item.translated_text,
                item.user_text,
                item.quantity.normalize().to_string(),
                item.unit_price_minor,
                item.total_minor,
                item.kind.code(),
                item.ocr_confidence.map(f64::from),
                item.edited_by_user,
                now,
                device_id
            ],
        )?;
        for (person, weight) in &item.assigned_to {
            conn.execute(
                "INSERT INTO line_item_assignment
                     (id, line_item_id, person_id, weight, created_at, updated_at,
                      origin_device_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
                params![
                    new_id(),
                    item_id,
                    person.as_str(),
                    weight.normalize().to_string(),
                    now,
                    device_id
                ],
            )?;
        }
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
        receipt_id: new.receipt_id,
        note: new.note,
        location: new.location,
        coordinates: new.coordinates,
        line_items: new.line_items,
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

/// Rebuilds the split from its stored code and share rows; splitting by
/// items also takes the expense's line items.
fn stored_split(
    code: &str,
    rows: Vec<ShareRow>,
    line_items: &[LineItem],
) -> Result<SplitMode, StorageError> {
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
        "items" => SplitMode::Items {
            participants: people
                .map(|(person, weight, _, _)| Ok((person, decimal(weight)?)))
                .collect::<Result<_, StorageError>>()?,
            items: item_lines(line_items),
        },
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

    use invuso_core::domain::{Category, ExpenseError, PaymentMethod, PaymentMethodKind, Person};
    use invuso_core::fx::Rate;
    use invuso_core::split::{ExpenseEntry, SplitError, balances};

    use super::*;
    use crate::storage::exchange_rates::CROSS_SOURCE;
    use crate::storage::{
        NewCategory, NewExchangeRate, NewGroup, NewPaymentMethod, NewPerson, Profile,
    };

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
                target_language: None,
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
            receipt_id: None,
            line_items: Vec::new(),
            source: ExpenseSource::Manual,
            note: None,
            location: None,
            coordinates: None,
            own_rate: None,
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
    fn receipt_stays_with_its_expense() {
        let s = setup("JPY");
        let rate = s.db.latest_rate(cur("JPY"), cur("JPY")).unwrap().unwrap();
        let receipt =
            s.db.create_receipt("receipts/r.jpg", Some("receipts/r_thumb.jpg"))
                .unwrap();
        let with_receipt = NewExpense {
            receipt_id: Some(receipt.id.clone()),
            ..ramen(&s)
        };
        let saved = s.db.create_expense(with_receipt.clone(), &rate).unwrap();
        assert_eq!(saved.receipt_id.as_deref(), Some(receipt.id.as_str()));
        assert_eq!(s.db.expense(&saved.id).unwrap(), Some(saved.clone()));

        // Thumbnail in the timeline (GRP-21) and on Home.
        let timeline = s.db.group_timeline(&s.group).unwrap();
        assert_eq!(
            timeline[0].thumbnail_path.as_deref(),
            Some("receipts/r_thumb.jpg")
        );
        let recent = s.db.recent_expenses(10).unwrap();
        assert_eq!(
            recent[0].thumbnail_path.as_deref(),
            Some("receipts/r_thumb.jpg")
        );

        // One receipt, one expense.
        assert!(matches!(
            s.db.create_expense(with_receipt, &rate),
            Err(StorageError::InvalidInput(_))
        ));

        // Editing keeps the receipt, whatever the input says (RCP-08 later).
        let edited = s.db.update_expense(&saved.id, ramen(&s), &rate).unwrap();
        assert_eq!(edited.receipt_id.as_deref(), Some(receipt.id.as_str()));
        assert_eq!(s.db.expense(&saved.id).unwrap(), Some(edited));

        let unknown = NewExpense {
            receipt_id: Some("missing".into()),
            ..ramen(&s)
        };
        assert!(matches!(
            s.db.create_expense(unknown, &rate),
            Err(StorageError::NotFound)
        ));
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
    fn editing_moves_the_group_and_rejects_invalid_input() {
        let s = setup("EUR");
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let equal = SplitMode::Equal(BTreeSet::from([s.me.id.clone()]));
        let saved =
            s.db.create_expense(hotel(&s, equal.clone()), &rate)
                .unwrap();
        // Moving it out of the group makes it personal (EXP-11).
        let personal = NewExpense {
            group_id: None,
            ..hotel(&s, equal.clone())
        };
        let moved = s.db.update_expense(&saved.id, personal, &rate).unwrap();
        assert_eq!(moved.group_id, None);
        assert!(s.db.group_timeline(&s.group).unwrap().is_empty());
        let saved =
            s.db.update_expense(&saved.id, hotel(&s, equal.clone()), &rate)
                .unwrap();
        assert_eq!(saved.group_id, Some(s.group.clone()));

        let empty = hotel(&s, SplitMode::Equal(BTreeSet::new()));
        assert!(s.db.update_expense(&saved.id, empty, &rate).is_err());
        assert_eq!(s.db.expense(&saved.id).unwrap(), Some(saved.clone()));
        assert!(matches!(
            s.db.update_expense(&ExpenseId::new("nope"), hotel(&s, equal), &rate),
            Err(StorageError::NotFound)
        ));
    }

    #[test]
    fn last_used_method_per_person() {
        let s = setup("EUR");
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let card = |name: &str, owner: &Person| {
            s.db.create_payment_method(NewPaymentMethod {
                name: name.into(),
                kind: PaymentMethodKind::CreditCard,
                owner_person_id: Some(owner.id.clone()),
                last4: None,
                color: "cerulean".into(),
                icon: "credit-card".into(),
            })
            .unwrap()
        };
        let (visa, amex, annas) = (
            card("Visa", &s.me),
            card("Amex", &s.me),
            card("Anna", &s.anna),
        );
        let paid = |method: &PaymentMethod, person: &Person| NewExpense {
            payments: vec![NewExpensePayment {
                person_id: person.id.clone(),
                payment_method_id: Some(method.id.clone()),
                amount_minor: 4_000,
            }],
            ..hotel(&s, SplitMode::Equal(BTreeSet::from([s.me.id.clone()])))
        };
        assert!(s.db.last_payment_methods().unwrap().is_empty());
        s.db.create_expense(paid(&visa, &s.me), &rate).unwrap();
        let latest = s.db.create_expense(paid(&amex, &s.me), &rate).unwrap();
        s.db.create_expense(paid(&annas, &s.anna), &rate).unwrap();
        let last = s.db.last_payment_methods().unwrap();
        assert_eq!(last.get(&s.me.id), Some(&amex.id));
        assert_eq!(last.get(&s.anna.id), Some(&annas.id));

        // Deleted expenses and archived methods no longer count.
        s.db.delete_expense(&latest.id).unwrap();
        assert_eq!(
            s.db.last_payment_methods().unwrap().get(&s.me.id),
            Some(&visa.id)
        );
        s.db.set_payment_method_archived(&visa.id, true).unwrap();
        assert_eq!(s.db.last_payment_methods().unwrap().get(&s.me.id), None);
    }

    #[test]
    fn note_location_and_coordinates_are_stored() {
        let s = setup("EUR");
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let equal = SplitMode::Equal(BTreeSet::from([s.me.id.clone()]));
        let shinjuku = GeoPoint::new(35.6909, 139.7003).unwrap();
        let saved =
            s.db.create_expense(
                NewExpense {
                    note: Some(
                        " mit Aussicht 
"
                        .into(),
                    ),
                    location: Some(" Shinjuku ".into()),
                    coordinates: Some(shinjuku),
                    ..hotel(&s, equal.clone())
                },
                &rate,
            )
            .unwrap();
        assert_eq!(saved.note.as_deref(), Some("mit Aussicht"));
        let stored = s.db.expense(&saved.id).unwrap().unwrap();
        assert_eq!(stored, saved);
        assert_eq!(stored.location.as_deref(), Some("Shinjuku"));
        assert_eq!(stored.coordinates, Some(shinjuku));

        // Editing can clear all three.
        let cleared =
            s.db.update_expense(&saved.id, hotel(&s, equal), &rate)
                .unwrap();
        assert_eq!(s.db.expense(&saved.id).unwrap(), Some(cleared.clone()));
        assert_eq!(
            (cleared.note, cleared.location, cleared.coordinates),
            (None, None, None)
        );
    }

    #[test]
    fn own_category_stays_on_its_expenses_after_rename_and_delete() {
        let s = setup("EUR");
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let souvenirs =
            s.db.create_category(NewCategory {
                name: "Souvenirs".into(),
                icon: "gift".into(),
                color: "thistle".into(),
            })
            .unwrap();
        let equal = SplitMode::Equal(BTreeSet::from([s.me.id.clone(), s.anna.id.clone()]));
        let new = NewExpense {
            category_id: Some(souvenirs.id.clone()),
            ..hotel(&s, equal)
        };
        let saved = s.db.create_expense(new.clone(), &rate).unwrap();
        assert_eq!(saved.category_id.as_ref(), Some(&souvenirs.id));

        s.db.update_category(&Category {
            name: "Mitbringsel".into(),
            ..souvenirs.clone()
        })
        .unwrap();
        s.db.delete_category(&souvenirs.id).unwrap();

        // The expense still points at it, and it can still be shown.
        let stored = s.db.expense(&saved.id).unwrap().unwrap();
        assert_eq!(stored.category_id.as_ref(), Some(&souvenirs.id));
        let shown =
            s.db.all_categories()
                .unwrap()
                .into_iter()
                .find(|c| c.id == souvenirs.id)
                .unwrap();
        assert_eq!(shown.name, "Mitbringsel");

        // Editing keeps it; a new expense cannot choose it any more.
        let edited = NewExpense {
            title: "Fächer".into(),
            ..new.clone()
        };
        s.db.update_expense(&saved.id, edited, &rate).unwrap();
        assert_eq!(
            s.db.expense(&saved.id).unwrap().unwrap().category_id,
            Some(souvenirs.id.clone())
        );
        assert!(matches!(
            s.db.create_expense(new, &rate),
            Err(StorageError::InvalidInput(_))
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
                target_language: None,
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

    fn line(text: &str, quantity: &str, total: i64, kind: LineItemKind) -> LineItem {
        LineItem {
            original_text: text.into(),
            quantity: d(quantity),
            unit_price_minor: None,
            total_minor: total,
            kind,
            ocr_confidence: Some(0.9),
            ..LineItem::default()
        }
    }

    /// 6 beers (me 2, Anna 1, Ben 3), shared crisps and a printed subtotal
    /// that must not count; 33 € paid by me (idee.md 7.2, 8.2).
    fn scanned(s: &Setup, ben: &PersonId, receipt: &str) -> NewExpense {
        let mut beer = line("Bier", "6", 2_700, LineItemKind::Article);
        beer.unit_price_minor = Some(450);
        beer.assigned_to = BTreeMap::from([
            (s.me.id.clone(), d("2")),
            (s.anna.id.clone(), d("1")),
            (ben.clone(), d("3")),
        ]);
        let mut crisps = line("Chips", "1", 600, LineItemKind::Article);
        crisps.user_text = Some("Paprika-Chips".into());
        crisps.edited_by_user = true;
        let line_items = vec![
            beer,
            crisps,
            line("Zwischensumme", "1", 3_300, LineItemKind::Ignored),
        ];
        let participants: BTreeMap<_, _> = [&s.me.id, &s.anna.id, ben]
            .into_iter()
            .map(|p| (p.clone(), Decimal::ONE))
            .collect();
        NewExpense {
            title: "Kiosk".into(),
            total: Money::new(3_300, cur("EUR")),
            payments: vec![NewExpensePayment {
                person_id: s.me.id.clone(),
                payment_method_id: None,
                amount_minor: 3_300,
            }],
            split: SplitMode::Items {
                participants,
                items: item_lines(&line_items),
            },
            line_items,
            receipt_id: Some(receipt.to_string()),
            source: ExpenseSource::Scan,
            ..ramen(s)
        }
    }

    #[test]
    fn line_items_and_their_assignments_are_saved() {
        let s = setup("EUR");
        let ben = add_ben(&s).id;
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let receipt = s.db.create_receipt("receipts/k.jpg", None).unwrap();
        s.db.save_receipt_text(
            &receipt.id,
            &crate::storage::ReceiptText::new("test", Vec::new(), 0.0),
        )
        .unwrap();

        let saved =
            s.db.create_expense(scanned(&s, &ben, &receipt.id), &rate)
                .unwrap();
        assert_eq!(saved.source, ExpenseSource::Scan);
        assert_eq!(s.db.expense(&saved.id).unwrap(), Some(saved.clone()));
        assert_eq!(saved.line_items[1].text(), "Paprika-Chips");
        assert_eq!(
            group_balances(&s),
            BTreeMap::from([
                (s.me.id.clone(), 3_300 - 1_100),
                (s.anna.id.clone(), -650),
                (ben.clone(), -1_550),
            ])
        );
        let status: String =
            s.db.with(|c| {
                Ok(c.query_row(
                    "SELECT status FROM receipt WHERE id = ?1",
                    [&receipt.id],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(status, "reviewed");

        // Ben's beers go to Anna; the crisps are dropped.
        let mut changed = scanned(&s, &ben, &receipt.id);
        changed.line_items.remove(1);
        changed.line_items[0].assigned_to = BTreeMap::from([(s.anna.id.clone(), d("1"))]);
        changed.total = Money::new(2_700, cur("EUR"));
        changed.payments[0].amount_minor = 2_700;
        changed.split = SplitMode::Items {
            participants: BTreeMap::from([(s.me.id.clone(), Decimal::ONE)]),
            items: item_lines(&changed.line_items),
        };
        let updated = s.db.update_expense(&saved.id, changed, &rate).unwrap();
        assert_eq!(updated.source, ExpenseSource::Scan);
        assert_eq!(s.db.expense(&saved.id).unwrap(), Some(updated));
        assert_eq!(
            group_balances(&s),
            BTreeMap::from([(s.me.id.clone(), 2_700), (s.anna.id.clone(), -2_700)])
        );
        let (items, assignments): (i64, i64) =
            s.db.with(|c| {
                Ok(c.query_row(
                    "SELECT (SELECT count(*) FROM line_item WHERE deleted_at IS NULL),
                            (SELECT count(*) FROM line_item_assignment WHERE deleted_at IS NULL)",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!((items, assignments), (2, 1));
    }

    #[test]
    fn split_and_line_items_must_agree() {
        let s = setup("EUR");
        let ben = add_ben(&s).id;
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let receipt = s.db.create_receipt("receipts/k.jpg", None).unwrap();

        let mut stale = scanned(&s, &ben, &receipt.id);
        stale.line_items.remove(1);
        assert!(matches!(
            s.db.create_expense(stale, &rate),
            Err(StorageError::InvalidInput(_))
        ));

        let mut wrong_sign = scanned(&s, &ben, &receipt.id);
        wrong_sign.line_items[1].kind = LineItemKind::Discount;
        wrong_sign.split = SplitMode::Items {
            participants: BTreeMap::from([(s.me.id.clone(), Decimal::ONE)]),
            items: item_lines(&wrong_sign.line_items),
        };
        assert!(matches!(
            s.db.create_expense(wrong_sign, &rate),
            Err(StorageError::Expense(ExpenseError::LineItem(_)))
        ));

        // Assigned people must belong to the group like everyone else.
        let stranger =
            s.db.create_person(NewPerson {
                name: "Fremd".into(),
                color: "thistle".into(),
                is_me: false,
                note: None,
            })
            .unwrap();
        let mut outsider = scanned(&s, &ben, &receipt.id);
        outsider.line_items[0].assigned_to = BTreeMap::from([(stranger.id, Decimal::ONE)]);
        outsider.split = SplitMode::Items {
            participants: BTreeMap::from([(s.me.id.clone(), Decimal::ONE)]),
            items: item_lines(&outsider.line_items),
        };
        assert!(matches!(
            s.db.create_expense(outsider, &rate),
            Err(StorageError::InvalidInput(_))
        ));
    }
}
