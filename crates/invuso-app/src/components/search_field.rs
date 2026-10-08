use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdSearch};

/// Search input with a magnifier, e.g. over the timeline (GRP-25) or the
/// receipt archive (RCP-09); `placeholder` also names it for screen
/// readers.
#[component]
pub fn SearchField(value: String, placeholder: String, oninput: EventHandler<String>) -> Element {
    rsx! {
        label { class: "flex min-h-12 items-center gap-2 rounded-2xl border border-jet-black-700 bg-jet-black-900 px-4 focus-within:border-cerulean-500 transition-colors",
            Icon { icon: LdSearch, class: "h-5 w-5 shrink-0 text-floral-white-400" }
            input {
                class: "min-h-12 min-w-0 flex-1 bg-transparent text-base text-floral-white-50 placeholder:text-floral-white-600 outline-none",
                r#type: "search",
                autocomplete: "off",
                placeholder: "{placeholder}",
                aria_label: "{placeholder}",
                value: "{value}",
                oninput: move |event| oninput.call(event.value()),
            }
        }
    }
}
