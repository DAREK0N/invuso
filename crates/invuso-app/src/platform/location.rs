//! The device's location for an expense, only when the user asks for it
//! (EXP-10).
//!
//! Dioxus has no location API. The WebView's `navigator.geolocation` works
//! on Android and asks for the permission itself (wry's
//! `onGeolocationPermissionsShowPrompt` requests it at runtime), so a few
//! lines of JavaScript suffice (AGENTS.md 5, stage 3) and no Kotlin is
//! needed. Nothing is sent anywhere; the position stays on the device.

use dioxus::prelude::document;
use invuso_core::domain::GeoPoint;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LocationError {
    #[error("location permission denied")]
    Denied,
    #[error("location unavailable")]
    Unavailable,
    #[error("location request timed out")]
    Timeout,
    #[error("{0}")]
    Failed(String),
}

// Stage 3 (AGENTS.md 5): answers `{lat, lon}` or `{error}`; never rejects.
// A position up to a minute old is fine for where an expense happened.
const SCRIPT: &str = r#"
return await new Promise((resolve) => {
    if (!navigator.geolocation) {
        resolve({ error: "unavailable" });
        return;
    }
    navigator.geolocation.getCurrentPosition(
        (p) => resolve({ lat: p.coords.latitude, lon: p.coords.longitude }),
        (e) => resolve({ error: e.code === 1 ? "denied" : e.code === 3 ? "timeout" : "unavailable" }),
        { enableHighAccuracy: true, timeout: 20000, maximumAge: 60000 }
    );
});
"#;

/// Asks the device where it is; Android shows its permission dialog the
/// first time. Must run inside the Dioxus runtime (e.g. a spawned task).
pub async fn current_position() -> Result<GeoPoint, LocationError> {
    let value = document::eval(SCRIPT)
        .await
        .map_err(|e| LocationError::Failed(e.to_string()))?;
    let number = |key: &str| value.get(key).and_then(serde_json::Value::as_f64);
    match (number("lat"), number("lon")) {
        (Some(lat), Some(lon)) => GeoPoint::new(lat, lon).map_err(|_| LocationError::Unavailable),
        _ => Err(
            match value.get("error").and_then(serde_json::Value::as_str) {
                Some("denied") => LocationError::Denied,
                Some("timeout") => LocationError::Timeout,
                _ => LocationError::Unavailable,
            },
        ),
    }
}
