use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdCheck};

/// One selectable entry of a picker list: short code, name, an optional
/// muted detail (e.g. a currency symbol) and a check mark when selected.
#[component]
pub fn OptionRow(
    code: String,
    name: String,
    selected: bool,
    onclick: EventHandler<()>,
    #[props(default)] detail: Option<String>,
) -> Element {
    let colors = if selected {
        "bg-cerulean-800 text-floral-white-50"
    } else {
        "text-floral-white-100 active:bg-jet-black-800"
    };

    rsx! {
        button {
            class: "flex min-h-12 w-full items-center gap-3 rounded-2xl px-3 text-left transition-colors ease-apple {colors}",
            r#type: "button",
            role: "option",
            aria_selected: if selected { "true" } else { "false" },
            onclick: move |_| onclick.call(()),
            span { class: "w-12 shrink-0 text-sm font-semibold tabular-nums text-cerulean-300", "{code}" }
            span { class: "flex-1 truncate text-base", "{name}" }
            if let Some(detail) = &detail {
                span { class: "shrink-0 text-sm text-floral-white-400", "{detail}" }
            }
            if selected {
                Icon { icon: LdCheck, class: "h-5 w-5 shrink-0 text-cerulean-300" }
            }
        }
    }
}
