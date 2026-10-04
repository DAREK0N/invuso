use dioxus::prelude::*;

/// Selectable pill for short single-choice lists, e.g. the kind or owner
/// of a payment method (UI-13). `children` is an optional leading element.
#[component]
pub fn Chip(
    label: String,
    selected: bool,
    onclick: EventHandler<()>,
    children: Element,
) -> Element {
    let colors = if selected {
        "border-cerulean-500 bg-cerulean-800 text-floral-white-50"
    } else {
        "border-jet-black-700 bg-jet-black-900 text-floral-white-200 active:bg-jet-black-800"
    };

    rsx! {
        button {
            class: "flex min-h-11 items-center gap-2 rounded-full border px-4 text-sm font-medium transition-colors ease-apple {colors}",
            r#type: "button",
            role: "radio",
            aria_checked: if selected { "true" } else { "false" },
            onclick: move |_| onclick.call(()),
            {children}
            "{label}"
        }
    }
}
