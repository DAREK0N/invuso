use dioxus::prelude::*;

/// Row inside a card with an on/off switch; the whole row toggles.
#[component]
pub fn SwitchRow(
    label: String,
    checked: bool,
    onchange: EventHandler<bool>,
    #[props(default)] hint: Option<String>,
) -> Element {
    let (track, knob) = if checked {
        ("bg-cerulean-600", "translate-x-5")
    } else {
        ("bg-jet-black-700", "translate-x-0")
    };

    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            role: "switch",
            aria_checked: if checked { "true" } else { "false" },
            onclick: move |_| onchange.call(!checked),
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "text-base text-floral-white-50", "{label}" }
                if let Some(hint) = &hint {
                    span { class: "text-sm text-floral-white-400", "{hint}" }
                }
            }
            span { class: "flex h-7 w-12 shrink-0 items-center rounded-full p-1 transition-colors ease-apple {track}",
                span { class: "h-5 w-5 rounded-full bg-floral-white-50 transition-transform ease-apple {knob}" }
            }
        }
    }
}
