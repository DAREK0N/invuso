use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdArrowRightLeft};
use invuso_core::Decimal;
use invuso_core::domain::Currency;
use invuso_core::fx::Rate;

use crate::components::{BottomSheet, Button, CompactNumberInput};
use crate::format::{NumberFormat, format_rate, number_text, parse_number};

/// Decimals a rate can be typed with, enough for `1 JPY = 0.0056271 EUR`.
const RATE_DECIMALS: u32 = 8;

/// Sheet to type in a rate between `from` and `to`, e.g. an exchange
/// office's (FX-10) or the one on the card statement (EXP-08).
/// `reference` is the daily rate `from → to` for comparison; it also picks
/// the direction that reads best (`1 EUR = 177 JPY` rather than
/// `1 JPY = 0.0056 EUR`). `initial` prefills a rate typed before, in its
/// own direction. `on_submit` gets the rate as typed; `error` shows what
/// went wrong with it on the caller's side, e.g. saving.
#[component]
pub fn RateSheet(
    from: Currency,
    to: Currency,
    reference: Option<Rate>,
    title: String,
    hint: String,
    #[props(default)] initial: Option<Rate>,
    #[props(default)] error: Option<String>,
    on_submit: EventHandler<Rate>,
    on_close: EventHandler<()>,
) -> Element {
    let format = NumberFormat::current();
    let mut turned = use_signal(|| match initial {
        Some(rate) => rate.base() == to,
        None => reference.is_some_and(|r| r.value() < Decimal::ONE),
    });
    let mut value = use_signal(|| {
        initial
            .map(|rate| number_text(rate.value(), format))
            .unwrap_or_default()
    });
    let mut invalid = use_signal(|| None::<String>);
    let (base, quote) = if turned() { (to, from) } else { (from, to) };
    let shown_reference = reference.map(|rate| {
        let rate = if rate.base() == base {
            rate
        } else {
            rate.inverse()
        };
        t!(
            "manual_rate.reference",
            line = t!(
                "converter.rate_line",
                base = base.code(),
                rate = format_rate(rate.value(), format),
                quote = quote.code()
            )
        )
        .to_string()
    });
    let message = invalid().or(error);

    let submit = move |_| {
        let typed = parse_number(&value(), format)
            .filter(|v| *v > Decimal::ZERO)
            .and_then(|v| Rate::new(base, quote, v).ok());
        match typed {
            Some(rate) => on_submit.call(rate),
            None => invalid.set(Some(t!("manual_rate.required").to_string())),
        }
    };

    rsx! {
        BottomSheet { title, on_close,
            div { class: "flex flex-col gap-4 px-5 pt-2",
                p { class: "text-sm text-floral-white-400", "{hint}" }
                div { class: "flex items-center gap-2",
                    span { class: "shrink-0 text-base font-semibold tabular-nums text-floral-white-100", "1 {base.code()} =" }
                    CompactNumberInput {
                        id: "manual-rate",
                        label: t!("manual_rate.value").to_string(),
                        value: value(),
                        decimals: RATE_DECIMALS,
                        unit: quote.code().to_string(),
                        invalid: message.is_some(),
                        oninput: move |text| {
                            value.set(text);
                            invalid.set(None);
                        },
                    }
                    button {
                        class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-cerulean-200 active:bg-jet-black-800 transition-colors",
                        r#type: "button",
                        aria_label: t!("manual_rate.turn").to_string(),
                        onclick: move |_| {
                            turned.toggle();
                            value.set(String::new());
                        },
                        Icon { icon: LdArrowRightLeft, class: "h-5 w-5" }
                    }
                }
                if let Some(line) = shown_reference {
                    p { class: "text-sm tabular-nums text-floral-white-400", "{line}" }
                }
                if let Some(message) = message {
                    p { class: "text-sm text-watermelon-300", role: "alert", "{message}" }
                }
                Button { class: "w-full", onclick: submit, {t!("common.save").to_string()} }
            }
        }
    }
}
