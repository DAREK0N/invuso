use invuso_core::domain::{Currency, Group, GroupId, validate_period};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::db::{new_id, now_ms};
use super::settings::ACTIVE_GROUP;
use super::{Db, StorageError, group_members, people};

/// Input for creating a group (GRP-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGroup {
    pub name: String,
    pub icon: String,
    pub color: String,
    pub base_currency: Currency,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub target_language: Option<String>,
}

const COLUMNS: &str = "id, name, icon, color, base_currency, start_date, end_date, target_language";

impl Db {
    /// Creates the group with "Ich" as its first member (AP-08), in one
    /// transaction so a group never exists without them.
    pub fn create_group(&self, new: NewGroup) -> Result<Group, StorageError> {
        let (start_date, end_date) =
            validate_period(new.start_date.as_deref(), new.end_date.as_deref())?;
        let group = Group {
            id: GroupId::new(new_id()),
            name: people::valid_name(&new.name)?,
            icon: new.icon,
            color: new.color,
            base_currency: new.base_currency,
            start_date,
            end_date,
            target_language: language(new.target_language.as_deref()),
        };
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "INSERT INTO expense_group
                     (id, name, icon, color, base_currency, start_date, end_date,
                      target_language, archived, created_at, updated_at, origin_device_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?9, ?10)",
                params![
                    group.id.as_str(),
                    group.name,
                    group.icon,
                    group.color,
                    group.base_currency.code(),
                    group.start_date,
                    group.end_date,
                    group.target_language,
                    now_ms(),
                    self.device_id()
                ],
            )?;
            if let Some(me) = people::me(&tx)? {
                group_members::insert(&tx, self.device_id(), &group.id, &me.id)?;
            }
            tx.commit()?;
            Ok(())
        })?;
        Ok(group)
    }

    pub fn group(&self, id: &GroupId) -> Result<Option<Group>, StorageError> {
        self.with(|conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {COLUMNS} FROM expense_group WHERE id = ?1 AND deleted_at IS NULL"
                    ),
                    [id.as_str()],
                    group_from_row,
                )
                .optional()?)
        })
    }

    /// All groups, newest first (ids are UUIDv7 and sort by creation time).
    pub fn groups(&self) -> Result<Vec<Group>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM expense_group WHERE deleted_at IS NULL ORDER BY id DESC"
            ))?;
            let groups = statement
                .query_map([], group_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(groups)
        })
    }

    /// The group marked as active (GRP-05), if it still exists; a deleted
    /// one becomes active again when it is restored.
    pub fn active_group(&self) -> Result<Option<Group>, StorageError> {
        match self.setting(ACTIVE_GROUP)? {
            Some(id) if !id.is_empty() => self.group(&GroupId::new(id)),
            _ => Ok(None),
        }
    }

    /// Marks the group as active, the default target of new expenses
    /// (GRP-05); `None` removes the mark.
    pub fn set_active_group(&self, id: Option<&GroupId>) -> Result<(), StorageError> {
        self.set_setting(ACTIVE_GROUP, id.map_or("", GroupId::as_str))
    }

    /// Saves name, icon, color, base currency, period and target language.
    pub fn update_group(&self, group: &Group) -> Result<(), StorageError> {
        let name = people::valid_name(&group.name)?;
        let (start_date, end_date) =
            validate_period(group.start_date.as_deref(), group.end_date.as_deref())?;
        let changed = self.with(|conn| {
            Ok(conn.execute(
                "UPDATE expense_group
                 SET name = ?2, icon = ?3, color = ?4, base_currency = ?5, start_date = ?6,
                     end_date = ?7, target_language = ?8, updated_at = ?9
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![
                    group.id.as_str(),
                    name,
                    group.icon,
                    group.color,
                    group.base_currency.code(),
                    start_date,
                    end_date,
                    language(group.target_language.as_deref()),
                    now_ms()
                ],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Soft delete: members and (later) expenses stay attached to the row,
    /// so restoring brings the whole group back (idee.md 4).
    pub fn delete_group(&self, id: &GroupId) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            let now = now_ms();
            Ok(conn.execute(
                "UPDATE expense_group SET deleted_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id.as_str(), now],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Undoes [`Db::delete_group`] (undo toast, UI-11).
    pub fn restore_group(&self, id: &GroupId) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            Ok(conn.execute(
                "UPDATE expense_group SET deleted_at = NULL, updated_at = ?2
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

/// The foreign key accepts soft-deleted groups; changes need a live one.
pub(super) fn check_group(conn: &Connection, id: &GroupId) -> Result<(), StorageError> {
    conn.query_row(
        "SELECT 1 FROM expense_group WHERE id = ?1 AND deleted_at IS NULL",
        [id.as_str()],
        |_| Ok(()),
    )
    .optional()?
    .ok_or(StorageError::NotFound)
}

fn group_from_row(row: &Row<'_>) -> rusqlite::Result<Group> {
    let currency: String = row.get(4)?;
    Ok(Group {
        id: GroupId::new(row.get::<_, String>(0)?),
        name: row.get(1)?,
        icon: row.get(2)?,
        color: row.get(3)?,
        base_currency: Currency::from_code(&currency).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, e.into())
        })?,
        start_date: row.get(5)?,
        end_date: row.get(6)?,
        target_language: row.get(7)?,
    })
}

/// A blank language means "follow the global setting".
fn language(code: Option<&str>) -> Option<String> {
    code.map(str::trim)
        .filter(|code| !code.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::GroupError;

    use super::*;
    use crate::storage::NewPerson;

    fn eur() -> Currency {
        Currency::from_code("EUR").unwrap()
    }

    fn new(name: &str) -> NewGroup {
        NewGroup {
            name: name.into(),
            icon: "plane".into(),
            color: "cerulean".into(),
            base_currency: eur(),
            start_date: None,
            end_date: None,
            target_language: None,
        }
    }

    fn me(db: &Db) {
        db.create_person(NewPerson {
            name: "Me".into(),
            color: "cerulean".into(),
            is_me: true,
            note: None,
        })
        .unwrap();
    }

    #[test]
    fn active_group_follows_deletion_and_can_be_cleared() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.active_group().unwrap(), None);
        let trip = db.create_group(new("Japan")).unwrap();
        db.set_active_group(Some(&trip.id)).unwrap();
        assert_eq!(db.active_group().unwrap(), Some(trip.clone()));

        db.delete_group(&trip.id).unwrap();
        assert_eq!(db.active_group().unwrap(), None);
        db.restore_group(&trip.id).unwrap();
        assert_eq!(db.active_group().unwrap(), Some(trip));

        db.set_active_group(None).unwrap();
        assert_eq!(db.active_group().unwrap(), None);
    }

    #[test]
    fn create_and_read_back() {
        let db = Db::open_in_memory().unwrap();
        let group = db
            .create_group(NewGroup {
                start_date: Some("2026-03-01".into()),
                end_date: Some(" 2026-03-14 ".into()),
                target_language: Some(" ja ".into()),
                ..new("  Japan Reise ")
            })
            .unwrap();
        assert_eq!(group.name, "Japan Reise");
        assert_eq!(group.target_language.as_deref(), Some("ja"));
        assert_eq!(group.end_date.as_deref(), Some("2026-03-14"));
        assert_eq!(db.group(&group.id).unwrap(), Some(group));
    }

    #[test]
    fn me_joins_new_groups() {
        let db = Db::open_in_memory().unwrap();
        me(&db);
        let group = db.create_group(new("Japan")).unwrap();
        let members = db.group_members(&group.id).unwrap();
        assert_eq!(members.len(), 1);
        assert!(members[0].person.is_me);
    }

    #[test]
    fn list_newest_first() {
        let db = Db::open_in_memory().unwrap();
        db.create_group(new("WG")).unwrap();
        db.create_group(new("Japan")).unwrap();
        let names: Vec<_> = db.groups().unwrap().into_iter().map(|g| g.name).collect();
        assert_eq!(names, ["Japan", "WG"]);
    }

    #[test]
    fn update_changes_every_field() {
        let db = Db::open_in_memory().unwrap();
        let group = db.create_group(new("Japan")).unwrap();
        let updated = Group {
            name: "Japan 2026".into(),
            icon: "mountain".into(),
            color: "pale-oak".into(),
            base_currency: Currency::from_code("JPY").unwrap(),
            start_date: Some("2026-03-01".into()),
            end_date: Some("2026-03-14".into()),
            target_language: Some("en".into()),
            ..group
        };
        db.update_group(&updated).unwrap();
        assert_eq!(db.group(&updated.id).unwrap(), Some(updated.clone()));
        // Blank follows the global target language again (TRL-05).
        let follows = Group {
            target_language: Some(" ".into()),
            ..updated
        };
        db.update_group(&follows).unwrap();
        let stored = db.group(&follows.id).unwrap().unwrap();
        assert_eq!(stored.target_language, None);
    }

    #[test]
    fn rejects_empty_name_and_bad_period() {
        let db = Db::open_in_memory().unwrap();
        assert!(matches!(
            db.create_group(new("  ")),
            Err(StorageError::InvalidInput(_))
        ));
        assert!(matches!(
            db.create_group(NewGroup {
                start_date: Some("2026-03-14".into()),
                end_date: Some("2026-03-01".into()),
                target_language: None,
                ..new("Japan")
            }),
            Err(StorageError::Group(GroupError::EndBeforeStart))
        ));
        let group = db.create_group(new("Japan")).unwrap();
        assert!(matches!(
            db.update_group(&Group {
                start_date: Some("14.03.2026".into()),
                ..group.clone()
            }),
            Err(StorageError::Group(GroupError::InvalidDate(_)))
        ));
        assert!(db.groups().unwrap().iter().all(|g| g.start_date.is_none()));
    }

    #[test]
    fn soft_delete_and_restore() {
        let db = Db::open_in_memory().unwrap();
        let group = db.create_group(new("Japan")).unwrap();
        assert!(matches!(
            db.restore_group(&group.id),
            Err(StorageError::NotFound)
        ));

        db.delete_group(&group.id).unwrap();
        assert_eq!(db.group(&group.id).unwrap(), None);
        assert!(db.groups().unwrap().is_empty());
        assert!(matches!(
            db.delete_group(&group.id),
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            db.update_group(&group),
            Err(StorageError::NotFound)
        ));
        let row_stays: i64 = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT count(*) FROM expense_group WHERE id = ?1",
                    [group.id.as_str()],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(row_stays, 1);

        db.restore_group(&group.id).unwrap();
        assert_eq!(db.group(&group.id).unwrap(), Some(group));
    }

    #[test]
    fn records_origin_device() {
        let db = Db::open_in_memory().unwrap();
        let group = db.create_group(new("Japan")).unwrap();
        let origin: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT origin_device_id FROM expense_group WHERE id = ?1",
                    [group.id.as_str()],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(origin, db.device_id());
    }
}
