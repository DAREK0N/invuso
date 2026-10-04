use invuso_core::domain::{Person, PersonId};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::db::{new_id, now_ms};
use super::{Db, StorageError};

/// Input for creating a person (PER-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPerson {
    pub name: String,
    pub color: String,
    pub is_me: bool,
}

const COLUMNS: &str = "id, name, color, avatar_path, is_me, note";

impl Db {
    pub fn create_person(&self, new: NewPerson) -> Result<Person, StorageError> {
        let name = valid_name(&new.name)?;
        let person = Person {
            id: PersonId::new(new_id()),
            name,
            color: new.color,
            avatar_path: None,
            is_me: new.is_me,
            note: None,
        };
        self.with(|conn| insert(conn, self.device_id(), &person))?;
        Ok(person)
    }

    pub fn person(&self, id: &PersonId) -> Result<Option<Person>, StorageError> {
        self.with(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {COLUMNS} FROM person WHERE id = ?1 AND deleted_at IS NULL"),
                    [id.as_str()],
                    person_from_row,
                )
                .optional()?)
        })
    }

    /// All people, "Ich" first, then by name.
    pub fn people(&self) -> Result<Vec<Person>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM person WHERE deleted_at IS NULL
                 ORDER BY is_me DESC, name COLLATE NOCASE, id"
            ))?;
            let people = statement
                .query_map([], person_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(people)
        })
    }

    /// The user's own person (PER-02), if onboarding created it already.
    pub fn me(&self) -> Result<Option<Person>, StorageError> {
        self.with(me)
    }

    pub fn update_person(&self, person: &Person) -> Result<(), StorageError> {
        let name = valid_name(&person.name)?;
        let changed = self.with(|conn| {
            update(
                conn,
                &Person {
                    name,
                    ..person.clone()
                },
            )
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Soft delete: the row stays for history and sync (idee.md 4).
    pub fn delete_person(&self, id: &PersonId) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            let now = now_ms();
            Ok(conn.execute(
                "UPDATE person SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
                params![id.as_str(), now],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }
}

pub(super) fn insert(
    conn: &Connection,
    device_id: &str,
    person: &Person,
) -> Result<(), StorageError> {
    conn.execute(
        "INSERT INTO person (id, name, color, is_me, created_at, updated_at, origin_device_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
        params![
            person.id.as_str(),
            person.name,
            person.color,
            person.is_me,
            now_ms(),
            device_id
        ],
    )?;
    Ok(())
}

/// Number of rows changed: 0 when the person does not exist (any more).
pub(super) fn update(conn: &Connection, person: &Person) -> Result<usize, StorageError> {
    Ok(conn.execute(
        "UPDATE person SET name = ?2, color = ?3, avatar_path = ?4, is_me = ?5, note = ?6, updated_at = ?7
         WHERE id = ?1 AND deleted_at IS NULL",
        params![
            person.id.as_str(),
            person.name,
            person.color,
            person.avatar_path,
            person.is_me,
            person.note,
            now_ms()
        ],
    )?)
}

pub(super) fn me(conn: &Connection) -> Result<Option<Person>, StorageError> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM person WHERE is_me = 1 AND deleted_at IS NULL"),
            [],
            person_from_row,
        )
        .optional()?)
}

pub(super) fn valid_name(name: &str) -> Result<String, StorageError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(StorageError::InvalidInput("name must not be empty"));
    }
    Ok(trimmed.to_string())
}

fn person_from_row(row: &Row<'_>) -> rusqlite::Result<Person> {
    Ok(Person {
        id: PersonId::new(row.get::<_, String>(0)?),
        name: row.get(1)?,
        color: row.get(2)?,
        avatar_path: row.get(3)?,
        is_me: row.get(4)?,
        note: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(name: &str, is_me: bool) -> NewPerson {
        NewPerson {
            name: name.into(),
            color: "cerulean".into(),
            is_me,
        }
    }

    #[test]
    fn create_list_and_me_first() {
        let db = Db::open_in_memory().unwrap();
        db.create_person(new("Ben", false)).unwrap();
        let me = db.create_person(new("  Konstantin ", true)).unwrap();
        db.create_person(new("anna", false)).unwrap();

        assert_eq!(me.name, "Konstantin");
        assert_eq!(db.me().unwrap(), Some(me.clone()));
        let names: Vec<_> = db.people().unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["Konstantin", "anna", "Ben"]);
    }

    #[test]
    fn only_one_me() {
        let db = Db::open_in_memory().unwrap();
        db.create_person(new("Me", true)).unwrap();
        assert!(matches!(
            db.create_person(new("Me too", true)),
            Err(StorageError::Sqlite(_))
        ));
    }

    #[test]
    fn update_and_soft_delete() {
        let db = Db::open_in_memory().unwrap();
        let mut ben = db.create_person(new("Ben", false)).unwrap();
        ben.note = Some("Kassenwart".into());
        db.update_person(&ben).unwrap();
        assert_eq!(db.person(&ben.id).unwrap(), Some(ben.clone()));

        db.delete_person(&ben.id).unwrap();
        assert_eq!(db.person(&ben.id).unwrap(), None);
        assert!(db.people().unwrap().is_empty());
        assert!(matches!(
            db.delete_person(&ben.id),
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            db.update_person(&ben),
            Err(StorageError::NotFound)
        ));
    }

    #[test]
    fn records_origin_device() {
        let db = Db::open_in_memory().unwrap();
        let ben = db.create_person(new("Ben", false)).unwrap();
        let origin: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT origin_device_id FROM person WHERE id = ?1",
                    [ben.id.as_str()],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(origin, db.device_id());
    }

    #[test]
    fn rejects_empty_name() {
        let db = Db::open_in_memory().unwrap();
        assert!(matches!(
            db.create_person(new("   ", false)),
            Err(StorageError::InvalidInput(_))
        ));
    }
}
