use dioxus::prelude::*;

use super::PlaceholderPage;

#[component]
pub fn Converter() -> Element {
    rsx! { PlaceholderPage { title: t!("page.converter").to_string() } }
}

#[component]
pub fn RateHistory() -> Element {
    rsx! { PlaceholderPage { title: t!("page.rate_history").to_string(), show_back: true } }
}
