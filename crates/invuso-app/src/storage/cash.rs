//! Cash on hand per person and currency (idee.md 5.10 `CASH`). Every person
//! has their own cash (user decision in AP-22); there is one
//! `cash_account` row per person and currency, created with its first
//! stored movement.
//!
//! Cash payments are not stored as movements: they are read live from the
//! expenses (payer + method of kind "Bargeld", in the expense's currency),
//! so editing or deleting an expense changes the cash at once (CASH-02).

use std::collections::BTreeMap;
use std::str::FromStr;

use invuso_core::Decimal;
use invuso_core::domain::{
    CashError, CashMovementId, CashMovementKind, Currency, ExpenseId, Money, PaymentMethodId,
    PaymentMethodKind, PersonId, cash_balances, cash_correction, local_date, validate_occurred_at,
};
use invuso_core::fx::{self, Rate};
use rusqlite::{Connection, OptionalExtension, params};

use super::db::{new_id, now_ms};
use super::exchange_rates::insert_manual_rate;
use super::expenses::insert_expense;
use super::{Db, NewExpense, RateQuote, StorageError, people};
use crate::clock;

/// A withdrawal from an ATM (CASH-03).
#[derive(Debug, Clone, PartialEq)]
pub struct NewWithdrawal {
    pub person: PersonId,
    /// The cash taken out.
    pub amount: Money,
    pub card: Option<PaymentMethodId>,
    /// What the card was charged, in the card's currency; gives the actual
    /// rate. `None` if not known (yet) or in the same currency.
    pub charged: Option<Money>,
    pub occurred_at: String,
}

/// The fee of a withdrawal, saved as an expense of its own (user decision
/// in AP-22), with the rate that converts it into the base currency.
#[derive(Debug, Clone, PartialEq)]
pub struct WithdrawalFee {
    pub expense: NewExpense,
    pub rate: RateQuote,
}

/// Cash changed into another currency (CASH-04), e.g. 200 € → 31 000 ¥.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewExchange {
    pub person: PersonId,
    pub given: Money,
    pub received: Money,
    pub occurred_at: String,
}

/// One line of a person's cash (CASH-06): a stored movement or a cash
/// payment of an expense.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashEntry {
    pub kind: CashMovementKind,
    /// Signed: positive adds cash, negative takes it away.
    pub amount: Money,
    pub occurred_at: String,
    /// `None` for a cash payment, which belongs to its expense.
    pub movement_id: Option<CashMovementId>,
    /// The paid expense, or the fee expense of a withdrawal.
    pub expense_id: Option<ExpenseId>,
    /// Title of the paid expense.
    pub title: Option<String>,
    /// The charged card of a withdrawal, the cash method of a payment.
    pub method: Option<String>,
    /// What the card was charged (withdrawal) or the other side of an
    /// exchange.
    pub counterpart: Option<Money>,
    pub fee: Option<Money>,
}

/// One stored movement as read from the database.
struct StoredRow {
    id: String,
    kind: String,
    amount_minor: i64,
    currency: String,
    occurred_at: String,
    expense_id: Option<String>,
    fee_minor: Option<i64>,
    fee_currency: Option<String>,
    method: Option<String>,
    fx_rate_id: Option<String>,
    rate: Option<(String, String, String)>,
    created_at: i64,
}

impl Db {
    /// Everything that changed the person's cash, latest first (CASH-06).
    pub fn cash_entries(&self, person: &PersonId) -> Result<Vec<CashEntry>, StorageError> {
        self.with(|conn| entries(conn, person))
    }

    /// The person's cash per currency (CASH-01), in currency order.
    pub fn cash_balances(&self, person: &PersonId) -> Result<Vec<Money>, StorageError> {
        let entries = self.cash_entries(person)?;
        Ok(cash_balances(entries.iter().map(|e| e.amount))?
            .into_values()
            .collect())
    }

    /// Books a withdrawal (CASH-03) and, if there is one, its fee as an
    /// expense, both in one transaction.
    pub fn record_withdrawal(
        &self,
        new: NewWithdrawal,
        fee: Option<WithdrawalFee>,
    ) -> Result<CashMovementId, StorageError> {
        validate_occurred_at(&new.occurred_at)?;
        positive(new.amount)?;
        let rate = match new.charged {
            Some(charged) if charged.currency() != new.amount.currency() => {
                positive(charged)?;
                Some(
                    fx::implied_rate(new.amount, charged)
                        .map_err(|_| StorageError::InvalidInput("rate cannot be computed"))?,
                )
            }
            _ => None,
        };
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            check_person(&tx, &new.person)?;
            if let Some(card) = &new.card {
                check_method(&tx, card)?;
            }
            let rate_id = rate
                .map(|rate| {
                    insert_manual_rate(&tx, self.device_id(), &rate, local_date(&new.occurred_at))
                })
                .transpose()?;
            let fee = fee
                .map(|fee| insert_expense(&tx, self.device_id(), fee.expense, &fee.rate))
                .transpose()?;
            let id = insert_movement(
                &tx,
                self.device_id(),
                &new.person,
                CashMovementKind::Withdrawal,
                new.amount,
                &new.occurred_at,
                Extras {
                    expense_id: fee.as_ref().map(|e| e.id.as_str()),
                    fx_rate_id: rate_id.as_deref(),
                    fee: fee.as_ref().map(|e| e.total),
                    card: new.card.as_ref(),
                },
            )?;
            tx.commit()?;
            Ok(id)
        })
    }

    /// Books an exchange (CASH-04): the given cash leaves its currency, the
    /// received cash arrives in the other, and the actual rate is archived.
    /// Returns the id of the received side.
    pub fn record_exchange(&self, new: NewExchange) -> Result<CashMovementId, StorageError> {
        validate_occurred_at(&new.occurred_at)?;
        positive(new.given)?;
        positive(new.received)?;
        if new.given.currency() == new.received.currency() {
            return Err(CashError::SameCurrency.into());
        }
        let rate = fx::implied_rate(new.given, new.received)
            .map_err(|_| StorageError::InvalidInput("rate cannot be computed"))?;
        let given = new.given.checked_neg().map_err(CashError::from)?;
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            check_person(&tx, &new.person)?;
            let rate_id =
                insert_manual_rate(&tx, self.device_id(), &rate, local_date(&new.occurred_at))?;
            let extras = Extras {
                fx_rate_id: Some(&rate_id),
                ..Extras::default()
            };
            insert_movement(
                &tx,
                self.device_id(),
                &new.person,
                CashMovementKind::Exchange,
                given,
                &new.occurred_at,
                extras,
            )?;
            let received = insert_movement(
                &tx,
                self.device_id(),
                &new.person,
                CashMovementKind::Exchange,
                new.received,
                &new.occurred_at,
                extras,
            )?;
            tx.commit()?;
            Ok(received)
        })
    }

    /// The person's cash in `currency` at the time `occurred_at`: what a
    /// cash count at that time is compared with (CASH-05). Times with
    /// another UTC offset count by when they happened, so a payment in
    /// Tokyo at 12:30 is before a count in Berlin at 08:13 the same day.
    pub fn cash_balance_at(
        &self,
        person: &PersonId,
        currency: Currency,
        occurred_at: &str,
    ) -> Result<Money, StorageError> {
        validate_occurred_at(occurred_at)?;
        self.with(|conn| balance_at(conn, person, currency, occurred_at))
    }

    /// Books a cash count (CASH-05): the difference between what the person
    /// counted and what the app expected at that time becomes a correction.
    /// Returns the correction, `None` if the cash was as expected.
    pub fn record_cash_count(
        &self,
        person: &PersonId,
        counted: Money,
        occurred_at: &str,
    ) -> Result<Option<(CashMovementId, Money)>, StorageError> {
        validate_occurred_at(occurred_at)?;
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            check_person(&tx, person)?;
            let expected = balance_at(&tx, person, counted.currency(), occurred_at)?;
            let Some(correction) = cash_correction(expected, counted)? else {
                return Ok(None);
            };
            let id = insert_movement(
                &tx,
                self.device_id(),
                person,
                CashMovementKind::Correction,
                correction,
                occurred_at,
                Extras::default(),
            )?;
            tx.commit()?;
            Ok(Some((id, correction)))
        })
    }

    /// Soft delete (idee.md 4). Takes along what belongs to the movement:
    /// the other side of an exchange and the fee expense of a withdrawal.
    pub fn delete_cash_movement(&self, id: &CashMovementId) -> Result<(), StorageError> {
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            let (kind, rate_id, expense_id) = movement_links(&tx, id, false)?;
            let now = now_ms();
            tx.execute(
                "UPDATE cash_movement SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1",
                params![id.as_str(), now],
            )?;
            if kind == CashMovementKind::Exchange.code() {
                tx.execute(
                    "UPDATE cash_movement SET deleted_at = ?2, updated_at = ?2
                     WHERE fx_rate_id = ?1 AND kind = ?3 AND deleted_at IS NULL",
                    params![rate_id, now, CashMovementKind::Exchange.code()],
                )?;
            }
            if let Some(expense) = expense_id {
                tx.execute(
                    "UPDATE expense SET deleted_at = ?2, updated_at = ?2
                     WHERE id = ?1 AND deleted_at IS NULL",
                    params![expense, now],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Undoes [`Db::delete_cash_movement`] (undo toast, UI-11). Brings back
    /// only what was deleted together with the movement.
    pub fn restore_cash_movement(&self, id: &CashMovementId) -> Result<(), StorageError> {
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            let (kind, rate_id, expense_id) = movement_links(&tx, id, true)?;
            let deleted_at: i64 = tx.query_row(
                "SELECT deleted_at FROM cash_movement WHERE id = ?1",
                [id.as_str()],
                |row| row.get(0),
            )?;
            let now = now_ms();
            tx.execute(
                "UPDATE cash_movement SET deleted_at = NULL, updated_at = ?2 WHERE id = ?1",
                params![id.as_str(), now],
            )?;
            if kind == CashMovementKind::Exchange.code() {
                tx.execute(
                    "UPDATE cash_movement SET deleted_at = NULL, updated_at = ?2
                     WHERE fx_rate_id = ?1 AND kind = ?3 AND deleted_at = ?4",
                    params![rate_id, now, CashMovementKind::Exchange.code(), deleted_at],
                )?;
            }
            if let Some(expense) = expense_id {
                tx.execute(
                    "UPDATE expense SET deleted_at = NULL, updated_at = ?2
                     WHERE id = ?1 AND deleted_at = ?3",
                    params![expense, now, deleted_at],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }
}

/// What a stored movement may point at besides its amount.
#[derive(Debug, Clone, Copy, Default)]
struct Extras<'a> {
    expense_id: Option<&'a str>,
    fx_rate_id: Option<&'a str>,
    fee: Option<Money>,
    card: Option<&'a PaymentMethodId>,
}

fn balance_at(
    conn: &Connection,
    person: &PersonId,
    currency: Currency,
    occurred_at: &str,
) -> Result<Money, StorageError> {
    let until = instant(occurred_at)?;
    let mut balance = Money::zero(currency);
    for entry in entries(conn, person)? {
        if entry.amount.currency() == currency && instant(&entry.occurred_at)? <= until {
            balance = balance.checked_add(entry.amount).map_err(CashError::from)?;
        }
    }
    Ok(balance)
}

fn instant(occurred_at: &str) -> Result<i64, StorageError> {
    clock::instant(occurred_at).ok_or(StorageError::InvalidInput("invalid stored time"))
}

fn positive(amount: Money) -> Result<(), StorageError> {
    if amount.amount_minor() <= 0 {
        return Err(CashError::NonPositiveAmount.into());
    }
    Ok(())
}

fn check_person(conn: &Connection, person: &PersonId) -> Result<(), StorageError> {
    match people::person_any(conn, person)? {
        Some(_) => Ok(()),
        None => Err(StorageError::NotFound),
    }
}

fn check_method(conn: &Connection, id: &PaymentMethodId) -> Result<(), StorageError> {
    conn.query_row(
        "SELECT 1 FROM payment_method WHERE id = ?1 AND deleted_at IS NULL",
        [id.as_str()],
        |_| Ok(()),
    )
    .optional()?
    .ok_or(StorageError::InvalidInput("payment method does not exist"))
}

/// Kind, rate id and expense id of a movement that is (`deleted`) or is
/// not deleted.
fn movement_links(
    conn: &Connection,
    id: &CashMovementId,
    deleted: bool,
) -> Result<(String, Option<String>, Option<String>), StorageError> {
    let filter = if deleted {
        "deleted_at IS NOT NULL"
    } else {
        "deleted_at IS NULL"
    };
    conn.query_row(
        &format!(
            "SELECT kind, fx_rate_id, expense_id FROM cash_movement WHERE id = ?1 AND {filter}"
        ),
        [id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .optional()?
    .ok_or(StorageError::NotFound)
}

/// The person's account in `currency`, created on first use.
fn account_id(
    conn: &Connection,
    device_id: &str,
    person: &PersonId,
    currency: Currency,
) -> Result<String, StorageError> {
    let existing = conn
        .query_row(
            "SELECT id FROM cash_account
             WHERE owner_person_id = ?1 AND currency = ?2 AND group_id IS NULL
               AND deleted_at IS NULL",
            params![person.as_str(), currency.code()],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO cash_account
             (id, currency, owner_person_id, group_id, name, created_at, updated_at,
              origin_device_id)
         VALUES (?1, ?2, ?3, NULL, ?2, ?4, ?4, ?5)",
        params![id, currency.code(), person.as_str(), now, device_id],
    )?;
    Ok(id)
}

fn insert_movement(
    conn: &Connection,
    device_id: &str,
    person: &PersonId,
    kind: CashMovementKind,
    amount: Money,
    occurred_at: &str,
    extras: Extras<'_>,
) -> Result<CashMovementId, StorageError> {
    let account = account_id(conn, device_id, person, amount.currency())?;
    let id = new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO cash_movement
             (id, cash_account_id, kind, amount_minor, occurred_at, expense_id, fx_rate_id,
              fee_minor, fee_currency, payment_method_id, created_at, updated_at,
              origin_device_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?12)",
        params![
            id,
            account,
            kind.code(),
            amount.amount_minor(),
            occurred_at,
            extras.expense_id,
            extras.fx_rate_id,
            extras.fee.map(|f| f.amount_minor()),
            extras.fee.map(|f| f.currency().code()),
            extras.card.map(PaymentMethodId::as_str),
            now,
            device_id
        ],
    )?;
    Ok(CashMovementId::new(id))
}

fn entries(conn: &Connection, person: &PersonId) -> Result<Vec<CashEntry>, StorageError> {
    let mut statement = conn.prepare(
        "SELECT m.id, m.kind, m.amount_minor, a.currency, m.occurred_at, m.expense_id,
                m.fee_minor, m.fee_currency, pm.name, m.fx_rate_id,
                r.base, r.quote, r.rate, m.created_at
         FROM cash_movement m
         JOIN cash_account a ON a.id = m.cash_account_id
         LEFT JOIN payment_method pm ON pm.id = m.payment_method_id
         LEFT JOIN exchange_rate r ON r.id = m.fx_rate_id
         WHERE a.owner_person_id = ?1 AND a.group_id IS NULL
           AND a.deleted_at IS NULL AND m.deleted_at IS NULL",
    )?;
    let stored = statement
        .query_map([person.as_str()], |row| {
            let rate = match (row.get(10)?, row.get(11)?, row.get(12)?) {
                (Some(base), Some(quote), Some(value)) => Some((base, quote, value)),
                _ => None,
            };
            Ok(StoredRow {
                id: row.get(0)?,
                kind: row.get(1)?,
                amount_minor: row.get(2)?,
                currency: row.get(3)?,
                occurred_at: row.get(4)?,
                expense_id: row.get(5)?,
                fee_minor: row.get(6)?,
                fee_currency: row.get(7)?,
                method: row.get(8)?,
                fx_rate_id: row.get(9)?,
                rate,
                created_at: row.get(13)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // Both sides of an exchange share the rate archived for it.
    let mut exchange_sides: BTreeMap<String, Vec<Money>> = BTreeMap::new();
    for row in stored
        .iter()
        .filter(|r| r.kind == CashMovementKind::Exchange.code())
    {
        if let Some(rate_id) = &row.fx_rate_id {
            exchange_sides
                .entry(rate_id.clone())
                .or_default()
                .push(Money::new(
                    row.amount_minor,
                    stored_currency(&row.currency)?,
                ));
        }
    }

    let mut entries: Vec<(i64, CashEntry)> = Vec::new();
    for row in stored {
        let kind = CashMovementKind::from_code(&row.kind)?;
        let amount = Money::new(row.amount_minor, stored_currency(&row.currency)?);
        let fee = match (row.fee_minor, &row.fee_currency) {
            (Some(minor), Some(code)) => Some(Money::new(minor, stored_currency(code)?)),
            _ => None,
        };
        let counterpart = match kind {
            CashMovementKind::Exchange => row
                .fx_rate_id
                .as_ref()
                .and_then(|id| exchange_sides.get(id))
                .and_then(|sides| sides.iter().find(|s| s.currency() != amount.currency()))
                .map(|other| Money::new(other.amount_minor().abs(), other.currency())),
            _ => match &row.rate {
                Some((base, quote, value)) => charged(amount, base, quote, value)?,
                None => None,
            },
        };
        entries.push((
            row.created_at,
            CashEntry {
                kind,
                amount,
                occurred_at: row.occurred_at,
                movement_id: Some(CashMovementId::new(row.id)),
                expense_id: row.expense_id.map(ExpenseId::new),
                title: None,
                method: row.method,
                counterpart,
                fee,
            },
        ));
    }

    let mut statement = conn.prepare(
        "SELECT e.id, e.title, e.occurred_at, e.currency, p.amount_minor, pm.name, p.created_at
         FROM expense_payment p
         JOIN expense e ON e.id = p.expense_id
         JOIN payment_method pm ON pm.id = p.payment_method_id
         LEFT JOIN expense_group g ON g.id = e.group_id
         WHERE p.person_id = ?1 AND pm.kind = ?2
           AND p.deleted_at IS NULL AND e.deleted_at IS NULL
           AND (e.group_id IS NULL OR g.deleted_at IS NULL)",
    )?;
    let payments = statement
        .query_map(
            params![person.as_str(), PaymentMethodKind::Cash.code()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    for (expense, title, occurred_at, currency, amount_minor, method, created_at) in payments {
        entries.push((
            created_at,
            CashEntry {
                kind: CashMovementKind::Expense,
                amount: Money::new(-amount_minor, stored_currency(&currency)?),
                occurred_at,
                movement_id: None,
                expense_id: Some(ExpenseId::new(expense)),
                title: Some(title),
                method: Some(method),
                counterpart: None,
                fee: None,
            },
        ));
    }

    // By when it happened, also across UTC offsets; the same time by entry.
    let mut keyed = entries
        .into_iter()
        .map(|(created, entry)| Ok((instant(&entry.occurred_at)?, created, entry)))
        .collect::<Result<Vec<_>, StorageError>>()?;
    keyed.sort_by(|(a_at, a_created, _), (b_at, b_created, _)| {
        b_at.cmp(a_at).then(b_created.cmp(a_created))
    });
    let entries: Vec<(i64, CashEntry)> = keyed
        .into_iter()
        .map(|(_, created, entry)| (created, entry))
        .collect();
    Ok(entries.into_iter().map(|(_, entry)| entry).collect())
}

/// What the card was charged for a withdrawal of `amount`, from the rate
/// archived with it.
fn charged(
    amount: Money,
    base: &str,
    quote: &str,
    value: &str,
) -> Result<Option<Money>, StorageError> {
    let base = stored_currency(base)?;
    let quote = stored_currency(quote)?;
    let value = Decimal::from_str(value)
        .map_err(|_| StorageError::InvalidInput("stored rate is not a number"))?;
    let rate = Rate::new(base, quote, value)
        .map_err(|_| StorageError::InvalidInput("stored rate is invalid"))?;
    let rate = if rate.base() == amount.currency() {
        rate
    } else {
        rate.inverse()
    };
    Ok(fx::convert(amount, &rate).ok())
}

fn stored_currency(code: &str) -> Result<Currency, StorageError> {
    Currency::from_code(code).map_err(|_| StorageError::InvalidInput("unknown stored currency"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use invuso_core::domain::{CategoryId, ExpenseSource, GroupId, Person};
    use invuso_core::split::SplitMode;

    use super::*;
    use crate::storage::{
        LAST_EXPENSE_GROUP, NewExchangeRate, NewExpensePayment, NewGroup, NewPaymentMethod,
        NewPerson, Profile,
    };

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn yen(amount: i64) -> Money {
        Money::new(amount, cur("JPY"))
    }

    fn eur(amount: i64) -> Money {
        Money::new(amount, cur("EUR"))
    }

    struct Setup {
        db: Db,
        me: Person,
        anna: Person,
        group: GroupId,
        cash: PaymentMethodId,
        anna_cash: PaymentMethodId,
        card: PaymentMethodId,
    }

    fn method(db: &Db, name: &str, kind: PaymentMethodKind, owner: &Person) -> PaymentMethodId {
        db.create_payment_method(NewPaymentMethod {
            name: name.into(),
            kind,
            owner_person_id: Some(owner.id.clone()),
            last4: None,
            color: "cerulean".into(),
            icon: "banknote".into(),
        })
        .unwrap()
        .id
    }

    fn setup() -> Setup {
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
                base_currency: cur("JPY"),
                start_date: None,
                end_date: None,
                target_language: None,
            })
            .unwrap()
            .id;
        db.add_group_member(&group, &anna.id).unwrap();
        let cash = method(&db, "Bargeld", PaymentMethodKind::Cash, &me);
        let anna_cash = method(&db, "Bargeld Anna", PaymentMethodKind::Cash, &anna);
        let card = method(&db, "Visa", PaymentMethodKind::CreditCard, &me);
        Setup {
            db,
            me,
            anna,
            group,
            cash,
            anna_cash,
            card,
        }
    }

    /// A yen expense in the group, paid by `payer` with `method`.
    fn pay(
        s: &Setup,
        payer: &Person,
        method: &PaymentMethodId,
        amount: i64,
        at: &str,
    ) -> ExpenseId {
        let rate = s.db.latest_rate(cur("JPY"), cur("JPY")).unwrap().unwrap();
        s.db.create_expense(
            NewExpense {
                group_id: Some(s.group.clone()),
                title: "Ramen".into(),
                category_id: None,
                occurred_at: at.into(),
                total: yen(amount),
                payments: vec![NewExpensePayment {
                    person_id: payer.id.clone(),
                    payment_method_id: Some(method.clone()),
                    amount_minor: amount,
                }],
                split: SplitMode::Equal(BTreeSet::from([s.me.id.clone(), s.anna.id.clone()])),
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

    fn withdrawal(s: &Setup, amount: Money, charged: Option<Money>, at: &str) -> NewWithdrawal {
        NewWithdrawal {
            person: s.me.id.clone(),
            amount,
            card: Some(s.card.clone()),
            charged,
            occurred_at: at.into(),
        }
    }

    const MORNING: &str = "2026-10-03T10:00:00+09:00";

    #[test]
    fn withdrawal_two_cash_payments_and_a_count_give_the_balance() {
        let s = setup();
        s.db.record_withdrawal(
            withdrawal(&s, yen(30_000), Some(eur(18_620)), MORNING),
            None,
        )
        .unwrap();
        pay(&s, &s.me, &s.cash, 1_280, "2026-10-03T12:00:00+09:00");
        pay(&s, &s.me, &s.cash, 4_500, "2026-10-03T19:00:00+09:00");
        // Paid by card or by someone else: not my cash.
        pay(&s, &s.me, &s.card, 9_999, "2026-10-03T20:00:00+09:00");
        pay(&s, &s.anna, &s.anna_cash, 800, "2026-10-03T20:30:00+09:00");
        assert_eq!(s.db.cash_balances(&s.me.id).unwrap(), [yen(24_220)]);

        let (_, correction) =
            s.db.record_cash_count(&s.me.id, yen(24_000), "2026-10-03T21:00:00+09:00")
                .unwrap()
                .unwrap();
        assert_eq!(correction, yen(-220));
        assert_eq!(s.db.cash_balances(&s.me.id).unwrap(), [yen(24_000)]);
        // Counting again finds nothing to correct.
        assert_eq!(
            s.db.record_cash_count(&s.me.id, yen(24_000), "2026-10-03T21:05:00+09:00")
                .unwrap(),
            None
        );

        // Anna has her own cash (user decision in AP-22).
        assert_eq!(s.db.cash_balances(&s.anna.id).unwrap(), [yen(-800)]);
    }

    #[test]
    fn count_compares_with_the_cash_at_its_time() {
        let s = setup();
        s.db.record_withdrawal(withdrawal(&s, yen(10_000), None, MORNING), None)
            .unwrap();
        pay(&s, &s.me, &s.cash, 2_000, "2026-10-03T19:00:00+09:00");
        // Counted at noon, before the evening payment.
        let (_, correction) =
            s.db.record_cash_count(&s.me.id, yen(9_500), "2026-10-03T12:00:00+09:00")
                .unwrap()
                .unwrap();
        assert_eq!(correction, yen(-500));
        assert_eq!(s.db.cash_balances(&s.me.id).unwrap(), [yen(7_500)]);
    }

    #[test]
    fn count_compares_by_instant_across_time_zones() {
        let s = setup();
        s.db.record_withdrawal(withdrawal(&s, yen(30_000), None, MORNING), None)
            .unwrap();
        // Paid in Tokyo at 12:30, counted in Berlin at 08:13 (= 15:13 Tokyo).
        pay(&s, &s.me, &s.cash, 3_200, "2026-10-03T12:30:00+09:00");
        let berlin = "2026-10-03T08:13:00+02:00";
        assert_eq!(
            s.db.cash_balance_at(&s.me.id, cur("JPY"), berlin).unwrap(),
            yen(26_800)
        );
        let (_, correction) =
            s.db.record_cash_count(&s.me.id, yen(26_500), berlin)
                .unwrap()
                .unwrap();
        assert_eq!(correction, yen(-300));
        assert_eq!(s.db.cash_balances(&s.me.id).unwrap(), [yen(26_500)]);
        // Listed by instant: the count is the latest.
        let entries = s.db.cash_entries(&s.me.id).unwrap();
        assert_eq!(entries[0].kind, CashMovementKind::Correction);
    }

    #[test]
    fn entries_list_latest_first_with_what_belongs_to_them() {
        let s = setup();
        s.db.record_withdrawal(
            withdrawal(&s, yen(30_000), Some(eur(18_620)), MORNING),
            None,
        )
        .unwrap();
        let ramen = pay(&s, &s.me, &s.cash, 1_280, "2026-10-03T12:00:00+09:00");
        let entries = s.db.cash_entries(&s.me.id).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, CashMovementKind::Expense);
        assert_eq!(entries[0].amount, yen(-1_280));
        assert_eq!(entries[0].movement_id, None);
        assert_eq!(entries[0].expense_id, Some(ramen));
        assert_eq!(entries[0].title.as_deref(), Some("Ramen"));
        assert_eq!(entries[0].method.as_deref(), Some("Bargeld"));
        assert_eq!(entries[1].kind, CashMovementKind::Withdrawal);
        assert_eq!(entries[1].method.as_deref(), Some("Visa"));
        assert_eq!(entries[1].counterpart, Some(eur(18_620)));
    }

    #[test]
    fn editing_or_deleting_a_cash_expense_changes_the_cash_at_once() {
        let s = setup();
        let ramen = pay(&s, &s.me, &s.cash, 1_280, "2026-10-03T12:00:00+09:00");
        assert_eq!(s.db.cash_balances(&s.me.id).unwrap(), [yen(-1_280)]);
        s.db.delete_expense(&ramen).unwrap();
        assert_eq!(s.db.cash_balances(&s.me.id).unwrap(), []);
        s.db.restore_expense(&ramen).unwrap();
        s.db.delete_group(&s.group).unwrap();
        assert_eq!(s.db.cash_balances(&s.me.id).unwrap(), []);
    }

    #[test]
    fn exchange_moves_cash_between_currencies_at_the_actual_rate() {
        let s = setup();
        let exchange = |given, received| NewExchange {
            person: s.me.id.clone(),
            given,
            received,
            occurred_at: "2026-10-02T09:00:00+02:00".into(),
        };
        let received =
            s.db.record_exchange(exchange(eur(20_000), yen(31_000)))
                .unwrap();
        assert_eq!(
            s.db.cash_balances(&s.me.id).unwrap(),
            [eur(-20_000), yen(31_000)]
        );
        let entries = s.db.cash_entries(&s.me.id).unwrap();
        let yen_side = entries
            .iter()
            .find(|e| e.movement_id.as_ref() == Some(&received))
            .unwrap();
        assert_eq!(yen_side.amount, yen(31_000));
        assert_eq!(yen_side.counterpart, Some(eur(20_000)));
        let eur_side = entries.iter().find(|e| e.amount == eur(-20_000)).unwrap();
        assert_eq!(eur_side.counterpart, Some(yen(31_000)));

        // Deleting one side takes the other along; undo brings both back.
        s.db.delete_cash_movement(&received).unwrap();
        assert_eq!(s.db.cash_balances(&s.me.id).unwrap(), []);
        s.db.restore_cash_movement(&received).unwrap();
        assert_eq!(
            s.db.cash_balances(&s.me.id).unwrap(),
            [eur(-20_000), yen(31_000)]
        );
        assert!(matches!(
            s.db.record_exchange(exchange(eur(100), eur(100))),
            Err(StorageError::Cash(CashError::SameCurrency))
        ));
    }

    #[test]
    fn withdrawal_fee_is_an_expense_that_goes_and_comes_back_with_it() {
        let s = setup();
        let rate = s.db.latest_rate(cur("EUR"), cur("EUR")).unwrap().unwrap();
        let fee = WithdrawalFee {
            expense: NewExpense {
                group_id: None,
                title: "Abhebegebühr".into(),
                category_id: Some(CategoryId::new("default-other")),
                occurred_at: MORNING.into(),
                total: eur(450),
                payments: vec![NewExpensePayment {
                    person_id: s.me.id.clone(),
                    payment_method_id: Some(s.card.clone()),
                    amount_minor: 450,
                }],
                split: SplitMode::Equal(BTreeSet::from([s.me.id.clone()])),
                receipt_id: None,
                line_items: Vec::new(),
                source: ExpenseSource::Manual,
                note: None,
                location: None,
                coordinates: None,
                own_rate: None,
            },
            rate,
        };
        s.db.set_setting(LAST_EXPENSE_GROUP, s.group.as_str())
            .unwrap();
        let id =
            s.db.record_withdrawal(withdrawal(&s, yen(30_000), None, MORNING), Some(fee))
                .unwrap();
        let entry = s.db.cash_entries(&s.me.id).unwrap().remove(0);
        assert_eq!(entry.fee, Some(eur(450)));
        assert_eq!(entry.counterpart, None);
        let fee_expense = entry.expense_id.unwrap();
        assert_eq!(s.db.expense(&fee_expense).unwrap().unwrap().total, eur(450));
        // The fee does not change what the expense form preselects.
        assert_eq!(
            s.db.setting(LAST_EXPENSE_GROUP).unwrap().as_deref(),
            Some(s.group.as_str())
        );

        s.db.delete_cash_movement(&id).unwrap();
        assert_eq!(s.db.expense(&fee_expense).unwrap(), None);
        assert_eq!(s.db.cash_balances(&s.me.id).unwrap(), []);
        s.db.restore_cash_movement(&id).unwrap();
        assert!(s.db.expense(&fee_expense).unwrap().is_some());

        // A fee expense deleted on its own stays deleted on undo.
        s.db.delete_expense(&fee_expense).unwrap();
        // Undo matches by deletion time, which has millisecond resolution.
        std::thread::sleep(std::time::Duration::from_millis(5));
        s.db.delete_cash_movement(&id).unwrap();
        s.db.restore_cash_movement(&id).unwrap();
        assert_eq!(s.db.expense(&fee_expense).unwrap(), None);
        assert!(matches!(
            s.db.restore_cash_movement(&id),
            Err(StorageError::NotFound)
        ));
    }

    #[test]
    fn actual_rates_never_convert_other_expenses() {
        let s = setup();
        s.db.archive_rates(
            "frankfurter",
            1_000,
            &[NewExchangeRate {
                rate: Rate::new(cur("EUR"), cur("JPY"), Decimal::from(160)).unwrap(),
                rate_date: "2026-10-03".into(),
            }],
        )
        .unwrap();
        s.db.record_withdrawal(
            withdrawal(&s, yen(30_000), Some(eur(20_000)), MORNING),
            None,
        )
        .unwrap();
        let quote =
            s.db.rate_on(cur("EUR"), cur("JPY"), "2026-10-03")
                .unwrap()
                .unwrap();
        assert_eq!(quote.rate.value(), Decimal::from(160));
        assert_eq!(s.db.last_rate_fetch().unwrap(), Some(1_000));
    }

    #[test]
    fn rejects_invalid_movements() {
        let s = setup();
        assert!(matches!(
            s.db.record_withdrawal(withdrawal(&s, yen(0), None, MORNING), None),
            Err(StorageError::Cash(CashError::NonPositiveAmount))
        ));
        assert!(matches!(
            s.db.record_withdrawal(withdrawal(&s, yen(100), None, "morgen"), None),
            Err(StorageError::Expense(_))
        ));
        assert!(matches!(
            s.db.record_cash_count(&s.me.id, yen(-1), MORNING),
            Err(StorageError::Cash(CashError::NonPositiveAmount))
        ));
        assert!(matches!(
            s.db.record_cash_count(&PersonId::new("nobody"), yen(1), MORNING),
            Err(StorageError::NotFound)
        ));
    }
}
