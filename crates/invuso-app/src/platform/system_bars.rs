//! Icon color of the status and navigation bar (UI-18).
//!
//! The app draws behind transparent system bars; with a light theme their
//! icons must turn dark. Only the Activity can change that, and only it
//! knows the device's dark mode for the "system" theme, so a thin bridge in
//! `MainActivity.kt` does it (AGENTS.md 5, stage 4).

/// Adapts the system bars to the app theme.
pub trait SystemBars {
    /// `theme` is the stored theme code, e.g. `light` or `system`.
    fn set_theme(&self, theme: &str) -> Result<(), String>;
}

/// The system bars of this platform.
pub fn system_bars() -> impl SystemBars {
    #[cfg(target_os = "android")]
    {
        super::android::AndroidSystemBars
    }
    #[cfg(not(target_os = "android"))]
    {
        NoSystemBars
    }
}

/// Host builds (tests, CI) have no system bars.
#[cfg(not(target_os = "android"))]
struct NoSystemBars;

#[cfg(not(target_os = "android"))]
impl SystemBars for NoSystemBars {
    fn set_theme(&self, _theme: &str) -> Result<(), String> {
        Ok(())
    }
}
