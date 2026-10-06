use dioxus::prelude::*;

/// Selectable payment method row of a picker list; `children` is its icon.
#[component]
pub fn MethodChoice(
    label: String,
    selected: bool,
    onclick: EventHandler<()>,
    children: Element,
) -> Element {
    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-3 rounded-2xl px-3 text-left active:bg-jet-black-800 transition-colors ease-apple",
            class: if selected { "bg-cerulean-900" },
            r#type: "button",
            role: "option",
            aria_selected: if selected { "true" } else { "false" },
            onclick: move |_| onclick.call(()),
            {children}
            span { class: "min-w-0 flex-1 truncate text-base text-floral-white-50", "{label}" }
        }
    }
}
