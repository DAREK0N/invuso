use dioxus::prelude::*;

use crate::Route;
use crate::components::{BottomNav, RouterBackTarget, ToastHost};

/// Frame for all main screens: content plus the floating BottomNav and the
/// toast above it.
#[component]
pub fn AppShell() -> Element {
    rsx! {
        main { class: "min-h-screen app-content-inset", Outlet::<Route> {} }
        ToastHost {}
        BottomNav {}
        RouterBackTarget {}
    }
}

/// Frame for capture and edit flows (idee.md 6): no BottomNav, so nothing
/// competes with the form and its save action.
#[component]
pub fn FocusShell() -> Element {
    rsx! {
        main { class: "min-h-screen app-focus-content-inset", Outlet::<Route> {} }
        RouterBackTarget {}
    }
}
