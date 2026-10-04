use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdChevronRight};

/// Row inside a card that leads to another page; `children` is a summary
/// on the right, e.g. a count or avatars.
#[component]
pub fn LinkRow(label: String, onclick: EventHandler<()>, children: Element) -> Element {
    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            span { class: "flex-1 text-base text-floral-white-50", "{label}" }
            {children}
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
        }
    }
}
