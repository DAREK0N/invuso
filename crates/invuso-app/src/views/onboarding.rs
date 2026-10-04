use dioxus::prelude::*;

use super::PlaceholderPage;

#[component]
pub fn Onboarding() -> Element {
    rsx! { PlaceholderPage { title: t!("page.onboarding").to_string() } }
}
