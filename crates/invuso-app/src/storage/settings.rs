use invuso_core::domain::Currency;
use rusqlite::{Connection, OptionalExtension, params};

use super::db::{new_id, now_ms};
use super::{Db, StorageError};

/// Key of this installation's id (see [`Db::device_id`]).
const DEVICE_ID: &str = "device_id";
/// ISO 4217 code of the home currency, the default base of new groups (SET-01).
pub const HOME_CURRENCY: &str = "home_currency";
/// ISO 639-1 code of the language receipts are translated into (SET-02).
pub const TARGET_LANGUAGE: &str = "target_language";
/// Favorite currencies, shown first in the currency picker (UI-15).
pub const FAVORITE_CURRENCY_LIST: &str = "favorite_currencies";
/// Currencies picked last, newest first (UI-15).
pub const RECENT_CURRENCY_LIST: &str = "recent_currencies";
/// Last selection of the currency converter (FX-05).
pub const CONVERTER_FROM: &str = "converter_from";
pub const CONVERTER_TO: &str = "converter_to";
/// Id of the manual rate the converter uses instead of the archived one
/// while its pair is shown (FX-10); empty = none.
pub const CONVERTER_MANUAL_RATE: &str = "converter_manual_rate";
/// Group (empty = none) and currency of the last saved expense, preselected
/// in the next expense form (AP-11).
pub const LAST_EXPENSE_GROUP: &str = "last_expense_group";
pub const LAST_EXPENSE_CURRENCY: &str = "last_expense_currency";
/// How sure a downloaded translation model must be (`strict`, `balanced`,
/// `all`; AP-21b).
pub const TRANSLATION_CONFIDENCE: &str = "translation_confidence";
/// Id of the group marked as active (GRP-05); empty = none.
pub(super) const ACTIVE_GROUP: &str = "active_group";

impl Db {
    /// Value of a global setting (idee.md 4.1 `Settings`).
    pub fn setting(&self, key: &str) -> Result<Option<String>, StorageError> {
        self.with(|conn| get(conn, key))
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.with(|conn| set(conn, key, value))
    }

    /// A currency stored under `key`; `None` if unset or no longer valid.
    pub fn currency_setting(&self, key: &str) -> Result<Option<Currency>, StorageError> {
        Ok(self
            .setting(key)?
            .and_then(|code| Currency::from_code(&code).ok()))
    }

    /// A list of currencies stored under `key`, in stored order; `None` if
    /// it was never saved (an empty list is a saved choice). Unknown codes
    /// are skipped.
    pub fn currency_list(&self, key: &str) -> Result<Option<Vec<Currency>>, StorageError> {
        Ok(self.setting(key)?.map(|stored| {
            stored
                .split(',')
                .filter_map(|code| Currency::from_code(code).ok())
                .collect()
        }))
    }

    pub fn set_currency_list(
        &self,
        key: &str,
        currencies: &[Currency],
    ) -> Result<(), StorageError> {
        let codes: Vec<&str> = currencies.iter().map(|c| c.code()).collect();
        self.set_setting(key, &codes.join(","))
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
    fn currency_settings_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let cur = |code| Currency::from_code(code).unwrap();
        assert_eq!(db.currency_list(RECENT_CURRENCY_LIST).unwrap(), None);
        db.set_currency_list(RECENT_CURRENCY_LIST, &[cur("JPY"), cur("EUR")])
            .unwrap();
        assert_eq!(
            db.currency_list(RECENT_CURRENCY_LIST).unwrap(),
            Some(vec![cur("JPY"), cur("EUR")])
        );
        db.set_currency_list(FAVORITE_CURRENCY_LIST, &[]).unwrap();
        assert_eq!(
            db.currency_list(FAVORITE_CURRENCY_LIST).unwrap(),
            Some(vec![])
        );

        db.set_setting(CONVERTER_FROM, "jpy").unwrap();
        assert_eq!(
            db.currency_setting(CONVERTER_FROM).unwrap(),
            Some(cur("JPY"))
        );
        db.set_setting(CONVERTER_TO, "XYZ").unwrap();
        assert_eq!(db.currency_setting(CONVERTER_TO).unwrap(), None);
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
