use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdChevronDown};
use invuso_core::domain::Currency;

/// Compact button showing a currency code; opens a currency picker.
/// `label` says what the currency is for, e.g. "Zielwährung". `unset`
/// shows no currency yet, e.g. on a receipt before the user picked one.
#[component]
pub fn CurrencyButton(
    currency: Currency,
    label: String,
    onclick: EventHandler<()>,
    #[props(default)] unset: bool,
) -> Element {
    let (text, color, spoken) = if unset {
        let choose = t!("currency_picker.choose").to_string();
        (choose.clone(), "text-pale-oak-200", choose)
    } else {
        (
            currency.code().to_string(),
            "text-cerulean-200",
            currency.name().to_string(),
        )
    };
    rsx! {
        button {
            class: "flex min-h-11 shrink-0 items-center gap-1 rounded-xl bg-jet-black-800 pr-2 pl-3 text-base font-semibold {color} active:bg-jet-black-700 transition-colors ease-apple",
            r#type: "button",
            aria_label: "{label}: {spoken}",
            onclick: move |_| onclick.call(()),
            span { class: "tabular-nums", "{text}" }
            Icon { icon: LdChevronDown, class: "h-4 w-4 text-floral-white-400" }
        }
    }
}
