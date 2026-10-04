use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdChevronDown};
use invuso_core::domain::Currency;

/// Compact button showing a currency code; opens a currency picker.
/// `label` says what the currency is for, e.g. "Zielwährung".
#[component]
pub fn CurrencyButton(currency: Currency, label: String, onclick: EventHandler<()>) -> Element {
    rsx! {
        button {
            class: "flex min-h-11 shrink-0 items-center gap-1 rounded-xl bg-jet-black-800 pr-2 pl-3 text-base font-semibold text-cerulean-200 active:bg-jet-black-700 transition-colors ease-apple",
            r#type: "button",
            aria_label: "{label}: {currency.name()}",
            onclick: move |_| onclick.call(()),
            span { class: "tabular-nums", "{currency.code()}" }
            Icon { icon: LdChevronDown, class: "h-4 w-4 text-floral-white-400" }
        }
    }
}
