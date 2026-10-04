use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdConstruction};

use crate::components::{EmptyState, TopBar};

/// Stand-in for a screen whose features belong to a later milestone.
#[component]
pub fn PlaceholderPage(title: String, #[props(default)] show_back: bool) -> Element {
    rsx! {
        TopBar { title, show_back }
        EmptyState {
            title: t!("placeholder.title").to_string(),
            text: t!("placeholder.text").to_string(),
            Icon { icon: LdConstruction, class: "h-8 w-8" }
        }
    }
}
