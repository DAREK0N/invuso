use dioxus::prelude::*;

use super::PlaceholderPage;

#[component]
pub fn Home() -> Element {
    rsx! { PlaceholderPage { title: t!("page.home").to_string() } }
}
