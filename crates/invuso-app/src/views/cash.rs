use dioxus::prelude::*;

use super::PlaceholderPage;

#[component]
pub fn Cash() -> Element {
    rsx! { PlaceholderPage { title: t!("page.cash").to_string(), show_back: true } }
}
