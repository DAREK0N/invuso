//! Saving a file where the user wants it and opening one the user picks,
//! e.g. a backup or a CSV export (DATA-01..03).
//!
//! Dioxus 0.7 answers `<input type="file">` on Android with an empty file
//! list, the WebView ignores `<a download>`, and sharing a file needs a
//! FileProvider that `dx` cannot declare. So the system's "save as" and
//! "open" dialogs (Storage Access Framework) are opened by a thin bridge in
//! `MainActivity.kt` (AGENTS.md 5, stage 4); it only copies bytes between
//! the chosen document and the path given here.

use std::future::Future;
use std::path::Path;

/// How a save or open dialog ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentOutcome {
    /// The file was copied to or from the chosen document.
    Done,
    /// The user backed out of the dialog.
    #[cfg_attr(
        not(target_os = "android"),
        expect(
            dead_code,
            reason = "only the Android bridge has a dialog to back out of"
        )
    )]
    Cancelled,
}

/// The platform's dialogs for saving and opening documents.
pub trait DocumentFiles {
    /// Whether the dialogs work on this platform.
    fn supported(&self) -> bool;

    /// Lets the user choose where to save a copy of `source`; `name` is the
    /// suggested file name, `mime` its media type. Only one dialog runs at
    /// a time; starting another cancels the first.
    fn save(
        &self,
        source: &Path,
        name: &str,
        mime: &str,
    ) -> impl Future<Output = Result<DocumentOutcome, String>> + use<Self>;

    /// Lets the user pick a document of one of the `mimes` types and copies
    /// it to `dest`.
    fn open(
        &self,
        dest: &Path,
        mimes: &[&str],
    ) -> impl Future<Output = Result<DocumentOutcome, String>> + use<Self>;
}

/// The document dialogs of this platform.
pub fn document_files() -> impl DocumentFiles {
    #[cfg(target_os = "android")]
    {
        super::android::AndroidDocumentFiles
    }
    #[cfg(not(target_os = "android"))]
    {
        NoDocumentFiles
    }
}

/// Host builds (tests, CI) have no file dialogs.
#[cfg(not(target_os = "android"))]
struct NoDocumentFiles;

#[cfg(not(target_os = "android"))]
impl DocumentFiles for NoDocumentFiles {
    fn supported(&self) -> bool {
        false
    }

    fn save(
        &self,
        _source: &Path,
        _name: &str,
        _mime: &str,
    ) -> impl Future<Output = Result<DocumentOutcome, String>> + use<> {
        std::future::ready(Err("unsupported platform: no file dialog".to_string()))
    }

    fn open(
        &self,
        _dest: &Path,
        _mimes: &[&str],
    ) -> impl Future<Output = Result<DocumentOutcome, String>> + use<> {
        std::future::ready(Err("unsupported platform: no file dialog".to_string()))
    }
}
