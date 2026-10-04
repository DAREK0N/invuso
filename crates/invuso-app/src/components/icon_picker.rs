use dioxus::prelude::*;

use crate::components::{PaymentIconGlyph, color_classes};
use crate::preferences::{PAYMENT_ICONS, payment_icon_name};

/// Row of round icon buttons for payment methods, drawn in the method's
/// `color` so the preview matches the list.
#[component]
pub fn IconPicker(
    label: String,
    selected: String,
    color: String,
    on_select: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "flex flex-col gap-2",
            span { class: "text-sm font-medium text-floral-white-300", "{label}" }
            div { class: "flex flex-wrap gap-2", role: "radiogroup", aria_label: "{label}",
                for icon in PAYMENT_ICONS {
                    IconOption {
                        key: "{icon}",
                        icon,
                        color: color.clone(),
                        selected: selected == icon,
                        onclick: move |_| on_select.call(icon.to_string()),
                    }
                }
            }
        }
    }
}

#[component]
fn IconOption(
    icon: &'static str,
    color: String,
    selected: bool,
    onclick: EventHandler<()>,
) -> Element {
    let look = if selected {
        format!(
            "{} ring-2 ring-floral-white-50 ring-offset-2 ring-offset-jet-black-900",
            color_classes(&color)
        )
    } else {
        "bg-jet-black-800 text-floral-white-300".to_string()
    };

    rsx! {
        button {
            class: "flex h-11 w-11 items-center justify-center rounded-full transition ease-apple active:scale-95 {look}",
            r#type: "button",
            role: "radio",
            aria_checked: if selected { "true" } else { "false" },
            aria_label: payment_icon_name(icon),
            onclick: move |_| onclick.call(()),
            PaymentIconGlyph { icon: icon.to_string() }
        }
    }
}
