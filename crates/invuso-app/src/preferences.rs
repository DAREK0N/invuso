//! Choices and defaults for the profile settings (SET-01, SET-02,
//! idee.md 7.5).

use invuso_core::domain::Currency;

/// Shown first in the currency picker, in this order.
pub const FAVORITE_CURRENCIES: [&str; 5] = ["EUR", "USD", "JPY", "CHF", "GBP"];

/// Target languages for receipt translations, as ISO 639-1 codes.
pub const TARGET_LANGUAGES: [&str; 6] = ["de", "en", "ja", "fr", "es", "it"];

/// Suggested when the system language is not a supported target language.
const FALLBACK_TARGET_LANGUAGE: &str = "de";

/// Suggested home currency during onboarding.
pub fn default_home_currency() -> Currency {
    Currency::from_code("EUR").expect("EUR is a valid ISO 4217 currency")
}

/// Favorites as currencies, for the picker.
pub fn favorite_currencies() -> Vec<Currency> {
    FAVORITE_CURRENCIES
        .iter()
        .filter_map(|code| Currency::from_code(code).ok())
        .collect()
}

/// Onboarding suggestion: the system language if it is a supported target
/// language, otherwise German. Accepts BCP 47 (`de-AT`) and POSIX (`de_AT`)
/// locale strings.
pub fn suggested_target_language(system_locale: Option<&str>) -> &'static str {
    system_locale
        .and_then(|locale| locale.split(['-', '_']).next())
        .map(str::to_ascii_lowercase)
        .and_then(|code| TARGET_LANGUAGES.into_iter().find(|&l| l == code))
        .unwrap_or(FALLBACK_TARGET_LANGUAGE)
}

/// Display name of a target language in the current app language.
pub fn language_name(code: &str) -> String {
    match code {
        "de" => t!("language.de").to_string(),
        "en" => t!("language.en").to_string(),
        "ja" => t!("language.ja").to_string(),
        "fr" => t!("language.fr").to_string(),
        "es" => t!("language.es").to_string(),
        "it" => t!("language.it").to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_supported_system_language() {
        assert_eq!(suggested_target_language(Some("ja-JP")), "ja");
        assert_eq!(suggested_target_language(Some("en_US")), "en");
        assert_eq!(suggested_target_language(Some("FR")), "fr");
    }

    #[test]
    fn falls_back_to_german() {
        assert_eq!(suggested_target_language(Some("zh-Hans-CN")), "de");
        assert_eq!(suggested_target_language(Some("")), "de");
        assert_eq!(suggested_target_language(None), "de");
    }

    #[test]
    fn favorites_are_valid_currencies() {
        assert_eq!(favorite_currencies().len(), FAVORITE_CURRENCIES.len());
        assert_eq!(default_home_currency().code(), "EUR");
    }
}
