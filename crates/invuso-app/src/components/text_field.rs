use dioxus::prelude::*;

/// Labeled single-line text input with an optional error below it.
/// `id` links label and input and must be unique on the page.
#[component]
pub fn TextField(
    id: String,
    label: String,
    value: String,
    oninput: EventHandler<String>,
    #[props(default)] placeholder: String,
    #[props(default)] error: Option<String>,
) -> Element {
    let border = if error.is_some() {
        "border-watermelon-400"
    } else {
        "border-jet-black-700 focus:border-cerulean-500"
    };
    let error_id = format!("{id}-error");

    rsx! {
        div { class: "flex flex-col gap-2",
            label { class: "text-sm font-medium text-floral-white-300", r#for: "{id}", "{label}" }
            input {
                id: "{id}",
                class: "min-h-12 w-full rounded-2xl border bg-jet-black-900 px-4 text-base text-floral-white-50 placeholder:text-floral-white-600 outline-none transition-colors {border}",
                r#type: "text",
                autocomplete: "off",
                value,
                placeholder,
                aria_invalid: if error.is_some() { "true" },
                aria_describedby: if error.is_some() { "{error_id}" },
                oninput: move |event| oninput.call(event.value()),
            }
            if let Some(error) = &error {
                p { id: "{error_id}", class: "text-sm text-watermelon-300", role: "alert", "{error}" }
            }
        }
    }
}
