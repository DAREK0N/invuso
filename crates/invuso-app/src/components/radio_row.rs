use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdCheck};

/// One choice of a single-choice list inside a card (`role="radiogroup"` on
/// the parent): label, optional hint below, check mark when selected.
/// `children` is an optional leading icon.
#[component]
pub fn RadioRow(
    label: String,
    selected: bool,
    onclick: EventHandler<()>,
    #[props(default)] hint: Option<String>,
    children: Element,
) -> Element {
    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            role: "radio",
            aria_checked: if selected { "true" } else { "false" },
            onclick: move |_| onclick.call(()),
            {children}
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "text-base text-floral-white-50", "{label}" }
                if let Some(hint) = &hint {
                    span { class: "text-sm text-floral-white-400", "{hint}" }
                }
            }
            if selected {
                Icon { icon: LdCheck, class: "h-5 w-5 shrink-0 text-cerulean-300" }
            }
        }
    }
}
