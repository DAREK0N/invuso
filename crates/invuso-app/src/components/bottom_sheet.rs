use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdX};

/// Modal sheet sliding up from the bottom edge (UI-08).
///
/// The backdrop carries the id `invuso-sheet-backdrop`: `MainActivity.kt`
/// clicks it on Android back, so back closes the sheet before it navigates.
#[component]
pub fn BottomSheet(title: String, on_close: EventHandler<()>, children: Element) -> Element {
    rsx! {
        div {
            id: "invuso-sheet-backdrop",
            class: "fixed inset-0 z-[1100] bg-black/50 animate-fade-in",
            onclick: move |_| on_close.call(()),
        }
        div {
            class: "fixed inset-x-0 bottom-0 z-[1101] safe-area-x animate-sheet-in",
            role: "dialog",
            aria_modal: "true",
            aria_label: "{title}",
            div {
                class: "mx-2 rounded-t-3xl border border-b-0 border-jet-black-800 bg-jet-black-900 shadow-2xl",
                style: "padding-bottom: calc(1rem + var(--safe-area-bottom));",
                div { class: "flex justify-center pt-2",
                    div { class: "h-1 w-10 rounded-full bg-floral-white-800" }
                }
                div { class: "flex items-center justify-between px-5 pt-2 pb-1",
                    h2 { class: "text-lg font-semibold text-floral-white-50", "{title}" }
                    button {
                        class: "flex h-11 w-11 items-center justify-center rounded-full text-floral-white-300 active:bg-jet-black-800 transition-colors",
                        r#type: "button",
                        aria_label: t!("common.close").to_string(),
                        onclick: move |_| on_close.call(()),
                        Icon { icon: LdX, class: "h-5 w-5" }
                    }
                }
                {children}
            }
        }
    }
}
