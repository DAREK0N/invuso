//! Machine translation on the device (TRL-01, decision 10.2; idee.md 2.2
//! `Translator`).
//!
//! First choice is the translation engine the device brings: Android 12+
//! has a public on-device API (`TranslationManager`), reachable only from
//! Java, so a thin bridge in `MainActivity.kt` calls it (AGENTS.md 5,
//! stage 4). Downloadable Opus-MT models follow in AP-21b as the fallback.

use std::future::Future;

/// How a translation request ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Translation {
    /// One entry per input text, in order; empty where the engine had no
    /// answer for that text.
    #[cfg_attr(
        all(not(target_os = "android"), not(test)),
        expect(dead_code, reason = "only the Android bridge translates")
    )]
    Done(Vec<String>),
    /// No engine for this language pair on this device; not an error.
    Unavailable,
}

/// Translates short texts (receipt lines) between two languages.
pub trait Translator {
    /// `source` and `target` are ISO 639-1 codes.
    fn translate(
        &self,
        source: &str,
        target: &str,
        texts: Vec<String>,
    ) -> impl Future<Output = Result<Translation, String>> + Send + use<Self>;
}

/// The device's own translation engine.
pub fn system_translator() -> impl Translator {
    #[cfg(target_os = "android")]
    {
        super::android::AndroidTranslator
    }
    #[cfg(not(target_os = "android"))]
    {
        NoTranslator
    }
}

/// Host builds (tests, CI) have no translation engine.
#[cfg(not(target_os = "android"))]
struct NoTranslator;

#[cfg(not(target_os = "android"))]
impl Translator for NoTranslator {
    fn translate(
        &self,
        _source: &str,
        _target: &str,
        _texts: Vec<String>,
    ) -> impl Future<Output = Result<Translation, String>> + Send + use<> {
        std::future::ready(Ok(Translation::Unavailable))
    }
}
