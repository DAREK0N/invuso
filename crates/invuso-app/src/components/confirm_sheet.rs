use dioxus::prelude::*;

use crate::components::{BottomSheet, Button, ButtonVariant, ErrorBanner};

/// Bottom sheet asking to confirm a destructive action, e.g. deleting.
/// `error` shows why the action failed, keeping the sheet open.
#[component]
pub fn ConfirmSheet(
    title: String,
    text: String,
    confirm_label: String,
    #[props(default)] error: Option<String>,
    on_confirm: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    rsx! {
        BottomSheet { title, on_close,
            div { class: "flex flex-col gap-4 px-5 pt-2",
                p { class: "text-base text-floral-white-300", "{text}" }
                ErrorBanner { error }
                div { class: "flex gap-3",
                    Button {
                        variant: ButtonVariant::Secondary,
                        class: "flex-1",
                        onclick: move |_| on_close.call(()),
                        {t!("common.cancel").to_string()}
                    }
                    Button {
                        variant: ButtonVariant::Danger,
                        class: "flex-1",
                        onclick: move |_| on_confirm.call(()),
                        "{confirm_label}"
                    }
                }
            }
        }
    }
}
