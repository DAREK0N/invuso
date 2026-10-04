use dioxus::prelude::*;
use invuso_core::domain::Currency;

use crate::format::{NumberFormat, amount_keystroke, currency_symbol, display_amount_text};

/// Small amount field for a part of a total, e.g. what one payer paid
/// (EXP-02). Same input rules as `AmountInput`, without a currency button:
/// the currency is fixed by the surrounding form. `value` is canonical text
/// (`format::parse_amount`).
#[component]
pub fn CompactAmountInput(
    id: String,
    label: String,
    value: String,
    currency: Currency,
    oninput: EventHandler<String>,
    #[props(default)] invalid: bool,
) -> Element {
    // See `AmountInput`: forces a render when a keystroke is rejected.
    let mut corrections = use_signal(|| 0_u32);
    corrections.read();

    let format = NumberFormat::current();
    let shown = display_amount_text(&value, format);
    let symbol = currency_symbol(currency);
    let border = if invalid {
        "border-watermelon-400"
    } else {
        "border-jet-black-700 focus-within:border-cerulean-500"
    };
    let typed = shown.clone();

    rsx! {
        label {
            r#for: "{id}",
            class: "flex min-h-11 w-36 shrink-0 cursor-text items-center gap-1 rounded-xl border bg-jet-black-950 px-3 transition-colors {border}",
            if format.symbol_before {
                span { class: "text-sm text-floral-white-400", aria_hidden: "true", "{symbol}" }
            }
            input {
                id: "{id}",
                class: "min-w-0 flex-1 bg-transparent text-right text-base font-semibold tabular-nums text-floral-white-50 placeholder:text-floral-white-600 outline-none",
                r#type: "text",
                inputmode: "decimal",
                autocomplete: "off",
                placeholder: "0",
                aria_label: "{label}",
                aria_invalid: if invalid { "true" },
                value: "{shown}",
                oninput: move |event| {
                    let (text, redraw) = amount_keystroke(&typed, &event.value(), currency, format);
                    if redraw {
                        *corrections.write() += 1;
                    }
                    if let Some(text) = text {
                        oninput.call(text);
                    }
                },
            }
            if !format.symbol_before {
                span { class: "text-sm text-floral-white-400", aria_hidden: "true", "{symbol}" }
            }
        }
    }
}
