//! Translating the lines of a receipt (idee.md 7.2 step 4, TRL-01..05).
//!
//! Order of preference (decision 10.2): the user's own corrections and
//! earlier translations from `translation_cache`, then the device's own
//! translator, then a downloaded Opus-MT pack. A machine translation is
//! only a suggestion; the user's text always wins (idee.md 1.4 principle 5).

pub mod downloads;
mod opus;
pub mod packs;
mod tokenizer;

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;

use invuso_core::domain::LineItem;

use crate::platform::{self, MachineText, Translation, Translator};
use crate::storage::{Db, StorageError, TRANSLATION_CONFIDENCE, UNKNOWN_LANGUAGE};

/// How sure a downloaded model must be before its translation is shown
/// (user setting on the language packs screen). Lines below stay as
/// printed. The device's own translator reports no confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranslationConfidence {
    /// Only translations the model was very sure of.
    Strict,
    #[default]
    Balanced,
    /// Everything, including made-up sentences.
    All,
}

impl TranslationConfidence {
    pub const ALL: [Self; 3] = [Self::Strict, Self::Balanced, Self::All];

    /// Stored value in `settings`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Balanced => "balanced",
            Self::All => "all",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.code() == code)
    }

    /// The level the user picked; the default if none or unreadable.
    pub fn current(db: &Db) -> Self {
        db.setting(TRANSLATION_CONFIDENCE)
            .ok()
            .flatten()
            .and_then(|code| Self::from_code(&code))
            .unwrap_or_default()
    }

    pub fn save(self, db: &Db) -> Result<(), StorageError> {
        db.set_setting(TRANSLATION_CONFIDENCE, self.code())
    }
}

/// What happened to the lines that were not remembered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineTranslation {
    /// Everything was remembered, or the engine translated the rest.
    Done,
    /// Receipt and target language are the same (TRL-03).
    SameLanguage,
    /// The receipt's language could not be told (TRL-02).
    UnknownLanguage,
    /// No engine on this device translates this pair.
    Unavailable,
    /// The engine failed; the message is for the user.
    Failed(String),
}

/// The device's translator first, a downloaded pack as fallback.
pub struct OnDeviceTranslator<S> {
    system: S,
    /// Where packs are installed; `None` without app storage (host builds).
    data_dir: Option<PathBuf>,
    confidence: TranslationConfidence,
}

/// The translation engines of this device, in order of preference;
/// translations of a pack are shown from `confidence` on.
pub fn on_device_translator(
    confidence: TranslationConfidence,
) -> OnDeviceTranslator<impl Translator> {
    OnDeviceTranslator {
        system: platform::system_translator(),
        data_dir: platform::data_dir().ok(),
        confidence,
    }
}

impl<S: Translator> Translator for OnDeviceTranslator<S> {
    fn translate(
        &self,
        source: &str,
        target: &str,
        texts: Vec<String>,
    ) -> impl Future<Output = Result<Translation, String>> + Send + use<S> {
        let system = self.system.translate(source, target, texts.clone());
        let pack = packs::find(source, target);
        let data_dir = self.data_dir.clone();
        let confidence = self.confidence;
        async move {
            let from_system = system.await;
            if matches!(from_system, Ok(Translation::Done(_))) {
                return from_system;
            }
            let installed = pack
                .zip(data_dir)
                .filter(|(pack, dir)| packs::is_installed(dir, pack));
            let Some((pack, dir)) = installed else {
                return from_system;
            };
            // Loading and running the model takes seconds; off the UI thread.
            tokio::task::spawn_blocking(move || {
                translate_with_pack(&packs::pack_dir(&dir, pack), &texts, confidence)
            })
            .await
            .map_err(|e| e.to_string())?
        }
    }
}

/// Loads the pack for this one receipt and translates its lines;
/// translations below `confidence` come back empty. Only those passing the
/// strictest level are remembered, so a later, stricter setting never meets
/// an unsure translation in the cache. The model is dropped afterwards: it
/// needs about as much memory as its files, too much to keep next to the
/// text recognition.
fn translate_with_pack(
    dir: &std::path::Path,
    texts: &[String],
    confidence: TranslationConfidence,
) -> Result<Translation, String> {
    let engine = opus::OpusMt::load(dir).map_err(|e| e.to_string())?;
    let translated = texts
        .iter()
        .map(|text| {
            let translated = engine.translate(text).map_err(|e| e.to_string())?;
            Ok(match translated {
                Some(t) if t.passes(confidence) => MachineText {
                    remember: t.passes(TranslationConfidence::Strict),
                    text: t.text,
                },
                _ => MachineText {
                    text: String::new(),
                    remember: false,
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Translation::Done(translated))
}

/// Translations by original text, and how the machine part went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineTranslations {
    pub texts: HashMap<String, String>,
    pub machine: MachineTranslation,
}

/// Translates `texts` of a receipt in `source` into `target`. New machine
/// translations are remembered, so the next receipt with the same line
/// needs no engine.
pub async fn translate_lines(
    db: &Db,
    translator: &impl Translator,
    source: Option<&str>,
    target: &str,
    texts: &[String],
) -> Result<LineTranslations, StorageError> {
    let mut wanted: Vec<String> = texts
        .iter()
        .filter(|text| !text.trim().is_empty())
        .cloned()
        .collect();
    wanted.sort();
    wanted.dedup();

    let mut found = db.remembered_translations(source, target, &wanted)?;
    let missing: Vec<String> = wanted
        .into_iter()
        .filter(|text| !found.contains_key(text))
        .collect();

    let machine = match source {
        _ if missing.is_empty() => MachineTranslation::Done,
        None => MachineTranslation::UnknownLanguage,
        Some(source) if source == target => MachineTranslation::SameLanguage,
        Some(source) => match translator.translate(source, target, missing.clone()).await {
            Ok(Translation::Done(translated)) => {
                let mut kept = Vec::new();
                for (text, machine) in missing.into_iter().zip(translated) {
                    let translation = machine.text.trim().to_string();
                    if translation.is_empty() || translation == text {
                        continue;
                    }
                    if machine.remember {
                        kept.push((text.clone(), translation.clone()));
                    }
                    found.insert(text, translation);
                }
                db.remember_translations(source, target, &kept, false)?;
                MachineTranslation::Done
            }
            Ok(Translation::Unavailable) => MachineTranslation::Unavailable,
            Err(message) => MachineTranslation::Failed(message),
        },
    };
    Ok(LineTranslations {
        texts: found,
        machine,
    })
}

/// Remembers what the user typed over a printed or translated line, so the
/// same line on the next receipt reads the same right away (TRL-04).
pub fn remember_corrections(
    db: &Db,
    source: Option<&str>,
    target: &str,
    items: &[LineItem],
) -> Result<(), StorageError> {
    let pairs: Vec<(String, String)> = items
        .iter()
        .filter_map(|item| {
            let original = item.original_text.trim();
            let correction = item.user_text.as_deref()?.trim();
            (!original.is_empty() && !correction.is_empty() && correction != original)
                .then(|| (item.original_text.clone(), correction.to_string()))
        })
        .collect();
    if pairs.is_empty() {
        return Ok(());
    }
    db.remember_translations(source.unwrap_or(UNKNOWN_LANGUAGE), target, &pairs, true)
}

/// After saving a reviewed receipt: keeps the language it was read in
/// (idee.md 4.1 `Receipt.detected_language`) and the user's corrections.
pub fn remember_review(
    db: &Db,
    receipt_id: Option<&str>,
    source: Option<&str>,
    target: &str,
    items: &[LineItem],
) -> Result<(), StorageError> {
    if let (Some(id), Some(language)) = (receipt_id, source) {
        db.set_receipt_language(id, language)?;
    }
    remember_corrections(db, source, target, items)
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Mutex;

    use super::*;

    /// Upper-cases texts and records what it was asked.
    #[derive(Default)]
    struct FakeTranslator {
        asked: Mutex<Vec<Vec<String>>>,
        available: bool,
    }

    impl Translator for FakeTranslator {
        fn translate(
            &self,
            _source: &str,
            _target: &str,
            texts: Vec<String>,
        ) -> impl Future<Output = Result<Translation, String>> + Send + use<> {
            self.asked.lock().unwrap().push(texts.clone());
            let answer = if self.available {
                Translation::Done(
                    texts
                        .iter()
                        .map(|t| MachineText {
                            text: t.to_uppercase(),
                            // Pretends lines with a space are unsure.
                            remember: !t.contains(' '),
                        })
                        .collect(),
                )
            } else {
                Translation::Unavailable
            };
            std::future::ready(Ok(answer))
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn texts(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| t.to_string()).collect()
    }

    fn item(original: &str, user: Option<&str>) -> LineItem {
        LineItem {
            original_text: original.into(),
            user_text: user.map(Into::into),
            ..LineItem::default()
        }
    }

    #[test]
    fn engine_translates_once_then_the_cache_answers() {
        let db = Db::open_in_memory().unwrap();
        let engine = FakeTranslator {
            available: true,
            ..Default::default()
        };
        let lines = texts(&["beer", "rice", "beer", " "]);
        let first = block_on(translate_lines(&db, &engine, Some("en"), "de", &lines)).unwrap();
        assert_eq!(first.machine, MachineTranslation::Done);
        assert_eq!(first.texts["beer"], "BEER");
        assert_eq!(first.texts.len(), 2);
        let second = block_on(translate_lines(&db, &engine, Some("en"), "de", &lines)).unwrap();
        assert_eq!(second.texts, first.texts);
        // Duplicates and blanks were never sent; the second run sent nothing.
        assert_eq!(*engine.asked.lock().unwrap(), [texts(&["beer", "rice"])]);
    }

    #[test]
    fn unsure_translations_are_shown_but_not_remembered() {
        let db = Db::open_in_memory().unwrap();
        let engine = FakeTranslator {
            available: true,
            ..Default::default()
        };
        let lines = texts(&["beer", "rice ball"]);
        let first = block_on(translate_lines(&db, &engine, Some("en"), "de", &lines)).unwrap();
        assert_eq!(first.texts["rice ball"], "RICE BALL");
        let remembered = db
            .remembered_translations(Some("en"), "de", &lines)
            .unwrap();
        assert_eq!(remembered.len(), 1);
        assert!(remembered.contains_key("beer"));
        block_on(translate_lines(&db, &engine, Some("en"), "de", &lines)).unwrap();
        // The unsure line goes to the engine again.
        assert_eq!(
            engine.asked.lock().unwrap().last().unwrap(),
            &texts(&["rice ball"])
        );
    }

    #[test]
    fn confidence_setting_round_trips() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(
            TranslationConfidence::current(&db),
            TranslationConfidence::Balanced
        );
        TranslationConfidence::Strict.save(&db).unwrap();
        assert_eq!(
            TranslationConfidence::current(&db),
            TranslationConfidence::Strict
        );
        db.set_setting(TRANSLATION_CONFIDENCE, "nonsense").unwrap();
        assert_eq!(
            TranslationConfidence::current(&db),
            TranslationConfidence::Balanced
        );
    }

    #[test]
    fn no_engine_call_for_the_same_or_an_unknown_language() {
        let db = Db::open_in_memory().unwrap();
        let engine = FakeTranslator {
            available: true,
            ..Default::default()
        };
        let lines = texts(&["Milch"]);
        let same = block_on(translate_lines(&db, &engine, Some("de"), "de", &lines)).unwrap();
        assert_eq!(same.machine, MachineTranslation::SameLanguage);
        let unknown = block_on(translate_lines(&db, &engine, None, "de", &lines)).unwrap();
        assert_eq!(unknown.machine, MachineTranslation::UnknownLanguage);
        assert!(engine.asked.lock().unwrap().is_empty());
        assert!(same.texts.is_empty() && unknown.texts.is_empty());
    }

    #[test]
    fn corrections_come_back_on_the_next_receipt_without_an_engine() {
        let db = Db::open_in_memory().unwrap();
        let engine = FakeTranslator::default();
        let lines = texts(&["生ビール", "お通し"]);
        let before = block_on(translate_lines(&db, &engine, Some("ja"), "de", &lines)).unwrap();
        assert_eq!(before.machine, MachineTranslation::Unavailable);

        remember_corrections(
            &db,
            Some("ja"),
            "de",
            &[
                item("生ビール", Some(" Bier vom Fass ")),
                item("お通し", None),
                item("", Some("Manuell")),
                item("Cola", Some("Cola")),
            ],
        )
        .unwrap();

        let after = block_on(translate_lines(&db, &engine, Some("ja"), "de", &lines)).unwrap();
        assert_eq!(after.texts["生ビール"], "Bier vom Fass");
        assert_eq!(after.texts.len(), 1);
        assert_eq!(after.machine, MachineTranslation::Unavailable);
        // Only the line without a correction went to the engine.
        assert_eq!(
            engine.asked.lock().unwrap().last().unwrap(),
            &texts(&["お通し"])
        );
    }

    #[test]
    fn corrections_of_receipts_in_the_target_language_are_remembered_too() {
        let db = Db::open_in_memory().unwrap();
        remember_corrections(&db, None, "de", &[item("Mi1ch", Some("Milch"))]).unwrap();
        let engine = FakeTranslator::default();
        let lines = texts(&["Mi1ch"]);
        let found = block_on(translate_lines(&db, &engine, Some("de"), "de", &lines)).unwrap();
        assert_eq!(found.texts["Mi1ch"], "Milch");
        assert_eq!(found.machine, MachineTranslation::Done);
    }
}
