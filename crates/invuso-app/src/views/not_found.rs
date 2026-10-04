use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdFileQuestion};

use crate::Route;
use crate::components::{EmptyState, TopBar};

#[component]
pub fn NotFound(route: Vec<String>) -> Element {
    let path = format!("/{}", route.join("/"));

    rsx! {
        TopBar { title: t!("page.not_found").to_string() }
        EmptyState { title: t!("page.not_found").to_string(), text: path,
            Icon { icon: LdFileQuestion, class: "h-8 w-8" }
        }
        div { class: "flex justify-center",
            Link {
                class: "rounded-xl bg-cerulean-600 px-5 py-3 font-medium text-floral-white-50 active:bg-cerulean-700",
                to: Route::Home {},
                {t!("nav.home").to_string()}
            }
        }
    }
}
