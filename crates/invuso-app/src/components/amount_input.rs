use dioxus::prelude::*;
use invuso_core::domain::Currency;

use crate::components::CurrencyButton;
use crate::format::{
    NumberFormat, amount_edit, clean_amount_input, currency_symbol, display_amount_text,
};

/// Amount field with its currency (UI-06). Only valid amounts for the
/// currency get through: digits and one decimal separator, at most the
/// currency's decimals. While typing, the field shows thousands separators
/// and the currency symbol next to the number, as the app language writes
/// them. `value` is the canonical text without grouping; read it with
/// `format::parse_amount`.
#[component]
pub fn AmountInput(
    id: String,
    label: String,
    value: String,
    currency: Currency,
    oninput: EventHandler<String>,
    on_currency_click: EventHandler<()>,
    #[props(default)] currency_label: String,
) -> Element {
    // When a keystroke is rejected or rewritten (e.g. a separator added),
    // the parent's text may stay the same and this component would not
    // re-render, leaving the typed characters in the field. Bumping this
    // forces a render, and Dioxus always writes `value` back because it is a
    // volatile attribute.
    let mut corrections = use_signal(|| 0_u32);
    corrections.read();

    let format = NumberFormat::current();
    let shown = display_amount_text(&value, format);
    // The field is as wide as its text, so the symbol sits right at the
    // number; `ch` is the width of a digit with tabular numbers.
    let width = shown.chars().count().max(1);
    let symbol = currency_symbol(currency);
    let symbol_color = if shown.is_empty() {
        "text-floral-white-600"
    } else {
        "text-floral-white-300"
    };
    let symbol_span = rsx! {
        span { class: "shrink-0 text-3xl font-semibold {symbol_color}", aria_hidden: "true", "{symbol}" }
    };
    let typed = shown.clone();

    rsx! {
        div { class: "flex min-h-16 items-center gap-3 rounded-2xl border border-jet-black-700 bg-jet-black-900 px-3 focus-within:border-cerulean-500 transition-colors",
            CurrencyButton {
                currency,
                label: if currency_label.is_empty() { label.clone() } else { currency_label },
                onclick: on_currency_click,
            }
            // Tapping anywhere right of the button focuses the input.
            label {
                r#for: "{id}",
                class: "flex min-w-0 flex-1 cursor-text items-baseline justify-end gap-1 overflow-hidden",
                if format.symbol_before {
                    {symbol_span.clone()}
                }
                input {
                    id: "{id}",
                    class: "min-w-0 max-w-full bg-transparent text-right text-3xl font-semibold tabular-nums text-floral-white-50 placeholder:text-floral-white-600 outline-none",
                    style: "width: calc({width}ch + 2px);",
                    r#type: "text",
                    inputmode: "decimal",
                    autocomplete: "off",
                    placeholder: "0",
                    aria_label: "{label}",
                    value: "{shown}",
                    oninput: move |event| {
                        let raw = event.value();
                        let edit = amount_edit(&typed, &raw, format);
                        match clean_amount_input(&edit, currency, format) {
                            Some(text) => {
                                if display_amount_text(&text, format) != raw {
                                    *corrections.write() += 1;
                                }
                                oninput.call(text);
                            }
                            None => *corrections.write() += 1,
                        }
                    },
                }
                if !format.symbol_before {
                    {symbol_span}
                }
            }
        }
    }
}
