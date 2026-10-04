use dioxus::prelude::*;

use super::PlaceholderPage;

mod profile;

pub use profile::Settings;

#[component]
pub fn SettingsPeople() -> Element {
    rsx! { PlaceholderPage { title: t!("page.settings_people").to_string(), show_back: true } }
}

#[component]
pub fn SettingsPaymentMethods() -> Element {
    rsx! { PlaceholderPage { title: t!("page.settings_payment_methods").to_string(), show_back: true } }
}

#[component]
pub fn SettingsCategories() -> Element {
    rsx! { PlaceholderPage { title: t!("page.settings_categories").to_string(), show_back: true } }
}

#[component]
pub fn SettingsAppearance() -> Element {
    rsx! { PlaceholderPage { title: t!("page.settings_appearance").to_string(), show_back: true } }
}

#[component]
pub fn SettingsLanguages() -> Element {
    rsx! { PlaceholderPage { title: t!("page.settings_languages").to_string(), show_back: true } }
}

#[component]
pub fn SettingsData() -> Element {
    rsx! { PlaceholderPage { title: t!("page.settings_data").to_string(), show_back: true } }
}
