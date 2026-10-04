use invuso_core::domain::{Currency, Person, PersonId};
use rusqlite::Connection;

use super::db::new_id;
use super::settings::{HOME_CURRENCY, TARGET_LANGUAGE};
use super::{Db, StorageError, people, settings};

/// Avatar color of "Ich" until people can pick colors (PER-01).
const ME_COLOR: &str = "cerulean";

/// What onboarding asks for and Settings → Profile edits (PER-02, SET-01..03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Name of "Ich".
    pub name: String,
    pub home_currency: Currency,
    /// ISO 639-1 code, e.g. `"de"`.
    pub target_language: String,
}

impl Db {
    /// The user's profile; `None` until onboarding has saved it.
    pub fn profile(&self) -> Result<Option<Profile>, StorageError> {
        self.with(|conn| {
            let Some(me) = people::me(conn)? else {
                return Ok(None);
            };
            let currency = settings::get(conn, HOME_CURRENCY)?
                .ok_or(StorageError::InvalidInput("home currency missing"))?;
            let home_currency = Currency::from_code(&currency)
                .map_err(|_| StorageError::InvalidInput("stored home currency is unknown"))?;
            let target_language = settings::get(conn, TARGET_LANGUAGE)?
                .ok_or(StorageError::InvalidInput("target language missing"))?;
            Ok(Some(Profile {
                name: me.name,
                home_currency,
                target_language,
            }))
        })
    }

    /// Creates "Ich" on first use, otherwise renames it, and stores both
    /// settings, all in one transaction so onboarding never ends half-done.
    pub fn save_profile(&self, profile: &Profile) -> Result<(), StorageError> {
        let name = people::valid_name(&profile.name)?;
        let language = profile.target_language.trim();
        if language.is_empty() {
            return Err(StorageError::InvalidInput(
                "target language must not be empty",
            ));
        }
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            save_me(&tx, self.device_id(), name)?;
            settings::set(&tx, HOME_CURRENCY, profile.home_currency.code())?;
            settings::set(&tx, TARGET_LANGUAGE, language)?;
            tx.commit()?;
            Ok(())
        })
    }
}

fn save_me(conn: &Connection, device_id: &str, name: String) -> Result<(), StorageError> {
    match people::me(conn)? {
        Some(me) => {
            people::update(conn, &Person { name, ..me })?;
        }
        None => people::insert(
            conn,
            device_id,
            &Person {
                id: PersonId::new(new_id()),
                name,
                color: ME_COLOR.to_string(),
                avatar_path: None,
                is_me: true,
                note: None,
            },
        )?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, currency: &str, language: &str) -> Profile {
        Profile {
            name: name.into(),
            home_currency: Currency::from_code(currency).unwrap(),
            target_language: language.into(),
        }
    }

    #[test]
    fn none_before_onboarding() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.profile().unwrap(), None);
    }

    #[test]
    fn first_save_creates_me_and_settings() {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&profile(" Konstantin ", "EUR", "de"))
            .unwrap();

        assert_eq!(
            db.profile().unwrap(),
            Some(profile("Konstantin", "EUR", "de"))
        );
        let me = db.me().unwrap().unwrap();
        assert!(me.is_me);
        assert_eq!(db.setting(HOME_CURRENCY).unwrap().as_deref(), Some("EUR"));
        assert_eq!(db.setting(TARGET_LANGUAGE).unwrap().as_deref(), Some("de"));
    }

    #[test]
    fn later_saves_update_the_same_me() {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&profile("Konstantin", "EUR", "de"))
            .unwrap();
        let id = db.me().unwrap().unwrap().id;

        db.save_profile(&profile("Kosta", "JPY", "en")).unwrap();

        assert_eq!(db.profile().unwrap(), Some(profile("Kosta", "JPY", "en")));
        assert_eq!(db.me().unwrap().unwrap().id, id);
        assert_eq!(db.people().unwrap().len(), 1);
    }

    #[test]
    fn rejects_empty_name_without_writing() {
        let db = Db::open_in_memory().unwrap();
        assert!(matches!(
            db.save_profile(&profile("  ", "EUR", "de")),
            Err(StorageError::InvalidInput(_))
        ));
        assert_eq!(db.profile().unwrap(), None);
        assert_eq!(db.setting(HOME_CURRENCY).unwrap(), None);
    }
}
