use rusqlite::{Connection, OptionalExtension, params};

use super::db::{new_id, now_ms};
use super::{Db, StorageError};

/// Key of this installation's id (see [`Db::device_id`]).
const DEVICE_ID: &str = "device_id";
/// ISO 4217 code of the home currency, the default base of new groups (SET-01).
pub const HOME_CURRENCY: &str = "home_currency";
/// ISO 639-1 code of the language receipts are translated into (SET-02).
pub const TARGET_LANGUAGE: &str = "target_language";

impl Db {
    /// Value of a global setting (idee.md 4.1 `Settings`).
    pub fn setting(&self, key: &str) -> Result<Option<String>, StorageError> {
        self.with(|conn| get(conn, key))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.with(|conn| set(conn, key, value))
    }
}

pub(super) fn ensure_device_id(conn: &Connection) -> Result<String, StorageError> {
    if let Some(id) = get(conn, DEVICE_ID)? {
        return Ok(id);
    }
    let id = new_id();
    set(conn, DEVICE_ID, &id)?;
    Ok(id)
}

pub(super) fn get(conn: &Connection, key: &str) -> Result<Option<String>, StorageError> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

pub(super) fn set(conn: &Connection, key: &str, value: &str) -> Result<(), StorageError> {
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value, now_ms()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_overwrite() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.setting(HOME_CURRENCY).unwrap(), None);
        db.set_setting(HOME_CURRENCY, "EUR").unwrap();
        db.set_setting(HOME_CURRENCY, "JPY").unwrap();
        assert_eq!(db.setting(HOME_CURRENCY).unwrap().as_deref(), Some("JPY"));
    }

    #[test]
    fn device_id_is_created_once() {
        let db = Db::open_in_memory().unwrap();
        let id = db.device_id().to_string();
        assert!(!id.is_empty());
        assert_eq!(db.setting(DEVICE_ID).unwrap(), Some(id.clone()));
        assert_eq!(db.with(ensure_device_id).unwrap(), id);
    }
}
