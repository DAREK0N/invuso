use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdArrowLeft};

/// Sticky page header with title, optional back button and optional
/// actions on the right (`children`, UI-02). Draws behind the status bar
/// and keeps its content below it.
#[component]
pub fn TopBar(title: String, #[props(default)] show_back: bool, children: Element) -> Element {
    let nav = use_navigator();

    rsx! {
        header { class: "sticky top-0 z-40 glass border-x-0 border-t-0 app-header-inset safe-area-x",
            div { class: "flex h-14 items-center gap-1 px-2",
                if show_back {
                    button {
                        class: "flex h-11 w-11 items-center justify-center rounded-full text-floral-white-200 active:bg-jet-black-800 transition-colors",
                        r#type: "button",
                        aria_label: t!("common.back").to_string(),
                        onclick: move |_| nav.go_back(),
                        Icon { icon: LdArrowLeft, class: "h-6 w-6" }
                    }
                } else {
                    div { class: "w-2" }
                }
                h1 { class: "min-w-0 flex-1 truncate text-xl font-semibold text-floral-white-50", "{title}" }
                {children}
            }
        }
    }
}
