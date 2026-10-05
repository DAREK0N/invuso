//! Remembered translations and the user's corrections of them
//! (idee.md 4.1 `TranslationCacheEntry`, TRL-04).

use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};

use super::db::now_ms;
use super::{Db, StorageError};

/// Source language of a correction whose receipt language is unknown
/// (BCP 47 "undetermined").
pub const UNKNOWN_LANGUAGE: &str = "und";

impl Db {
    /// What is remembered for each of `texts` in `target`: the user's own
    /// correction from any receipt language first (TRL-04), otherwise an
    /// earlier machine translation from `source`. Texts without an entry
    /// are left out.
    pub fn remembered_translations(
        &self,
        source: Option<&str>,
        target: &str,
        texts: &[String],
    ) -> Result<HashMap<String, String>, StorageError> {
        self.with(|conn| {
            let mut corrections = conn.prepare(
                "SELECT translated_text FROM translation_cache
                 WHERE target_lang = ?1 AND source_text = ?2 AND from_user = 1
                 ORDER BY updated_at DESC LIMIT 1",
            )?;
            let mut machine = conn.prepare(
                "SELECT translated_text FROM translation_cache
                 WHERE source_lang = ?1 AND target_lang = ?2 AND source_text = ?3",
            )?;
            let mut found = HashMap::new();
            for text in texts {
                let correction = corrections
                    .query_row(params![target, text], |row| row.get::<_, String>(0))
                    .optional()?;
                let translation = match (correction, source) {
                    (Some(correction), _) => Some(correction),
                    (None, Some(source)) => machine
                        .query_row(params![source, target, text], |row| row.get(0))
                        .optional()?,
                    (None, None) => None,
                };
                if let Some(translation) = translation {
                    found.insert(text.clone(), translation);
                }
            }
            Ok(found)
        })
    }

    /// Stores translations of `source` texts into `target`. A machine
    /// translation never replaces a correction of the user.
    pub fn remember_translations(
        &self,
        source: &str,
        target: &str,
        pairs: &[(String, String)],
        from_user: bool,
    ) -> Result<(), StorageError> {
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            {
                let mut insert = tx.prepare(
                    "INSERT INTO translation_cache
                         (source_lang, target_lang, source_text, translated_text, from_user,
                          updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (source_lang, target_lang, source_text) DO UPDATE
                     SET translated_text = excluded.translated_text,
                         from_user = excluded.from_user,
                         updated_at = excluded.updated_at
                     WHERE excluded.from_user = 1 OR translation_cache.from_user = 0",
                )?;
                let now = now_ms();
                for (text, translation) in pairs {
                    insert.execute(params![source, target, text, translation, from_user, now])?;
                }
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// The language a receipt's text was detected in (TRL-02).
    pub fn receipt_language(&self, id: &str) -> Result<Option<String>, StorageError> {
        self.with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT detected_language FROM receipt WHERE id = ?1",
                    [id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten())
        })
    }

    pub fn set_receipt_language(&self, id: &str, language: &str) -> Result<(), StorageError> {
        self.with(|conn| {
            conn.execute(
                "UPDATE receipt SET detected_language = ?2, updated_at = ?3
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id, language, now_ms()],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| t.to_string()).collect()
    }

    fn pair(text: &str, translation: &str) -> (String, String) {
        (text.into(), translation.into())
    }

    #[test]
    fn machine_translations_are_found_by_language_pair() {
        let db = Db::open_in_memory().unwrap();
        db.remember_translations("ja", "de", &[pair("牛乳", "Milch")], false)
            .unwrap();
        let wanted = texts(&["牛乳", "パン"]);
        let found = db
            .remembered_translations(Some("ja"), "de", &wanted)
            .unwrap();
        assert_eq!(found, HashMap::from([("牛乳".into(), "Milch".into())]));
        assert!(
            db.remembered_translations(Some("ja"), "en", &wanted)
                .unwrap()
                .is_empty()
        );
        assert!(
            db.remembered_translations(None, "de", &wanted)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn corrections_win_over_machine_translations_and_stay() {
        let db = Db::open_in_memory().unwrap();
        db.remember_translations("ja", "de", &[pair("生ビール", "Rohbier")], false)
            .unwrap();
        db.remember_translations("ja", "de", &[pair("生ビール", "Bier vom Fass")], true)
            .unwrap();
        // A later machine translation does not overwrite the correction.
        db.remember_translations("ja", "de", &[pair("生ビール", "Rohbier")], false)
            .unwrap();
        let wanted = texts(&["生ビール"]);
        // Corrections apply whatever language the next receipt is read in.
        for source in [Some("ja"), None, Some("zh")] {
            let found = db.remembered_translations(source, "de", &wanted).unwrap();
            assert_eq!(found["生ビール"], "Bier vom Fass");
        }
    }

    #[test]
    fn newest_correction_wins() {
        let db = Db::open_in_memory().unwrap();
        db.remember_translations(UNKNOWN_LANGUAGE, "de", &[pair("Mi1ch", "Milch")], true)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.remember_translations("de", "de", &[pair("Mi1ch", "Vollmilch")], true)
            .unwrap();
        let found = db
            .remembered_translations(Some("de"), "de", &texts(&["Mi1ch"]))
            .unwrap();
        assert_eq!(found["Mi1ch"], "Vollmilch");
    }
}
