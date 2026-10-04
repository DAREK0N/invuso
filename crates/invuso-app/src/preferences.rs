//! Choices and defaults for the profile settings (SET-01, SET-02,
//! idee.md 7.5), for people (PER-01) and for payment methods (PAY-01).

use invuso_core::domain::{Currency, PaymentMethodKind};

/// Shown first in the currency picker, in this order.
pub const FAVORITE_CURRENCIES: [&str; 5] = ["EUR", "USD", "JPY", "CHF", "GBP"];

/// Target languages for receipt translations, as ISO 639-1 codes.
pub const TARGET_LANGUAGES: [&str; 6] = ["de", "en", "ja", "fr", "es", "it"];

/// Suggested when the system language is not a supported target language.
const FALLBACK_TARGET_LANGUAGE: &str = "de";

/// Colors a person can have, as design-token scale names (idee.md 3.2).
/// The positive/negative scales are left out so an avatar never reads as
/// a balance.
pub const PERSON_COLORS: [&str; 7] = [
    "cerulean",
    "muted-teal",
    "pale-oak",
    "thistle",
    "dusty-grape",
    "ash-grey",
    "slate-grey",
];

/// Preselected color for a new person: the one fewest people have, ties
/// broken by palette order, so a group of people starts out distinguishable.
pub fn suggested_person_color<'a>(used: impl IntoIterator<Item = &'a str>) -> &'static str {
    let mut counts = [0_usize; PERSON_COLORS.len()];
    for color in used {
        if let Some(index) = PERSON_COLORS.iter().position(|&c| c == color) {
            counts[index] += 1;
        }
    }
    let (index, _) = counts
        .iter()
        .enumerate()
        .min_by_key(|&(index, count)| (*count, index))
        .unwrap_or((0, &0));
    PERSON_COLORS[index]
}

/// Display name of a person color in the current app language.
pub fn color_name(color: &str) -> String {
    match color {
        "cerulean" => t!("color.cerulean").to_string(),
        "muted-teal" => t!("color.muted_teal").to_string(),
        "pale-oak" => t!("color.pale_oak").to_string(),
        "thistle" => t!("color.thistle").to_string(),
        "dusty-grape" => t!("color.dusty_grape").to_string(),
        "ash-grey" => t!("color.ash_grey").to_string(),
        "slate-grey" => t!("color.slate_grey").to_string(),
        other => other.to_string(),
    }
}

/// Icons a payment method can have, as keys stored in the database; drawn
/// by `components::PaymentIconGlyph`.
pub const PAYMENT_ICONS: [&str; 8] = [
    "banknote",
    "credit-card",
    "wallet",
    "smartphone",
    "landmark",
    "train-front",
    "coins",
    "piggy-bank",
];

/// Icon preselected for a kind; the user can pick another one.
pub fn default_payment_icon(kind: PaymentMethodKind) -> &'static str {
    match kind {
        PaymentMethodKind::Cash => "banknote",
        PaymentMethodKind::CreditCard | PaymentMethodKind::DebitCard => "credit-card",
        PaymentMethodKind::PayPal => "wallet",
        PaymentMethodKind::BankTransfer => "landmark",
        PaymentMethodKind::IcCard => "train-front",
        PaymentMethodKind::Other => "coins",
    }
}

/// Accessible name of a payment icon in the current app language.
pub fn payment_icon_name(icon: &str) -> String {
    match icon {
        "banknote" => t!("payment_icon.banknote").to_string(),
        "credit-card" => t!("payment_icon.credit_card").to_string(),
        "wallet" => t!("payment_icon.wallet").to_string(),
        "smartphone" => t!("payment_icon.smartphone").to_string(),
        "landmark" => t!("payment_icon.landmark").to_string(),
        "train-front" => t!("payment_icon.train").to_string(),
        "coins" => t!("payment_icon.coins").to_string(),
        "piggy-bank" => t!("payment_icon.piggy_bank").to_string(),
        other => other.to_string(),
    }
}

/// Display name of a payment method kind in the current app language.
pub fn payment_kind_name(kind: PaymentMethodKind) -> String {
    match kind {
        PaymentMethodKind::Cash => t!("payment_kind.cash").to_string(),
        PaymentMethodKind::CreditCard => t!("payment_kind.credit_card").to_string(),
        PaymentMethodKind::DebitCard => t!("payment_kind.debit_card").to_string(),
        PaymentMethodKind::PayPal => t!("payment_kind.paypal").to_string(),
        PaymentMethodKind::BankTransfer => t!("payment_kind.bank_transfer").to_string(),
        PaymentMethodKind::IcCard => t!("payment_kind.ic_card").to_string(),
        PaymentMethodKind::Other => t!("payment_kind.other").to_string(),
    }
}

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
    fn suggests_least_used_person_color() {
        assert_eq!(suggested_person_color([]), "cerulean");
        assert_eq!(suggested_person_color(["cerulean"]), "muted-teal");
        assert_eq!(
            suggested_person_color(["cerulean", "pale-oak", "unknown"]),
            "muted-teal"
        );
        let all_once = PERSON_COLORS;
        assert_eq!(suggested_person_color(all_once), "cerulean");
        let mut all_but_last_twice = PERSON_COLORS.to_vec();
        all_but_last_twice.extend(&PERSON_COLORS[..PERSON_COLORS.len() - 1]);
        assert_eq!(suggested_person_color(all_but_last_twice), "slate-grey");
    }

    #[test]
    fn every_kind_suggests_a_known_icon() {
        for kind in PaymentMethodKind::ALL {
            assert!(PAYMENT_ICONS.contains(&default_payment_icon(kind)));
        }
    }

    #[test]
    fn favorites_are_valid_currencies() {
        assert_eq!(favorite_currencies().len(), FAVORITE_CURRENCIES.len());
        assert_eq!(default_home_currency().code(), "EUR");
    }
}
