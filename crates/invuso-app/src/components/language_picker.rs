use dioxus::prelude::*;

use crate::components::OptionRow;
use crate::preferences::{TARGET_LANGUAGES, language_name};

/// List of the supported target languages with code and name (SET-02).
/// With `follow_global`, a first entry selects `""`: no language of its own,
/// the global one applies (TRL-05); the text names that one.
#[component]
pub fn LanguagePicker(
    selected: String,
    on_select: EventHandler<String>,
    #[props(default)] follow_global: Option<String>,
) -> Element {
    rsx! {
        div { role: "listbox", class: "flex flex-col gap-1",
            if let Some(text) = follow_global {
                OptionRow {
                    code: "–".to_string(),
                    name: text,
                    selected: selected.is_empty(),
                    onclick: move |_| on_select.call(String::new()),
                }
            }
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
