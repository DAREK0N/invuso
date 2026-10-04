use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdBanknote, LdCamera, LdImage, LdPencil},
};

use crate::Route;
use crate::components::BottomSheet;

/// Action sheet behind the central plus button (idee.md 7.1).
#[component]
pub fn AddActionSheet(on_close: EventHandler<()>) -> Element {
    rsx! {
        BottomSheet { title: t!("add_sheet.title").to_string(), on_close,
            div { class: "flex flex-col gap-1 px-3 pt-2",
                ActionRow {
                    label: t!("add_sheet.scan").to_string(),
                    to: Route::Scan {},
                    on_close,
                    Icon { icon: LdCamera, class: "h-5 w-5" }
                }
                ActionRow {
                    label: t!("add_sheet.gallery").to_string(),
                    to: Route::Scan {},
                    on_close,
                    Icon { icon: LdImage, class: "h-5 w-5" }
                }
                ActionRow {
                    label: t!("add_sheet.manual").to_string(),
                    to: Route::ExpenseNew {},
                    on_close,
                    Icon { icon: LdPencil, class: "h-5 w-5" }
                }
                ActionRow {
                    label: t!("add_sheet.cash").to_string(),
                    to: Route::Cash {},
                    on_close,
                    Icon { icon: LdBanknote, class: "h-5 w-5" }
                }
            }
        }
    }
}

/// One tappable entry of the action sheet; `children` is its icon.
#[component]
fn ActionRow(label: String, to: Route, on_close: EventHandler<()>, children: Element) -> Element {
    let nav = use_navigator();

    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-4 rounded-2xl px-3 text-left text-floral-white-100 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| {
                on_close.call(());
                nav.push(to.clone());
            },
            span { class: "flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-cerulean-800 text-cerulean-200",
                {children}
            }
            span { class: "text-base font-medium", "{label}" }
        }
    }
}
