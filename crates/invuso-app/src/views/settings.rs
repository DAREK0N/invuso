use dioxus::prelude::*;

use super::PlaceholderPage;

mod appearance;
mod languages;
mod licenses;
mod payment_methods;
mod people;
mod person_detail;
mod profile;

pub use appearance::SettingsAppearance;
pub use languages::SettingsLanguages;
pub use licenses::SettingsLicenses;
pub use payment_methods::SettingsPaymentMethods;
pub(crate) use people::PersonFormSheet;
pub use people::SettingsPeople;
pub use person_detail::PersonDetail;
pub use profile::Settings;

#[component]
pub fn SettingsCategories() -> Element {
    rsx! { PlaceholderPage { title: t!("page.settings_categories").to_string(), show_back: true } }
}

#[component]
pub fn SettingsData() -> Element {
    rsx! { PlaceholderPage { title: t!("page.settings_data").to_string(), show_back: true } }
}
