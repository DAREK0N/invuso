//! Handing text to other apps, e.g. the settlement to a messenger (SPL-09).
//!
//! The Android WebView does not implement `navigator.share`, so the system
//! share sheet is opened by a thin bridge in `MainActivity.kt` (AGENTS.md 5,
//! stage 4).

/// Shares plain text through the platform's share dialog.
pub trait TextShare {
    /// Whether sharing works on this platform.
    fn supported(&self) -> bool;

    /// Opens the share dialog with `text`; returns once it is opened.
    fn share(&self, text: &str) -> Result<(), String>;
}

/// The share dialog of this platform.
pub fn text_share() -> impl TextShare {
    #[cfg(target_os = "android")]
    {
        super::android::AndroidTextShare
    }
    #[cfg(not(target_os = "android"))]
    {
        NoTextShare
    }
}

/// Host builds (tests, CI) cannot share.
#[cfg(not(target_os = "android"))]
struct NoTextShare;

#[cfg(not(target_os = "android"))]
impl TextShare for NoTextShare {
    fn supported(&self) -> bool {
        false
    }

    fn share(&self, _text: &str) -> Result<(), String> {
        Err("unsupported platform: no share dialog".to_string())
    }
}
