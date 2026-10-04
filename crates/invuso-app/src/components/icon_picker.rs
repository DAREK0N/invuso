use dioxus::prelude::*;

use crate::components::{GroupIconGlyph, PaymentIconGlyph, color_classes};
use crate::preferences::{GROUP_ICONS, PAYMENT_ICONS, group_icon_name, payment_icon_name};

/// Which icons an [`IconPicker`] offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IconSet {
    #[default]
    Payment,
    Group,
}

impl IconSet {
    fn icons(self) -> &'static [&'static str] {
        match self {
            Self::Payment => &PAYMENT_ICONS,
            Self::Group => &GROUP_ICONS,
        }
    }

    fn name(self, icon: &str) -> String {
        match self {
            Self::Payment => payment_icon_name(icon),
            Self::Group => group_icon_name(icon),
        }
    }
}

/// Row of round icon buttons for payment methods or groups, drawn in the
/// item's `color` so the preview matches the list.
#[component]
pub fn IconPicker(
    label: String,
    selected: String,
    color: String,
    on_select: EventHandler<String>,
    #[props(default)] set: IconSet,
) -> Element {
    rsx! {
        div { class: "flex flex-col gap-2",
            span { class: "text-sm font-medium text-floral-white-300", "{label}" }
            div { class: "flex flex-wrap gap-2", role: "radiogroup", aria_label: "{label}",
                for &icon in set.icons() {
                    IconOption {
                        key: "{icon}",
                        icon,
                        set,
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
    set: IconSet,
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
            aria_label: set.name(icon),
            onclick: move |_| onclick.call(()),
            match set {
                IconSet::Payment => rsx! { PaymentIconGlyph { icon: icon.to_string() } },
                IconSet::Group => rsx! { GroupIconGlyph { icon: icon.to_string() } },
            }
        }
    }
}
