//! Taking or choosing receipt images (RCP-01, RCP-02; idee.md 2.2
//! `ImageSource`).
//!
//! Dioxus 0.7 answers `<input type="file">` on Android with an empty file
//! list, and the WebView's own camera capture needs a FileProvider that `dx`
//! cannot declare. So the system camera and photo picker are opened by a
//! thin bridge in `MainActivity.kt` (AGENTS.md 5, stage 4); it only copies
//! the image to the path given here.

use std::future::Future;
use std::path::Path;

/// Where a receipt image comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    /// The system camera app (RCP-01).
    Camera,
    /// The system photo picker (RCP-02).
    Gallery,
}

/// How a pick ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickOutcome {
    /// The image was copied to the requested path.
    #[cfg_attr(
        not(target_os = "android"),
        expect(dead_code, reason = "only the Android bridge saves images")
    )]
    Saved,
    /// The user backed out without an image.
    Cancelled,
}

/// Lets the user take or choose one image.
pub trait ImageSource {
    /// Whether `kind` works on this device.
    fn supports(&self, kind: ImageKind) -> bool;

    /// Opens the camera or picker and copies the image to `dest`. Only one
    /// pick runs at a time; starting another cancels the first.
    fn pick(
        &self,
        kind: ImageKind,
        dest: &Path,
    ) -> impl Future<Output = Result<PickOutcome, String>> + use<Self>;
}

/// The image source of this platform.
pub fn image_source() -> impl ImageSource {
    #[cfg(target_os = "android")]
    {
        super::android::AndroidImageSource
    }
    #[cfg(not(target_os = "android"))]
    {
        NoImageSource
    }
}

/// Host builds (tests, CI) have neither camera nor picker.
#[cfg(not(target_os = "android"))]
struct NoImageSource;

#[cfg(not(target_os = "android"))]
impl ImageSource for NoImageSource {
    fn supports(&self, _kind: ImageKind) -> bool {
        false
    }

    fn pick(
        &self,
        _kind: ImageKind,
        _dest: &Path,
    ) -> impl Future<Output = Result<PickOutcome, String>> + use<> {
        std::future::ready(Err("unsupported platform: no camera or picker".to_string()))
    }
}
