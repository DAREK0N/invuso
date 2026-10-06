use invuso_core::domain::{
    Currency, GroupId, Money, PaymentMethodId, PersonId, Settlement, SettlementId,
    validate_settlement,
};
use rusqlite::{OptionalExtension, Row, params};

use super::db::{new_id, now_ms};
use super::groups::check_group;
use super::{Db, StorageError};

/// Input for recording a settlement (SPL-06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSettlement {
    pub group_id: GroupId,
    pub from: PersonId,
    pub to: PersonId,
    /// In the group's base currency; paying in another currency is SPL-10.
    pub amount: Money,
    pub payment_method_id: Option<PaymentMethodId>,
    pub occurred_at: String,
    pub note: Option<String>,
}

const COLUMNS: &str = "id, group_id, from_person_id, to_person_id, amount_minor, currency, \
                       payment_method_id, occurred_at, note";

impl Db {
    /// Saves a settlement between two people of a live group.
    pub fn create_settlement(&self, new: NewSettlement) -> Result<Settlement, StorageError> {
        validate_settlement(&new.from, &new.to, new.amount, &new.occurred_at)?;
        let note = new
            .note
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string);
        let settlement = Settlement {
            id: SettlementId::new(new_id()),
            group_id: new.group_id,
            from: new.from,
            to: new.to,
            amount: new.amount,
            payment_method_id: new.payment_method_id,
            occurred_at: new.occurred_at,
            note,
        };
        self.with(|conn| {
            check_group(conn, &settlement.group_id)?;
            let base: String = conn.query_row(
                "SELECT base_currency FROM expense_group WHERE id = ?1",
                [settlement.group_id.as_str()],
                |row| row.get(0),
            )?;
            if base != settlement.amount.currency().code() {
                return Err(StorageError::InvalidInput(
                    "a settlement must be in the group's base currency",
                ));
            }
            let now = now_ms();
            conn.execute(
                "INSERT INTO settlement
                     (id, group_id, from_person_id, to_person_id, amount_minor, currency,
                      payment_method_id, occurred_at, note, created_at, updated_at,
                      origin_device_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?11)",
                params![
                    settlement.id.as_str(),
                    settlement.group_id.as_str(),
                    settlement.from.as_str(),
                    settlement.to.as_str(),
                    settlement.amount.amount_minor(),
                    settlement.amount.currency().code(),
                    settlement
                        .payment_method_id
                        .as_ref()
                        .map(PaymentMethodId::as_str),
                    settlement.occurred_at,
                    settlement.note,
                    now,
                    self.device_id()
                ],
            )?;
            Ok(())
        })?;
        Ok(settlement)
    }

    /// The group's settlements, latest first.
    pub fn group_settlements(&self, group: &GroupId) -> Result<Vec<Settlement>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM settlement
                 WHERE group_id = ?1 AND deleted_at IS NULL
                 ORDER BY occurred_at DESC, created_at DESC, id"
            ))?;
            let settlements = statement
                .query_map([group.as_str()], settlement_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(settlements)
        })
    }

    pub fn settlement(&self, id: &SettlementId) -> Result<Option<Settlement>, StorageError> {
        self.with(|conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {COLUMNS} FROM settlement WHERE id = ?1 AND deleted_at IS NULL"
                    ),
                    [id.as_str()],
                    settlement_from_row,
                )
                .optional()?)
        })
    }

    /// Soft delete (idee.md 4), so the undo toast can bring it back.
    pub fn delete_settlement(&self, id: &SettlementId) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            Ok(conn.execute(
                "UPDATE settlement SET deleted_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id.as_str(), now_ms()],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Undoes [`Db::delete_settlement`] (undo toast, UI-11).
    pub fn restore_settlement(&self, id: &SettlementId) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            Ok(conn.execute(
                "UPDATE settlement SET deleted_at = NULL, updated_at = ?2
                 WHERE id = ?1 AND deleted_at IS NOT NULL",
                params![id.as_str(), now_ms()],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }
}

fn settlement_from_row(row: &Row<'_>) -> rusqlite::Result<Settlement> {
    let currency: String = row.get(5)?;
    let currency = Currency::from_code(&currency).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, e.into())
    })?;
    Ok(Settlement {
        id: SettlementId::new(row.get::<_, String>(0)?),
        group_id: GroupId::new(row.get::<_, String>(1)?),
        from: PersonId::new(row.get::<_, String>(2)?),
        to: PersonId::new(row.get::<_, String>(3)?),
        amount: Money::new(row.get(4)?, currency),
        payment_method_id: row.get::<_, Option<String>>(6)?.map(PaymentMethodId::new),
        occurred_at: row.get(7)?,
        note: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::{Group, SettlementError};

    use super::*;
    use crate::storage::{NewGroup, NewPerson, Profile};

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    struct Setup {
        db: Db,
        me: PersonId,
        anna: PersonId,
        trip: Group,
    }

    fn setup() -> Setup {
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
        let trip = db
            .create_group(NewGroup {
                name: "Japan".into(),
                icon: "plane".into(),
                color: "cerulean".into(),
                base_currency: cur("EUR"),
                start_date: None,
                end_date: None,
                target_language: None,
            })
            .unwrap();
        db.add_group_member(&trip.id, &anna).unwrap();
        Setup { db, me, anna, trip }
    }

    fn new(s: &Setup, amount: i64, at: &str) -> NewSettlement {
        NewSettlement {
            group_id: s.trip.id.clone(),
            from: s.me.clone(),
            to: s.anna.clone(),
            amount: Money::new(amount, cur("EUR")),
            payment_method_id: None,
            occurred_at: at.into(),
            note: Some("  ".into()),
        }
    }

    #[test]
    fn saves_lists_latest_first_and_reads_back() {
        let s = setup();
        let first =
            s.db.create_settlement(new(&s, 1_000, "2026-10-05T12:00:00+02:00"))
                .unwrap();
        let second =
            s.db.create_settlement(new(&s, 2_340, "2026-10-06T09:00:00+02:00"))
                .unwrap();
        assert_eq!(first.note, None);
        assert_eq!(
            s.db.group_settlements(&s.trip.id).unwrap(),
            [second.clone(), first]
        );
        assert_eq!(s.db.settlement(&second.id).unwrap(), Some(second));
    }

    #[test]
    fn delete_hides_and_restore_brings_back() {
        let s = setup();
        let saved =
            s.db.create_settlement(new(&s, 500, "2026-10-06T09:00:00+02:00"))
                .unwrap();
        s.db.delete_settlement(&saved.id).unwrap();
        assert_eq!(s.db.group_settlements(&s.trip.id).unwrap(), []);
        assert!(matches!(
            s.db.delete_settlement(&saved.id),
            Err(StorageError::NotFound)
        ));
        s.db.restore_settlement(&saved.id).unwrap();
        assert_eq!(s.db.group_settlements(&s.trip.id).unwrap(), [saved]);
    }

    #[test]
    fn rejects_invalid_settlements() {
        let s = setup();
        let mut same = new(&s, 500, "2026-10-06T09:00:00+02:00");
        same.to = s.me.clone();
        assert!(matches!(
            s.db.create_settlement(same),
            Err(StorageError::Settlement(SettlementError::SamePerson))
        ));
        let mut yen = new(&s, 500, "2026-10-06T09:00:00+02:00");
        yen.amount = Money::new(500, cur("JPY"));
        assert!(matches!(
            s.db.create_settlement(yen),
            Err(StorageError::InvalidInput(_))
        ));
        s.db.delete_group(&s.trip.id).unwrap();
        assert!(matches!(
            s.db.create_settlement(new(&s, 500, "2026-10-06T09:00:00+02:00")),
            Err(StorageError::NotFound)
        ));
    }
}
