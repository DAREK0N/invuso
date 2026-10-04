use dioxus::prelude::*;

/// Centered icon, title and explanation for screens without content (UI-10).
/// `children` is the icon.
#[component]
pub fn EmptyState(title: String, text: String, children: Element) -> Element {
    rsx! {
        div { class: "flex flex-col items-center px-8 py-16 text-center",
            div { class: "mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-jet-black-900 text-cerulean-400",
                {children}
            }
            h2 { class: "text-lg font-semibold text-floral-white-100", "{title}" }
            p { class: "mt-1 max-w-xs text-sm text-floral-white-400", "{text}" }
        }
    }
}
