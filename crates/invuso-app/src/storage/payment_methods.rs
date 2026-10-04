use invuso_core::domain::{
    PaymentMethod, PaymentMethodId, PaymentMethodKind, PersonId, validate_last4,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::db::{new_id, now_ms};
use super::{Db, StorageError, people};

/// Input for creating a payment method (PAY-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPaymentMethod {
    pub name: String,
    pub kind: PaymentMethodKind,
    pub owner_person_id: Option<PersonId>,
    pub last4: Option<String>,
    pub color: String,
    pub icon: String,
}

const COLUMNS: &str =
    "pm.id, pm.name, pm.kind, pm.owner_person_id, pm.last4, pm.color, pm.icon, pm.archived";

/// Methods of deleted people stay in the table (restoring the person brings
/// them back) but are left out of every list.
const VISIBLE: &str = "pm.deleted_at IS NULL
    AND (pm.owner_person_id IS NULL OR EXISTS (
        SELECT 1 FROM person p WHERE p.id = pm.owner_person_id AND p.deleted_at IS NULL))";

impl Db {
    pub fn create_payment_method(
        &self,
        new: NewPaymentMethod,
    ) -> Result<PaymentMethod, StorageError> {
        let method = PaymentMethod {
            id: PaymentMethodId::new(new_id()),
            name: people::valid_name(&new.name)?,
            kind: new.kind,
            owner_person_id: new.owner_person_id,
            last4: validate_last4(new.kind, new.last4.as_deref())?,
            color: new.color,
            icon: new.icon,
            archived: false,
        };
        self.with(|conn| {
            check_owner(conn, method.owner_person_id.as_ref())?;
            conn.execute(
                "INSERT INTO payment_method
                     (id, name, kind, owner_person_id, last4, color, icon, archived,
                      created_at, updated_at, origin_device_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8, ?9)",
                params![
                    method.id.as_str(),
                    method.name,
                    method.kind.code(),
                    method.owner_person_id.as_ref().map(PersonId::as_str),
                    method.last4,
                    method.color,
                    method.icon,
                    now_ms(),
                    self.device_id()
                ],
            )?;
            Ok(())
        })?;
        Ok(method)
    }

    pub fn payment_method(
        &self,
        id: &PaymentMethodId,
    ) -> Result<Option<PaymentMethod>, StorageError> {
        self.with(|conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {COLUMNS} FROM payment_method pm WHERE pm.id = ?1 AND pm.deleted_at IS NULL"
                    ),
                    [id.as_str()],
                    method_from_row,
                )
                .optional()?)
        })
    }

    /// All methods including archived ones, for the management page: grouped
    /// by owner ("Ich" first, then by name), active before archived.
    pub fn payment_methods(&self) -> Result<Vec<PaymentMethod>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM payment_method pm
                 LEFT JOIN person o ON o.id = pm.owner_person_id
                 WHERE {VISIBLE}
                 ORDER BY o.id IS NULL, o.is_me DESC, o.name COLLATE NOCASE, o.id,
                          pm.archived, pm.name COLLATE NOCASE, pm.id"
            ))?;
            let methods = statement
                .query_map([], method_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(methods)
        })
    }

    /// Active (not archived) methods of one person, for choosing who paid
    /// with what (EXP-03).
    pub fn payment_methods_of(&self, owner: &PersonId) -> Result<Vec<PaymentMethod>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM payment_method pm
                 WHERE {VISIBLE} AND pm.owner_person_id = ?1 AND pm.archived = 0
                 ORDER BY pm.name COLLATE NOCASE, pm.id"
            ))?;
            let methods = statement
                .query_map([owner.as_str()], method_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(methods)
        })
    }

    /// Saves name, kind, owner, last digits, color, icon and archived state.
    pub fn update_payment_method(&self, method: &PaymentMethod) -> Result<(), StorageError> {
        let name = people::valid_name(&method.name)?;
        let last4 = validate_last4(method.kind, method.last4.as_deref())?;
        let changed = self.with(|conn| {
            check_owner(conn, method.owner_person_id.as_ref())?;
            Ok(conn.execute(
                "UPDATE payment_method
                 SET name = ?2, kind = ?3, owner_person_id = ?4, last4 = ?5, color = ?6,
                     icon = ?7, archived = ?8, updated_at = ?9
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![
                    method.id.as_str(),
                    name,
                    method.kind.code(),
                    method.owner_person_id.as_ref().map(PersonId::as_str),
                    last4,
                    method.color,
                    method.icon,
                    method.archived,
                    now_ms()
                ],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Archived methods disappear from pickers but stay on old payments and
    /// on the management page (PAY-01).
    pub fn set_payment_method_archived(
        &self,
        id: &PaymentMethodId,
        archived: bool,
    ) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            Ok(conn.execute(
                "UPDATE payment_method SET archived = ?2, updated_at = ?3
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id.as_str(), archived, now_ms()],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Soft delete: payments keep pointing at the row (idee.md 4).
    pub fn delete_payment_method(&self, id: &PaymentMethodId) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            let now = now_ms();
            Ok(conn.execute(
                "UPDATE payment_method SET deleted_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id.as_str(), now],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Undoes [`Db::delete_payment_method`] (undo toast, UI-11).
    pub fn restore_payment_method(&self, id: &PaymentMethodId) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            Ok(conn.execute(
                "UPDATE payment_method SET deleted_at = NULL, updated_at = ?2
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

/// The foreign key accepts soft-deleted people; a new owner must be alive.
fn check_owner(conn: &Connection, owner: Option<&PersonId>) -> Result<(), StorageError> {
    let Some(owner) = owner else {
        return Ok(());
    };
    let exists = conn
        .query_row(
            "SELECT 1 FROM person WHERE id = ?1 AND deleted_at IS NULL",
            [owner.as_str()],
            |_| Ok(()),
        )
        .optional()?;
    exists.ok_or(StorageError::InvalidInput("owner does not exist"))
}

fn method_from_row(row: &Row<'_>) -> rusqlite::Result<PaymentMethod> {
    let kind: String = row.get(2)?;
    Ok(PaymentMethod {
        id: PaymentMethodId::new(row.get::<_, String>(0)?),
        name: row.get(1)?,
        kind: PaymentMethodKind::from_code(&kind).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into())
        })?,
        owner_person_id: row.get::<_, Option<String>>(3)?.map(PersonId::new),
        last4: row.get(4)?,
        color: row.get(5)?,
        icon: row.get(6)?,
        archived: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::{PaymentMethodError, Person};

    use super::*;
    use crate::storage::NewPerson;

    fn person(db: &Db, name: &str, is_me: bool) -> Person {
        db.create_person(NewPerson {
            name: name.into(),
            color: "cerulean".into(),
            is_me,
            note: None,
        })
        .unwrap()
    }

    fn new(name: &str, kind: PaymentMethodKind, owner: &Person) -> NewPaymentMethod {
        NewPaymentMethod {
            name: name.into(),
            kind,
            owner_person_id: Some(owner.id.clone()),
            last4: None,
            color: "cerulean".into(),
            icon: "credit-card".into(),
        }
    }

    fn names(methods: &[PaymentMethod]) -> Vec<&str> {
        methods.iter().map(|m| m.name.as_str()).collect()
    }

    #[test]
    fn create_and_read_back() {
        let db = Db::open_in_memory().unwrap();
        let me = person(&db, "Me", true);
        let visa = db
            .create_payment_method(NewPaymentMethod {
                last4: Some("4242".into()),
                ..new("  Visa DKB ", PaymentMethodKind::CreditCard, &me)
            })
            .unwrap();
        assert_eq!(visa.name, "Visa DKB");
        assert_eq!(visa.last4.as_deref(), Some("4242"));
        assert!(!visa.archived);
        assert_eq!(db.payment_method(&visa.id).unwrap(), Some(visa));
    }

    #[test]
    fn list_grouped_by_owner_me_first_archived_last() {
        let db = Db::open_in_memory().unwrap();
        let ben = person(&db, "Ben", false);
        let me = person(&db, "Me", true);
        db.create_payment_method(new("PayPal", PaymentMethodKind::PayPal, &ben))
            .unwrap();
        let old = db
            .create_payment_method(new("Alte Karte", PaymentMethodKind::DebitCard, &me))
            .unwrap();
        db.create_payment_method(new("Visa", PaymentMethodKind::CreditCard, &me))
            .unwrap();
        db.create_payment_method(new("Bargeld", PaymentMethodKind::Cash, &me))
            .unwrap();
        db.set_payment_method_archived(&old.id, true).unwrap();

        assert_eq!(
            names(&db.payment_methods().unwrap()),
            ["Bargeld", "Visa", "Alte Karte", "PayPal"]
        );
        assert_eq!(
            names(&db.payment_methods_of(&me.id).unwrap()),
            ["Bargeld", "Visa"]
        );
        assert_eq!(names(&db.payment_methods_of(&ben.id).unwrap()), ["PayPal"]);
    }

    #[test]
    fn archive_and_unarchive() {
        let db = Db::open_in_memory().unwrap();
        let me = person(&db, "Me", true);
        let visa = db
            .create_payment_method(new("Visa", PaymentMethodKind::CreditCard, &me))
            .unwrap();
        db.set_payment_method_archived(&visa.id, true).unwrap();
        assert!(db.payment_method(&visa.id).unwrap().unwrap().archived);
        assert!(db.payment_methods_of(&me.id).unwrap().is_empty());

        db.set_payment_method_archived(&visa.id, false).unwrap();
        assert!(!db.payment_method(&visa.id).unwrap().unwrap().archived);
        assert_eq!(db.payment_methods_of(&me.id).unwrap().len(), 1);
    }

    #[test]
    fn update_changes_every_field() {
        let db = Db::open_in_memory().unwrap();
        let me = person(&db, "Me", true);
        let ben = person(&db, "Ben", false);
        let method = db
            .create_payment_method(new("Karte", PaymentMethodKind::CreditCard, &me))
            .unwrap();
        let updated = PaymentMethod {
            name: "Suica".into(),
            kind: PaymentMethodKind::IcCard,
            owner_person_id: Some(ben.id.clone()),
            last4: None,
            color: "pale-oak".into(),
            icon: "train-front".into(),
            archived: true,
            ..method
        };
        db.update_payment_method(&updated).unwrap();
        assert_eq!(db.payment_method(&updated.id).unwrap(), Some(updated));
    }

    #[test]
    fn rejects_invalid_last4() {
        let db = Db::open_in_memory().unwrap();
        let me = person(&db, "Me", true);
        for bad in ["123", "12345", "12a4"] {
            assert!(matches!(
                db.create_payment_method(NewPaymentMethod {
                    last4: Some(bad.into()),
                    ..new("Visa", PaymentMethodKind::CreditCard, &me)
                }),
                Err(StorageError::PaymentMethod(
                    PaymentMethodError::InvalidLast4
                ))
            ));
        }
        assert!(matches!(
            db.create_payment_method(NewPaymentMethod {
                last4: Some("1234".into()),
                ..new("PayPal", PaymentMethodKind::PayPal, &me)
            }),
            Err(StorageError::PaymentMethod(
                PaymentMethodError::Last4NotAllowed
            ))
        ));
        let visa = db
            .create_payment_method(new("Visa", PaymentMethodKind::CreditCard, &me))
            .unwrap();
        assert!(matches!(
            db.update_payment_method(&PaymentMethod {
                last4: Some("12345".into()),
                ..visa
            }),
            Err(StorageError::PaymentMethod(
                PaymentMethodError::InvalidLast4
            ))
        ));
        assert!(db.payment_methods().unwrap()[0].last4.is_none());
    }

    #[test]
    fn database_rejects_invalid_last4_too() {
        let db = Db::open_in_memory().unwrap();
        let me = person(&db, "Me", true);
        let visa = db
            .create_payment_method(new("Visa", PaymentMethodKind::CreditCard, &me))
            .unwrap();
        for bad in ["123", "12345", "12a4"] {
            let result = db.with(|conn| {
                Ok(conn.execute(
                    "UPDATE payment_method SET last4 = ?2 WHERE id = ?1",
                    params![visa.id.as_str(), bad],
                )?)
            });
            assert!(matches!(result, Err(StorageError::Sqlite(_))), "{bad}");
        }
    }

    #[test]
    fn rejects_empty_name_and_missing_owner() {
        let db = Db::open_in_memory().unwrap();
        let me = person(&db, "Me", true);
        let ben = person(&db, "Ben", false);
        assert!(matches!(
            db.create_payment_method(new("  ", PaymentMethodKind::Cash, &me)),
            Err(StorageError::InvalidInput(_))
        ));
        db.delete_person(&ben.id).unwrap();
        assert!(matches!(
            db.create_payment_method(new("PayPal", PaymentMethodKind::PayPal, &ben)),
            Err(StorageError::InvalidInput(_))
        ));
    }

    #[test]
    fn soft_delete_and_restore() {
        let db = Db::open_in_memory().unwrap();
        let me = person(&db, "Me", true);
        let visa = db
            .create_payment_method(new("Visa", PaymentMethodKind::CreditCard, &me))
            .unwrap();
        assert!(matches!(
            db.restore_payment_method(&visa.id),
            Err(StorageError::NotFound)
        ));

        db.delete_payment_method(&visa.id).unwrap();
        assert_eq!(db.payment_method(&visa.id).unwrap(), None);
        assert!(db.payment_methods().unwrap().is_empty());
        assert!(matches!(
            db.delete_payment_method(&visa.id),
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            db.update_payment_method(&visa),
            Err(StorageError::NotFound)
        ));
        let row_stays: i64 = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT count(*) FROM payment_method WHERE id = ?1",
                    [visa.id.as_str()],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(row_stays, 1);

        db.restore_payment_method(&visa.id).unwrap();
        assert_eq!(db.payment_method(&visa.id).unwrap(), Some(visa));
    }

    #[test]
    fn methods_follow_their_deleted_owner() {
        let db = Db::open_in_memory().unwrap();
        let ben = person(&db, "Ben", false);
        db.create_payment_method(new("PayPal", PaymentMethodKind::PayPal, &ben))
            .unwrap();
        db.delete_person(&ben.id).unwrap();
        assert!(db.payment_methods().unwrap().is_empty());
        assert!(db.payment_methods_of(&ben.id).unwrap().is_empty());

        db.restore_person(&ben.id).unwrap();
        assert_eq!(names(&db.payment_methods().unwrap()), ["PayPal"]);
    }

    #[test]
    fn records_origin_device() {
        let db = Db::open_in_memory().unwrap();
        let me = person(&db, "Me", true);
        let visa = db
            .create_payment_method(new("Visa", PaymentMethodKind::CreditCard, &me))
            .unwrap();
        let origin: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT origin_device_id FROM payment_method WHERE id = ?1",
                    [visa.id.as_str()],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(origin, db.device_id());
    }
}
