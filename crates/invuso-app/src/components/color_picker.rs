use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdCheck};

use crate::components::color_classes;
use crate::preferences::{PERSON_COLORS, color_name};

/// Row of round color swatches from the person palette (PER-01).
#[component]
pub fn ColorPicker(label: String, selected: String, on_select: EventHandler<String>) -> Element {
    rsx! {
        div { class: "flex flex-col gap-2",
            span { class: "text-sm font-medium text-floral-white-300", "{label}" }
            div { class: "flex flex-wrap gap-2", role: "radiogroup", aria_label: "{label}",
                for color in PERSON_COLORS {
                    button {
                        key: "{color}",
                        class: "flex h-11 w-11 items-center justify-center rounded-full transition ease-apple active:scale-95 {color_classes(color)}",
                        class: if selected == color { "ring-2 ring-floral-white-50 ring-offset-2 ring-offset-jet-black-900" },
                        r#type: "button",
                        role: "radio",
                        aria_checked: if selected == color { "true" } else { "false" },
                        aria_label: color_name(color),
                        onclick: move |_| on_select.call(color.to_string()),
                        if selected == color {
                            Icon { icon: LdCheck, class: "h-5 w-5" }
                        }
                    }
                }
            }
        }
    }
}
