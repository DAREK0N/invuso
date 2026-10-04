use dioxus::prelude::*;

/// Inline error message, e.g. when saving in a sheet failed. Renders
/// nothing without an error.
#[component]
pub fn ErrorBanner(error: Option<String>) -> Element {
    rsx! {
        if let Some(error) = error {
            p { class: "rounded-2xl bg-watermelon-900 px-4 py-3 text-sm text-watermelon-200", role: "alert",
                "{error}"
            }
        }
    }
}
