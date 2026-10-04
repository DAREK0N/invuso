//! Choices and defaults for the profile settings (SET-01, SET-02,
//! idee.md 7.5), for people (PER-01), payment methods (PAY-01) and groups
//! (GRP-01).

use invuso_core::domain::{Category, Currency, PaymentMethodKind};

/// Favorites of the currency picker until the user changes them.
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

/// Display name of a category: default categories store a key that is
/// translated, the user's own ones their name (EXP-09).
pub fn category_name(category: &Category) -> String {
    if !category.is_default {
        return category.name.clone();
    }
    match category.name.as_str() {
        "food" => t!("category.food").to_string(),
        "groceries" => t!("category.groceries").to_string(),
        "transport" => t!("category.transport").to_string(),
        "lodging" => t!("category.lodging").to_string(),
        "activities" => t!("category.activities").to_string(),
        "shopping" => t!("category.shopping").to_string(),
        "health" => t!("category.health").to_string(),
        "other" => t!("category.other").to_string(),
        other => other.to_string(),
    }
}

/// Icons a group can have, as keys stored in the database; drawn by
/// `components::GroupIconGlyph`.
pub const GROUP_ICONS: [&str; 12] = [
    "users",
    "plane",
    "luggage",
    "tree-palm",
    "mountain",
    "tent",
    "home",
    "utensils",
    "car",
    "briefcase",
    "party-popper",
    "heart",
];

/// Icon preselected for a new group.
pub const DEFAULT_GROUP_ICON: &str = "users";

/// Accessible name of a group icon in the current app language.
pub fn group_icon_name(icon: &str) -> String {
    match icon {
        "users" => t!("group_icon.users").to_string(),
        "plane" => t!("group_icon.plane").to_string(),
        "luggage" => t!("group_icon.luggage").to_string(),
        "tree-palm" => t!("group_icon.tree_palm").to_string(),
        "mountain" => t!("group_icon.mountain").to_string(),
        "tent" => t!("group_icon.tent").to_string(),
        "home" => t!("group_icon.home").to_string(),
        "utensils" => t!("group_icon.utensils").to_string(),
        "car" => t!("group_icon.car").to_string(),
        "briefcase" => t!("group_icon.briefcase").to_string(),
        "party-popper" => t!("group_icon.party_popper").to_string(),
        "heart" => t!("group_icon.heart").to_string(),
        other => other.to_string(),
    }
}

/// A stored `YYYY-MM-DD` date in the app language's usual form; anything
/// else is shown as stored.
pub fn display_date(iso: &str) -> String {
    match (iso.get(0..4), iso.get(5..7), iso.get(8..10)) {
        (Some(y), Some(m), Some(d)) if iso.len() == 10 => {
            t!("date.format", y = y, m = m, d = d).to_string()
        }
        _ => iso.to_string(),
    }
}

/// Heading of a day in the timeline (GRP-20): "Heute", "Gestern", else the
/// weekday and date, e.g. "Sa., 03.10.2026". `today` is `YYYY-MM-DD`.
pub fn day_heading(date: &str, today: &str) -> String {
    if date == today {
        return t!("date.today").to_string();
    }
    if crate::clock::previous_day(today).as_deref() == Some(date) {
        return t!("date.yesterday").to_string();
    }
    let weekday = match crate::clock::weekday(date) {
        Some(0) => t!("date.weekday_mon"),
        Some(1) => t!("date.weekday_tue"),
        Some(2) => t!("date.weekday_wed"),
        Some(3) => t!("date.weekday_thu"),
        Some(4) => t!("date.weekday_fri"),
        Some(5) => t!("date.weekday_sat"),
        Some(6) => t!("date.weekday_sun"),
        _ => return display_date(date),
    };
    t!(
        "date.day_heading",
        weekday = weekday,
        date = display_date(date)
    )
    .to_string()
}

/// "01.03.2026 – 14.03.2026", "ab …", "bis …"; `None` without any date.
pub fn period_text(start: Option<&str>, end: Option<&str>) -> Option<String> {
    match (start, end) {
        (Some(start), Some(end)) => Some(
            t!(
                "group.period_range",
                start = display_date(start),
                end = display_date(end)
            )
            .to_string(),
        ),
        (Some(start), None) => {
            Some(t!("group.period_from", start = display_date(start)).to_string())
        }
        (None, Some(end)) => Some(t!("group.period_until", end = display_date(end)).to_string()),
        (None, None) => None,
    }
}

/// Suggested home currency during onboarding.
pub fn default_home_currency() -> Currency {
    Currency::from_code("EUR").expect("EUR is a valid ISO 4217 currency")
}

/// The default favorites as currencies.
pub fn default_favorite_currencies() -> Vec<Currency> {
    FAVORITE_CURRENCIES
        .iter()
        .filter_map(|code| Currency::from_code(code).ok())
        .collect()
}

/// How many recently picked currencies the picker remembers.
pub const RECENT_CURRENCY_LIMIT: usize = 5;

/// `recent` with `picked` moved to the front, without duplicates and cut
/// to [`RECENT_CURRENCY_LIMIT`].
pub fn with_recent(recent: &[Currency], picked: Currency) -> Vec<Currency> {
    std::iter::once(picked)
        .chain(recent.iter().copied().filter(|&c| c != picked))
        .take(RECENT_CURRENCY_LIMIT)
        .collect()
}

/// Adds `currency` to the end of the favorites, or removes it if it is one.
pub fn toggle_favorite(favorites: &[Currency], currency: Currency) -> Vec<Currency> {
    if favorites.contains(&currency) {
        favorites
            .iter()
            .copied()
            .filter(|&c| c != currency)
            .collect()
    } else {
        favorites.iter().copied().chain([currency]).collect()
    }
}

/// Preselection of the converter without a saved one: the first favorite
/// that is not the home currency, converted into the home currency.
pub fn default_converter_pair(home: Currency, favorites: &[Currency]) -> (Currency, Currency) {
    let from = favorites
        .iter()
        .chain(default_favorite_currencies().iter())
        .copied()
        .find(|&c| c != home)
        .unwrap_or(home);
    (from, home)
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
    fn default_group_icon_is_offered() {
        assert!(GROUP_ICONS.contains(&DEFAULT_GROUP_ICON));
    }

    #[test]
    fn formats_dates_and_periods() {
        rust_i18n::set_locale("de");
        assert_eq!(display_date("2026-03-01"), "01.03.2026");
        assert_eq!(display_date("garbage"), "garbage");
        assert_eq!(
            period_text(Some("2026-03-01"), Some("2026-03-14")).as_deref(),
            Some("01.03.2026 – 14.03.2026")
        );
        assert_eq!(
            period_text(None, Some("2026-03-14")).as_deref(),
            Some("bis 14.03.2026")
        );
        assert_eq!(period_text(None, None), None);
    }

    #[test]
    fn favorites_are_valid_currencies() {
        assert_eq!(
            default_favorite_currencies().len(),
            FAVORITE_CURRENCIES.len()
        );
        assert_eq!(default_home_currency().code(), "EUR");
    }

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    #[test]
    fn recent_currencies_move_to_front_and_stay_short() {
        let recent = with_recent(&[], cur("JPY"));
        assert_eq!(recent, vec![cur("JPY")]);
        let recent = with_recent(&[cur("EUR"), cur("JPY"), cur("USD")], cur("JPY"));
        assert_eq!(recent, vec![cur("JPY"), cur("EUR"), cur("USD")]);
        let full = ["EUR", "USD", "GBP", "CHF", "THB"].map(cur);
        let recent = with_recent(&full, cur("JPY"));
        assert_eq!(recent.len(), RECENT_CURRENCY_LIMIT);
        assert_eq!(recent[0], cur("JPY"));
        assert!(!recent.contains(&cur("THB")));
    }

    #[test]
    fn toggles_favorites() {
        let favorites = toggle_favorite(&[cur("EUR")], cur("JPY"));
        assert_eq!(favorites, vec![cur("EUR"), cur("JPY")]);
        assert_eq!(toggle_favorite(&favorites, cur("EUR")), vec![cur("JPY")]);
    }

    #[test]
    fn converter_starts_from_a_foreign_favorite() {
        let favorites = [cur("EUR"), cur("JPY")];
        assert_eq!(
            default_converter_pair(cur("EUR"), &favorites),
            (cur("JPY"), cur("EUR"))
        );
        assert_eq!(
            default_converter_pair(cur("JPY"), &favorites),
            (cur("EUR"), cur("JPY"))
        );
        assert_eq!(
            default_converter_pair(cur("EUR"), &[]),
            (cur("USD"), cur("EUR"))
        );
    }
}
