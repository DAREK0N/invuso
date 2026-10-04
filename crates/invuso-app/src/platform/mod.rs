//! Platform-specific pieces behind plain Rust functions (AGENTS.md 5).

use std::path::PathBuf;

#[cfg(target_os = "android")]
mod android;

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
