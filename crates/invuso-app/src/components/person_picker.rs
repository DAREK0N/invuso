use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdCheck, LdSquare, LdSquareCheck},
};
use invuso_core::domain::{Person, PersonId};

use crate::components::Avatar;

/// One person in a [`PersonPicker`].
#[derive(Debug, Clone, PartialEq)]
pub struct PersonOption {
    pub person: Person,
    pub selected: bool,
    /// Muted text at the end of the row, e.g. the person's share.
    pub detail: Option<String>,
}

/// List of people to pick from (UI-14). `multiple` shows check boxes and
/// lets several be selected; otherwise a check mark marks the one chosen.
/// Tapping a row reports the person; the caller keeps the selection.
#[component]
pub fn PersonPicker(
    options: Vec<PersonOption>,
    #[props(default)] multiple: bool,
    on_toggle: EventHandler<PersonId>,
) -> Element {
    rsx! {
        div {
            class: "flex flex-col",
            role: "listbox",
            aria_multiselectable: if multiple { "true" },
            for option in options {
                PersonRow {
                    key: "{option.person.id.as_str()}",
                    option: option.clone(),
                    multiple,
                    onclick: move |_| on_toggle.call(option.person.id.clone()),
                }
            }
        }
    }
}

#[component]
fn PersonRow(option: PersonOption, multiple: bool, onclick: EventHandler<()>) -> Element {
    let person = &option.person;
    let name_color = if option.selected || !multiple {
        "text-floral-white-50"
    } else {
        "text-floral-white-300"
    };

    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-3 rounded-2xl px-3 text-left active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            role: "option",
            aria_selected: if option.selected { "true" } else { "false" },
            onclick: move |_| onclick.call(()),
            if multiple {
                if option.selected {
                    Icon { icon: LdSquareCheck, class: "h-6 w-6 shrink-0 text-cerulean-300" }
                } else {
                    Icon { icon: LdSquare, class: "h-6 w-6 shrink-0 text-floral-white-500" }
                }
            }
            Avatar { name: person.name.clone(), color: person.color.clone() }
            span { class: "min-w-0 flex-1 truncate text-base {name_color}", "{person.name}" }
            if let Some(detail) = &option.detail {
                span { class: "shrink-0 text-sm tabular-nums text-floral-white-400", "{detail}" }
            }
            if !multiple && option.selected {
                Icon { icon: LdCheck, class: "h-5 w-5 shrink-0 text-cerulean-300" }
            }
        }
    }
}
