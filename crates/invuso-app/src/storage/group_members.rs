use std::str::FromStr;

use invuso_core::domain::{GroupId, GroupMember, Person, PersonId};
use rusqlite::{Connection, OptionalExtension, params};
use rust_decimal::Decimal;

use super::db::{new_id, now_ms};
use super::{Db, StorageError, groups};

impl Db {
    /// Members of a group, "Ich" first, then by name. People who were
    /// deleted globally are left out, like everywhere else.
    pub fn group_members(&self, group: &GroupId) -> Result<Vec<GroupMember>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(
                "SELECT p.id, p.name, p.color, p.avatar_path, p.is_me, p.note, gm.default_weight
                 FROM group_member gm JOIN person p ON p.id = gm.person_id
                 WHERE gm.group_id = ?1 AND gm.deleted_at IS NULL AND p.deleted_at IS NULL
                 ORDER BY p.is_me DESC, p.name COLLATE NOCASE, p.id",
            )?;
            let members = statement
                .query_map([group.as_str()], |row| {
                    let weight: String = row.get(6)?;
                    Ok(GroupMember {
                        person: Person {
                            id: PersonId::new(row.get::<_, String>(0)?),
                            name: row.get(1)?,
                            color: row.get(2)?,
                            avatar_path: row.get(3)?,
                            is_me: row.get(4)?,
                            note: row.get(5)?,
                        },
                        default_weight: Decimal::from_str(&weight).map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                6,
                                rusqlite::types::Type::Text,
                                e.into(),
                            )
                        })?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(members)
        })
    }

    /// Adds a person to a group (GRP-03). Someone who was removed before
    /// gets their old membership back, default weight included.
    pub fn add_group_member(&self, group: &GroupId, person: &PersonId) -> Result<(), StorageError> {
        self.with(|conn| {
            groups::check_group(conn, group)?;
            conn.query_row(
                "SELECT 1 FROM person WHERE id = ?1 AND deleted_at IS NULL",
                [person.as_str()],
                |_| Ok(()),
            )
            .optional()?
            .ok_or(StorageError::NotFound)?;

            let existing: Option<(String, Option<i64>)> = conn
                .query_row(
                    "SELECT id, deleted_at FROM group_member
                     WHERE group_id = ?1 AND person_id = ?2
                     ORDER BY deleted_at IS NULL DESC, updated_at DESC LIMIT 1",
                    params![group.as_str(), person.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            match existing {
                Some((_, None)) => Err(StorageError::AlreadyMember),
                Some((id, Some(_))) => {
                    conn.execute(
                        "UPDATE group_member SET deleted_at = NULL, updated_at = ?2 WHERE id = ?1",
                        params![id, now_ms()],
                    )?;
                    Ok(())
                }
                None => insert(conn, self.device_id(), group, person),
            }
        })
    }

    /// Removes a person from a group (soft, idee.md 4). "Ich" stays in every
    /// group (user decision in AP-08).
    pub fn remove_group_member(
        &self,
        group: &GroupId,
        person: &PersonId,
    ) -> Result<(), StorageError> {
        self.with(|conn| {
            let is_me: Option<bool> = conn
                .query_row(
                    "SELECT is_me FROM person WHERE id = ?1",
                    [person.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            if is_me == Some(true) {
                return Err(StorageError::CannotRemoveMe);
            }
            let now = now_ms();
            let changed = conn.execute(
                "UPDATE group_member SET deleted_at = ?3, updated_at = ?3
                 WHERE group_id = ?1 AND person_id = ?2 AND deleted_at IS NULL",
                params![group.as_str(), person.as_str(), now],
            )?;
            if changed == 0 {
                return Err(StorageError::NotFound);
            }
            Ok(())
        })
    }
}

/// New membership with the default weight 1 (PER-04).
pub(super) fn insert(
    conn: &Connection,
    device_id: &str,
    group: &GroupId,
    person: &PersonId,
) -> Result<(), StorageError> {
    let now = now_ms();
    conn.execute(
        "INSERT INTO group_member
             (id, group_id, person_id, default_weight, created_at, updated_at, origin_device_id)
         VALUES (?1, ?2, ?3, '1', ?4, ?4, ?5)",
        params![new_id(), group.as_str(), person.as_str(), now, device_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::{Currency, Group};

    use super::*;
    use crate::storage::{NewGroup, NewPerson};

    fn person(db: &Db, name: &str, is_me: bool) -> Person {
        db.create_person(NewPerson {
            name: name.into(),
            color: "cerulean".into(),
            is_me,
            note: None,
        })
        .unwrap()
    }

    fn group(db: &Db) -> Group {
        db.create_group(NewGroup {
            name: "Japan".into(),
            icon: "plane".into(),
            color: "cerulean".into(),
            base_currency: Currency::from_code("EUR").unwrap(),
            start_date: None,
            end_date: None,
            target_language: None,
        })
        .unwrap()
    }

    fn names(db: &Db, group: &Group) -> Vec<String> {
        db.group_members(&group.id)
            .unwrap()
            .into_iter()
            .map(|m| m.person.name)
            .collect()
    }

    #[test]
    fn add_lists_me_first_with_default_weight() {
        let db = Db::open_in_memory().unwrap();
        let ben = person(&db, "Ben", false);
        let anna = person(&db, "anna", false);
        person(&db, "Me", true);
        let japan = group(&db);
        db.add_group_member(&japan.id, &ben.id).unwrap();
        db.add_group_member(&japan.id, &anna.id).unwrap();

        assert_eq!(names(&db, &japan), ["Me", "anna", "Ben"]);
        assert!(
            db.group_members(&japan.id)
                .unwrap()
                .iter()
                .all(|m| m.default_weight == Decimal::ONE)
        );
    }

    #[test]
    fn a_person_cannot_join_twice() {
        let db = Db::open_in_memory().unwrap();
        let ben = person(&db, "Ben", false);
        let japan = group(&db);
        db.add_group_member(&japan.id, &ben.id).unwrap();
        assert!(matches!(
            db.add_group_member(&japan.id, &ben.id),
            Err(StorageError::AlreadyMember)
        ));
        // The unique index guards the table even without the check above.
        let result = db.with(|conn| insert(conn, db.device_id(), &japan.id, &ben.id));
        assert!(matches!(result, Err(StorageError::Sqlite(_))));
        assert_eq!(names(&db, &japan), ["Ben"]);
    }

    #[test]
    fn remove_and_add_again_reuses_the_membership() {
        let db = Db::open_in_memory().unwrap();
        let ben = person(&db, "Ben", false);
        let japan = group(&db);
        db.add_group_member(&japan.id, &ben.id).unwrap();
        db.with(|conn| {
            Ok(conn.execute(
                "UPDATE group_member SET default_weight = '0.5' WHERE person_id = ?1",
                [ben.id.as_str()],
            )?)
        })
        .unwrap();

        db.remove_group_member(&japan.id, &ben.id).unwrap();
        assert!(names(&db, &japan).is_empty());
        assert!(matches!(
            db.remove_group_member(&japan.id, &ben.id),
            Err(StorageError::NotFound)
        ));

        db.add_group_member(&japan.id, &ben.id).unwrap();
        let members = db.group_members(&japan.id).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].default_weight, Decimal::new(5, 1));
        let rows: i64 = db
            .with(|conn| {
                Ok(conn.query_row("SELECT count(*) FROM group_member", [], |row| row.get(0))?)
            })
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn me_cannot_be_removed() {
        let db = Db::open_in_memory().unwrap();
        person(&db, "Me", true);
        let japan = group(&db);
        let me = db.me().unwrap().unwrap();
        assert!(matches!(
            db.remove_group_member(&japan.id, &me.id),
            Err(StorageError::CannotRemoveMe)
        ));
        assert_eq!(names(&db, &japan), ["Me"]);
    }

    #[test]
    fn deleted_people_and_groups_are_rejected() {
        let db = Db::open_in_memory().unwrap();
        let ben = person(&db, "Ben", false);
        let cleo = person(&db, "Cleo", false);
        let japan = group(&db);
        db.add_group_member(&japan.id, &cleo.id).unwrap();

        db.delete_person(&cleo.id).unwrap();
        assert!(names(&db, &japan).is_empty());
        db.delete_person(&ben.id).unwrap();
        assert!(matches!(
            db.add_group_member(&japan.id, &ben.id),
            Err(StorageError::NotFound)
        ));

        db.restore_person(&ben.id).unwrap();
        db.restore_person(&cleo.id).unwrap();
        assert_eq!(names(&db, &japan), ["Cleo"]);
        db.delete_group(&japan.id).unwrap();
        assert!(matches!(
            db.add_group_member(&japan.id, &ben.id),
            Err(StorageError::NotFound)
        ));
    }

    #[test]
    fn records_origin_device() {
        let db = Db::open_in_memory().unwrap();
        let ben = person(&db, "Ben", false);
        let japan = group(&db);
        db.add_group_member(&japan.id, &ben.id).unwrap();
        let origin: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT origin_device_id FROM group_member WHERE person_id = ?1",
                    [ben.id.as_str()],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(origin, db.device_id());
    }
}
