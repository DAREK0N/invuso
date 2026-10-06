//! Look and language of the app (UI-18, SET-04, SET-05, CORE-08).
//!
//! The look is stored as settings and shown through attributes on `<html>`
//! that `tailwind.css` reads: `data-theme`, `data-ui-radius`,
//! `data-ui-scale` and `data-ui-shadows`. The app language picks the
//! `rust-i18n` locale; switching it rebuilds the router (see [`UiLanguage`]).

use dioxus::logger::tracing::warn;
use dioxus::prelude::*;

use crate::Route;
use crate::platform::{self, SystemBars};
use crate::storage::{
    APP_LANGUAGE, CORNER_RADIUS, Db, SURFACE_SHADOWS, StorageError, THEME, UI_SCALE,
};

/// Color theme (UI-18). Dark is the original look; the others override its
/// design tokens in `tailwind.css`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Light or Dark, following the device's dark mode.
    System,
    #[default]
    Dark,
    /// Deeper blue-black with a muted accent.
    Night,
    /// True black page for OLED screens.
    Oled,
    Light,
}

impl Theme {
    pub const ALL: [Theme; 5] = [
        Theme::System,
        Theme::Dark,
        Theme::Night,
        Theme::Oled,
        Theme::Light,
    ];

    /// Stored value and `data-theme` attribute.
    pub fn code(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Dark => "dark",
            Theme::Night => "night",
            Theme::Oled => "oled",
            Theme::Light => "light",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.code() == code)
    }

    pub fn label(self) -> String {
        match self {
            Theme::System => t!("appearance.theme_system"),
            Theme::Dark => t!("appearance.theme_dark"),
            Theme::Night => t!("appearance.theme_night"),
            Theme::Oled => t!("appearance.theme_oled"),
            Theme::Light => t!("appearance.theme_light"),
        }
        .to_string()
    }
}

/// Step of the corner radius and of the UI scale (SET-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Size {
    Small,
    #[default]
    Normal,
    Large,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::Small, Size::Normal, Size::Large];

    /// Stored value and value of the `data-ui-*` attributes.
    pub fn code(self) -> &'static str {
        match self {
            Size::Small => "small",
            Size::Normal => "normal",
            Size::Large => "large",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|size| size.code() == code)
    }

    pub fn label(self) -> String {
        match self {
            Size::Small => t!("appearance.size_small"),
            Size::Normal => t!("appearance.size_normal"),
            Size::Large => t!("appearance.size_large"),
        }
        .to_string()
    }
}

/// Everything the appearance screen sets (UI-18, SET-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Appearance {
    pub theme: Theme,
    pub radius: Size,
    pub scale: Size,
    /// Soft shadow under cards and floating bars; off keeps the flat,
    /// bordered look.
    pub shadows: bool,
}

impl Appearance {
    /// The stored look; defaults for anything unset or unreadable, so a
    /// broken setting never keeps the app from starting.
    pub fn load(db: &Db) -> Self {
        let stored = |key| db.setting(key).ok().flatten();
        Self {
            theme: stored(THEME)
                .and_then(|code| Theme::from_code(&code))
                .unwrap_or_default(),
            radius: stored(CORNER_RADIUS)
                .and_then(|code| Size::from_code(&code))
                .unwrap_or_default(),
            scale: stored(UI_SCALE)
                .and_then(|code| Size::from_code(&code))
                .unwrap_or_default(),
            shadows: stored(SURFACE_SHADOWS).as_deref() == Some("on"),
        }
    }

    pub fn save(&self, db: &Db) -> Result<(), StorageError> {
        db.set_setting(THEME, self.theme.code())?;
        db.set_setting(CORNER_RADIUS, self.radius.code())?;
        db.set_setting(UI_SCALE, self.scale.code())?;
        db.set_setting(SURFACE_SHADOWS, if self.shadows { "on" } else { "off" })
    }

    /// Script that sets the attributes on `<html>`. Every value comes from
    /// a fixed code, never from user text.
    fn script(&self, locale: &str) -> String {
        format!(
            "var r = document.documentElement; \
             r.setAttribute('data-theme', '{}'); \
             r.setAttribute('data-ui-radius', '{}'); \
             r.setAttribute('data-ui-scale', '{}'); \
             r.setAttribute('data-ui-shadows', '{}'); \
             r.setAttribute('lang', '{}'); \
             return true;",
            self.theme.code(),
            self.radius.code(),
            self.scale.code(),
            self.shadows,
            locale,
        )
    }
}

/// Shows `appearance` and the app language on the page and in the system
/// bars. Call from an effect, after the first render.
pub fn apply(appearance: Appearance, locale: &'static str) {
    // AGENTS.md 5, stage 3: Dioxus renders inside <body> and has no API for
    // attributes of the root <html> element, which the design tokens and
    // the rem size (UI scale) hang off.
    let script = appearance.script(locale);
    spawn(async move {
        if let Err(error) = document::eval(&script).await {
            warn!("applying the appearance failed: {error}");
        }
    });
    if let Err(error) = platform::system_bars().set_theme(appearance.theme.code()) {
        warn!("setting the system bar colors failed: {error}");
    }
}

/// Language of the app's own texts (SET-04).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppLanguage {
    /// German on a German device, English everywhere else.
    #[default]
    System,
    German,
    English,
}

impl AppLanguage {
    pub const ALL: [AppLanguage; 3] = [
        AppLanguage::System,
        AppLanguage::German,
        AppLanguage::English,
    ];

    pub fn code(self) -> &'static str {
        match self {
            AppLanguage::System => "system",
            AppLanguage::German => "de",
            AppLanguage::English => "en",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.code() == code)
    }

    /// Each language in its own name, so it is found whatever is set now.
    pub fn label(self) -> String {
        match self {
            AppLanguage::System => t!("app_language.system"),
            AppLanguage::German => t!("app_language.de"),
            AppLanguage::English => t!("app_language.en"),
        }
        .to_string()
    }

    /// The stored choice; System if unset or unreadable.
    pub fn load(db: &Db) -> Self {
        db.setting(APP_LANGUAGE)
            .ok()
            .flatten()
            .and_then(|code| Self::from_code(&code))
            .unwrap_or_default()
    }

    pub fn save(self, db: &Db) -> Result<(), StorageError> {
        db.set_setting(APP_LANGUAGE, self.code())
    }

    /// The `rust-i18n` locale. `system_locale` is a BCP 47 (`de-AT`) or
    /// POSIX (`de_AT`) string; English is the fallback, as in `locales/`.
    pub fn locale(self, system_locale: Option<&str>) -> &'static str {
        match self {
            AppLanguage::German => "de",
            AppLanguage::English => "en",
            AppLanguage::System => {
                let language = system_locale
                    .and_then(|locale| locale.split(['-', '_']).next())
                    .map(str::to_ascii_lowercase);
                if language.as_deref() == Some("de") {
                    "de"
                } else {
                    "en"
                }
            }
        }
    }
}

/// The locale of the stored app language on this device.
pub fn startup_locale(db: &Db) -> &'static str {
    AppLanguage::load(db).locale(platform::system_locale().as_deref())
}

/// Current app locale, shared above the router. `t!` is not reactive, so a
/// new locale rebuilds the router; it then opens [`UiLanguage::restart_route`]
/// instead of the start screen.
#[derive(Clone, Copy, PartialEq)]
pub struct UiLanguage {
    locale: Signal<&'static str>,
    restart_at: Signal<Option<Route>>,
}

impl UiLanguage {
    /// Must be called inside a component, like any `Signal::new`.
    pub fn new(locale: &'static str) -> Self {
        rust_i18n::set_locale(locale);
        Self {
            locale: Signal::new(locale),
            restart_at: Signal::new(None),
        }
    }

    /// Subscribes the caller to language changes.
    pub fn locale(&self) -> &'static str {
        *self.locale.read()
    }

    /// Screen to reopen after a rebuild; `None` before the first switch.
    pub fn restart_route(&self) -> Option<Route> {
        self.restart_at.peek().clone()
    }

    /// Stores `language`; if its locale differs, switches the texts and
    /// rebuilds the screens at `current`.
    pub fn switch(
        &mut self,
        db: &Db,
        language: AppLanguage,
        current: Route,
    ) -> Result<(), StorageError> {
        language.save(db)?;
        let locale = language.locale(platform::system_locale().as_deref());
        if locale != *self.locale.peek() {
            rust_i18n::set_locale(locale);
            self.restart_at.set(Some(current));
            self.locale.set(locale);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for theme in Theme::ALL {
            assert_eq!(Theme::from_code(theme.code()), Some(theme));
        }
        for size in Size::ALL {
            assert_eq!(Size::from_code(size.code()), Some(size));
        }
        for language in AppLanguage::ALL {
            assert_eq!(AppLanguage::from_code(language.code()), Some(language));
        }
        assert_eq!(Theme::from_code("sepia"), None);
    }

    #[test]
    fn appearance_starts_dark_and_round_trips() {
        let db = Db::open_in_memory().unwrap();
        let fresh = Appearance::load(&db);
        assert_eq!(fresh.theme, Theme::Dark);
        assert_eq!(fresh.radius, Size::Normal);
        assert_eq!(fresh.scale, Size::Normal);
        assert!(!fresh.shadows);

        let chosen = Appearance {
            theme: Theme::Light,
            radius: Size::Small,
            scale: Size::Large,
            shadows: true,
        };
        chosen.save(&db).unwrap();
        assert_eq!(Appearance::load(&db), chosen);
    }

    #[test]
    fn unreadable_appearance_falls_back_to_defaults() {
        let db = Db::open_in_memory().unwrap();
        db.set_setting(THEME, "sepia").unwrap();
        db.set_setting(UI_SCALE, "huge").unwrap();
        assert_eq!(Appearance::load(&db), Appearance::default());
    }

    #[test]
    fn script_sets_every_attribute() {
        let script = Appearance {
            theme: Theme::Oled,
            radius: Size::Large,
            scale: Size::Small,
            shadows: true,
        }
        .script("en");
        for part in [
            "'data-theme', 'oled'",
            "'data-ui-radius', 'large'",
            "'data-ui-scale', 'small'",
            "'data-ui-shadows', 'true'",
            "'lang', 'en'",
        ] {
            assert!(script.contains(part), "{part} missing in {script}");
        }
    }

    #[test]
    fn system_language_is_german_only_on_german_devices() {
        let system = AppLanguage::System;
        assert_eq!(system.locale(Some("de-DE")), "de");
        assert_eq!(system.locale(Some("de_AT")), "de");
        assert_eq!(system.locale(Some("DE")), "de");
        assert_eq!(system.locale(Some("en-US")), "en");
        assert_eq!(system.locale(Some("ja-JP")), "en");
        assert_eq!(system.locale(None), "en");
        assert_eq!(AppLanguage::German.locale(Some("en-US")), "de");
        assert_eq!(AppLanguage::English.locale(Some("de-DE")), "en");
    }

    /// Keys of a `locales/*.yml` file as dotted paths, e.g. `nav.home`.
    fn locale_keys(yaml: &str) -> std::collections::BTreeSet<String> {
        let mut path: Vec<(usize, String)> = Vec::new();
        let mut keys = std::collections::BTreeSet::new();
        for line in yaml.lines() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = trimmed.split_once(':') else {
                continue;
            };
            let indent = line.len() - trimmed.len();
            path.retain(|(level, _)| *level < indent);
            path.push((indent, key.to_string()));
            if !value.trim().is_empty() {
                let dotted: Vec<&str> = path.iter().map(|(_, k)| k.as_str()).collect();
                keys.insert(dotted.join("."));
            }
        }
        keys
    }

    #[test]
    fn every_text_exists_in_german_and_english() {
        let de = locale_keys(include_str!("../locales/de.yml"));
        let en = locale_keys(include_str!("../locales/en.yml"));
        assert!(de.len() > 100);
        assert_eq!(
            de.difference(&en).collect::<Vec<_>>(),
            Vec::<&String>::new()
        );
        assert_eq!(
            en.difference(&de).collect::<Vec<_>>(),
            Vec::<&String>::new()
        );
    }

    #[test]
    fn app_language_is_stored() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(AppLanguage::load(&db), AppLanguage::System);
        AppLanguage::English.save(&db).unwrap();
        assert_eq!(AppLanguage::load(&db), AppLanguage::English);
    }
}
