use dioxus::prelude::*;

/// Titled card of rows, e.g. a ranking on the group overview; `children`
/// are the rows.
#[component]
pub fn CardSection(title: String, children: Element) -> Element {
    rsx! {
        section { class: "flex flex-col gap-2",
            h2 { class: "px-1 text-sm font-medium text-floral-white-300", "{title}" }
            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                {children}
            }
        }
    }
}
