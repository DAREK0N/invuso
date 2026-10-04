use dioxus::prelude::*;

use crate::components::OptionRow;
use crate::preferences::{TARGET_LANGUAGES, language_name};

/// List of the supported target languages with code and name (SET-02).
#[component]
pub fn LanguagePicker(selected: String, on_select: EventHandler<String>) -> Element {
    rsx! {
        div { role: "listbox", class: "flex flex-col gap-1",
            for code in TARGET_LANGUAGES {
                OptionRow {
                    key: "{code}",
                    code: code.to_string(),
                    name: language_name(code),
                    selected: selected == code,
                    onclick: move |_| on_select.call(code.to_string()),
                }
            }
        }
    }
}
