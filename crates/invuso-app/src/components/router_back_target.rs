use dioxus::prelude::*;

use crate::Route;

/// Hidden bridge target for the Android hardware/gesture back.
///
/// `MainActivity.kt` clicks this element when back is pressed and
/// `data-can-go-back` is "true"; otherwise Android handles back itself
/// (leaves the app). Rendered by every layout so it exists on all pages.
#[component]
pub fn RouterBackTarget() -> Element {
    let nav = use_navigator();
    // Subscribing to the route re-evaluates `can_go_back` after every navigation.
    let _route = use_route::<Route>();
    let can_go_back = nav.can_go_back();

    rsx! {
        button {
            id: "invuso-router-back",
            class: "hidden",
            r#type: "button",
            tabindex: "-1",
            aria_hidden: "true",
            "data-can-go-back": if can_go_back { "true" } else { "false" },
            onclick: move |_| nav.go_back(),
        }
    }
}
