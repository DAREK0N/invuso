use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdX};

/// Labeled date input (`YYYY-MM-DD`, empty when unset) with a button to
/// clear it. The WebView opens the system date picker for `type="date"`,
/// so no JavaScript or Android code is needed. `min` is the earliest date
/// the picker offers.
#[component]
pub fn DateField(
    id: String,
    label: String,
    value: String,
    oninput: EventHandler<String>,
    #[props(default)] min: Option<String>,
    #[props(default)] error: Option<String>,
) -> Element {
    let border = if error.is_some() {
        "border-watermelon-400"
    } else {
        "border-jet-black-700 focus-within:border-cerulean-500"
    };
    let error_id = format!("{id}-error");

    rsx! {
        div { class: "flex flex-col gap-2",
            label { class: "text-sm font-medium text-floral-white-300", r#for: "{id}", "{label}" }
            div { class: "flex min-h-12 items-center rounded-2xl border bg-jet-black-900 transition-colors {border}",
                input {
                    id: "{id}",
                    class: "min-h-12 min-w-0 flex-1 bg-transparent px-4 text-base text-floral-white-50 outline-none",
                    r#type: "date",
                    value: "{value}",
                    min,
                    aria_invalid: if error.is_some() { "true" },
                    aria_describedby: if error.is_some() { "{error_id}" },
                    oninput: move |event| oninput.call(event.value()),
                }
                if !value.is_empty() {
                    button {
                        class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-floral-white-400 active:bg-jet-black-800 transition-colors",
                        r#type: "button",
                        aria_label: t!("common.clear_date").to_string(),
                        onclick: move |_| oninput.call(String::new()),
                        Icon { icon: LdX, class: "h-5 w-5" }
                    }
                }
            }
            if let Some(error) = &error {
                p { id: "{error_id}", class: "text-sm text-watermelon-300", role: "alert", "{error}" }
            }
        }
    }
}
