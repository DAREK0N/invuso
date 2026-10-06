use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdArrowRightLeft};
use invuso_core::Decimal;
use invuso_core::domain::Currency;
use invuso_core::fx::Rate;

use crate::clock::local_now;
use crate::components::{BottomSheet, Button, CompactNumberInput};
use crate::format::{NumberFormat, format_rate, parse_number};
use crate::services::converter::save_manual_rate;
use crate::state::{DataRevision, Toaster};
use crate::storage::Db;

/// Decimals a manual rate can be typed with, enough for `1 JPY = 0.0056271 EUR`.
const RATE_DECIMALS: u32 = 8;

/// Sheet to type in a rate, e.g. an exchange office's (FX-10). It is
/// archived as "manual" and the converter uses it for this pair until the
/// user goes back to the daily rate. `reference` is the daily rate
/// `from → to` for comparison; it also picks the direction that reads
/// best (`1 EUR = 177 JPY` rather than `1 JPY = 0.0056 EUR`).
/// `on_saved` gets the new rate's id.
#[component]
pub(super) fn ManualRateSheet(
    from: Currency,
    to: Currency,
    reference: Option<Rate>,
    on_saved: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let mut revision = use_context::<DataRevision>();
    let mut toaster = use_context::<Toaster>();
    let mut turned = use_signal(|| reference.is_some_and(|r| r.value() < Decimal::ONE));
    let mut value = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);

    let format = NumberFormat::current();
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

    let save = move |_| {
        let Some(rate) = parse_number(&value(), format).filter(|v| *v > Decimal::ZERO) else {
            error.set(Some(t!("manual_rate.required").to_string()));
            return;
        };
        match save_manual_rate(&db, base, quote, rate, &local_now().0) {
            Ok(id) => {
                revision.bump();
                toaster.show(t!("manual_rate.saved").to_string(), None);
                on_saved.call(id);
            }
            Err(e) => error.set(Some(format!("{} {e}", t!("manual_rate.save_error")))),
        }
    };

    rsx! {
        BottomSheet { title: t!("manual_rate.title").to_string(), on_close,
            div { class: "flex flex-col gap-4 px-5 pt-2",
                p { class: "text-sm text-floral-white-400", {t!("manual_rate.hint").to_string()} }
                div { class: "flex items-center gap-2",
                    span { class: "shrink-0 text-base font-semibold tabular-nums text-floral-white-100", "1 {base.code()} =" }
                    CompactNumberInput {
                        id: "manual-rate",
                        label: t!("manual_rate.value").to_string(),
                        value: value(),
                        decimals: RATE_DECIMALS,
                        unit: quote.code().to_string(),
                        invalid: error().is_some(),
                        oninput: move |text| {
                            value.set(text);
                            error.set(None);
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
                if let Some(message) = error() {
                    p { class: "text-sm text-watermelon-300", role: "alert", "{message}" }
                }
                Button { class: "w-full", onclick: save, {t!("common.save").to_string()} }
            }
        }
    }
}
