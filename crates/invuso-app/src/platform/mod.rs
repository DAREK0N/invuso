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
    #[cfg(not(target_os = "android"))]
    {
        desktop_data_dir()
    }
}

/// Desktop is a development target only; mirrors the usual per-user
/// application data locations.
#[cfg(not(target_os = "android"))]
fn desktop_data_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .ok_or_else(|| "no user data directory found".to_string())?;
    Ok(base.join("invuso"))
}
