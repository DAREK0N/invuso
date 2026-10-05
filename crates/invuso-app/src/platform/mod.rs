//! Platform-specific pieces behind plain Rust functions (AGENTS.md 5).

use std::path::PathBuf;

#[cfg(target_os = "android")]
mod android;
mod images;

pub use images::{ImageKind, ImageSource, PickOutcome, image_source};

/// Private, persistent directory for the database and receipt images.
pub fn data_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "android")]
    {
        android::files_dir()
    }
    // Android is the only app target; host builds exist for tests and CI
    // only, and the web version will bring its own storage.
    #[cfg(not(target_os = "android"))]
    {
        Err("unsupported platform: no app data directory".to_string())
    }
}

/// The device language as a BCP 47 tag, e.g. `"de-DE"`, if known.
///
/// `sys-locale` reads it in plain Rust (on Android from the
/// `persist.sys.locale` system property), so no JNI is needed.
pub fn system_locale() -> Option<String> {
    sys_locale::get_locale()
}
