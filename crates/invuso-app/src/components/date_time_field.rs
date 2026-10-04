use dioxus::prelude::*;

/// Labeled date and time inputs side by side (UI-16): `YYYY-MM-DD` and
/// `HH:MM`. The WebView opens the system pickers for `type="date"` and
/// `type="time"`, so no JavaScript or Android code is needed.
#[component]
pub fn DateTimeField(
    id: String,
    label: String,
    date: String,
    time: String,
    date_label: String,
    time_label: String,
    on_date: EventHandler<String>,
    on_time: EventHandler<String>,
    #[props(default)] error: Option<String>,
) -> Element {
    let border = if error.is_some() {
        "border-watermelon-400"
    } else {
        "border-jet-black-700 focus-within:border-cerulean-500"
    };
    let error_id = format!("{id}-error");

    rsx! {
        fieldset { class: "flex flex-col gap-2",
            legend { class: "mb-2 text-sm font-medium text-floral-white-300", "{label}" }
            div { class: "flex gap-2",
                input {
                    id: "{id}-date",
                    class: "min-h-12 min-w-0 flex-1 rounded-2xl border bg-jet-black-900 px-4 text-base text-floral-white-50 outline-none transition-colors {border}",
                    r#type: "date",
                    value: "{date}",
                    aria_label: "{date_label}",
                    aria_invalid: if error.is_some() { "true" },
                    aria_describedby: if error.is_some() { "{error_id}" },
                    oninput: move |event| on_date.call(event.value()),
                }
                input {
                    id: "{id}-time",
                    class: "min-h-12 w-32 shrink-0 rounded-2xl border bg-jet-black-900 px-4 text-base text-floral-white-50 outline-none transition-colors {border}",
                    r#type: "time",
                    value: "{time}",
                    aria_label: "{time_label}",
                    aria_invalid: if error.is_some() { "true" },
                    aria_describedby: if error.is_some() { "{error_id}" },
                    oninput: move |event| on_time.call(event.value()),
                }
            }
            if let Some(error) = &error {
                p { id: "{error_id}", class: "text-sm text-watermelon-300", role: "alert", "{error}" }
            }
        }
    }
}
