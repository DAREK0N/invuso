use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdChevronDown, LdX},
};

/// Pill that opens a filter or switches one on and off (UI-13). `menu`
/// marks one that opens a sheet with a chevron; `active` highlights it
/// while the filter narrows the list.
#[component]
pub fn FilterChip(
    label: String,
    active: bool,
    #[props(default)] menu: bool,
    onclick: EventHandler<()>,
) -> Element {
    let colors = if active {
        "border-cerulean-500 bg-cerulean-800 text-floral-white-50"
    } else {
        "border-jet-black-700 bg-jet-black-900 text-floral-white-200 active:bg-jet-black-800"
    };

    rsx! {
        button {
            class: "flex min-h-11 items-center gap-1 rounded-full border px-4 text-sm font-medium transition-colors ease-apple {colors}",
            r#type: "button",
            aria_pressed: if !menu { if active { "true" } else { "false" } },
            aria_haspopup: if menu { "dialog" },
            onclick: move |_| onclick.call(()),
            "{label}"
            if menu {
                Icon { icon: LdChevronDown, class: "h-4 w-4 shrink-0" }
            }
        }
    }
}

/// One value of an active filter with a button to remove it.
#[component]
pub fn RemovableChip(label: String, remove_label: String, on_remove: EventHandler<()>) -> Element {
    rsx! {
        span { class: "flex min-h-11 max-w-full items-center rounded-full bg-cerulean-900 pl-4 text-sm text-floral-white-100",
            span { class: "truncate", "{label}" }
            button {
                class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-floral-white-300 active:bg-cerulean-800 transition-colors",
                r#type: "button",
                aria_label: "{remove_label}",
                onclick: move |_| on_remove.call(()),
                Icon { icon: LdX, class: "h-4 w-4" }
            }
        }
    }
}
