use std::time::Duration;

use dioxus::prelude::*;

use crate::state::Toaster;

/// How long a toast stays visible; long enough to reach its undo button.
const TOAST_DURATION: Duration = Duration::from_secs(5);

/// Renders the current toast above the BottomNav and hides it after
/// [`TOAST_DURATION`] (simple form of UI-11).
#[component]
pub fn ToastHost() -> Element {
    let mut toaster = use_context::<Toaster>();
    let toast = toaster.current();

    // Re-runs whenever the toast changes: one timer per toast. A newer toast
    // is not hidden by an older timer because `dismiss` checks the id.
    use_effect(move || {
        if let Some(id) = toaster.current().map(|toast| toast.id) {
            spawn(async move {
                tokio::time::sleep(TOAST_DURATION).await;
                toaster.dismiss(id);
            });
        }
    });

    rsx! {
        if let Some(toast) = toast {
            div {
                key: "{toast.id}",
                class: "fixed app-toast-bottom inset-x-0 z-[1050] px-3 safe-area-x animate-fade-in",
                role: "status",
                aria_live: "polite",
                div { class: "flex min-h-12 items-center gap-3 rounded-2xl border border-jet-black-700 bg-jet-black-800 py-1 pr-1 pl-4 shadow-xl",
                    span { class: "flex-1 text-sm text-floral-white-100", "{toast.message}" }
                    if let Some(action) = toast.action.clone() {
                        button {
                            class: "min-h-11 shrink-0 rounded-xl px-4 text-sm font-semibold text-cerulean-300 active:bg-jet-black-700 transition-colors",
                            r#type: "button",
                            onclick: move |_| {
                                toaster.dismiss(toast.id);
                                (action.run)();
                            },
                            "{action.label}"
                        }
                    }
                }
            }
        }
    }
}
