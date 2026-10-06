use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdArrowRightLeft, LdHistory, LdPenLine},
};
use invuso_core::domain::Currency;

use super::manual_rate::ManualRateSheet;
use super::{Side, initial_pair, save_pair};
use crate::components::{
    BottomSheet, Button, ButtonVariant, CardSection, CurrencyButton, CurrencyPicker, EmptyState,
    ErrorBanner, TopBar,
};
use crate::format::{NumberFormat, age_text, format_rate};
use crate::preferences::{
    default_converter_pair, default_favorite_currencies, default_home_currency, display_date,
};
use crate::state::DataRevision;
use crate::storage::{CROSS_SOURCE, Db, HistoryEntry, MANUAL_SOURCE, now_ms};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sheet {
    Pick(Side),
    ManualRate,
}

/// Rate archive of a currency pair (FX-08): every day's rate, newest
/// first, with source and fetch time; manual rates can be added (FX-10).
/// Shares the pair with the converter.
#[component]
pub fn RateHistory() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let initial = use_hook(|| initial_pair(&db).map_err(|e| e.to_string()));
    let (start_from, start_to) = initial.clone().unwrap_or_else(|_| {
        default_converter_pair(default_home_currency(), &default_favorite_currencies())
    });
    let mut from = use_signal(|| start_from);
    let mut to = use_signal(|| start_to);
    let mut sheet = use_signal(|| None::<Sheet>);
    let mut error = use_signal(|| initial.err());

    let history_db = db.clone();
    let history = use_memo(move || {
        revision.track();
        history_db
            .rate_history(from(), to())
            .map_err(|e| format!("{} {e}", t!("rate_history.load_error")))
    });

    // A `Callback` is `Copy`, so the swap button and the picker can share it.
    let apply = use_callback(move |(new_from, new_to): (Currency, Currency)| {
        from.set(new_from);
        to.set(new_to);
        error.set(
            save_pair(&db, new_from, new_to)
                .err()
                .map(|e| e.to_string()),
        );
    });
    let mut pick = move |side: Side, currency: Currency| {
        sheet.set(None);
        let (current_from, current_to) = (from(), to());
        match side {
            Side::From if currency == current_to => apply.call((currency, current_from)),
            Side::From => apply.call((currency, current_to)),
            Side::To if currency == current_from => apply.call((current_to, currency)),
            Side::To => apply.call((current_from, currency)),
        }
    };
    let format = NumberFormat::current();
    let now = now_ms();

    rsx! {
        TopBar { title: t!("page.rate_history").to_string(), show_back: true }
        div { class: "mx-4 flex flex-col gap-4 pt-4 pb-6 safe-area-x",
            ErrorBanner { error: error() }
            div { class: "flex items-center justify-center gap-2",
                CurrencyButton {
                    currency: from(),
                    label: t!("converter.from").to_string(),
                    onclick: move |_| sheet.set(Some(Sheet::Pick(Side::From))),
                }
                button {
                    class: "flex h-11 w-11 items-center justify-center rounded-full text-cerulean-200 active:bg-jet-black-800 transition-colors",
                    r#type: "button",
                    aria_label: t!("converter.swap").to_string(),
                    onclick: move |_| apply.call((to(), from())),
                    Icon { icon: LdArrowRightLeft, class: "h-5 w-5" }
                }
                CurrencyButton {
                    currency: to(),
                    label: t!("converter.to").to_string(),
                    onclick: move |_| sheet.set(Some(Sheet::Pick(Side::To))),
                }
            }
            if from() != to() {
                Button {
                    variant: ButtonVariant::Secondary,
                    class: "w-full",
                    onclick: move |_| sheet.set(Some(Sheet::ManualRate)),
                    Icon { icon: LdPenLine, class: "h-5 w-5" }
                    {t!("rate_history.add_manual").to_string()}
                }
            }
            match &*history.read() {
                Err(message) => rsx! { ErrorBanner { error: Some(message.clone()) } },
                Ok(entries) if entries.is_empty() => rsx! {
                    EmptyState {
                        title: t!("rate_history.empty_title").to_string(),
                        text: t!("rate_history.empty_text", from = from().code(), to = to().code()).to_string(),
                        Icon { icon: LdHistory, class: "h-8 w-8" }
                    }
                },
                Ok(entries) => rsx! {
                    CardSection {
                        title: t!("rate_history.heading", base = from().code(), quote = to().code()).to_string(),
                        for (index, entry) in entries.iter().enumerate() {
                            HistoryRow {
                                key: "{index}",
                                rate: format_rate(entry.rate.value(), format),
                                date: display_date(&entry.rate_date),
                                detail: entry_detail(entry, now),
                                manual: entry.source == MANUAL_SOURCE,
                            }
                        }
                    }
                },
            }
        }
        match sheet() {
            Some(Sheet::Pick(side)) => rsx! {
                BottomSheet {
                    title: match side {
                        Side::From => t!("converter.from").to_string(),
                        Side::To => t!("converter.to").to_string(),
                    },
                    on_close: move |_| sheet.set(None),
                    div { class: "flex max-h-[70vh] flex-col gap-2 overflow-y-auto overscroll-contain px-3 pt-2",
                        CurrencyPicker {
                            selected: if side == Side::From { from() } else { to() },
                            on_select: move |currency| pick(side, currency),
                        }
                    }
                }
            },
            Some(Sheet::ManualRate) => rsx! {
                ManualRateSheet {
                    from: from(),
                    to: to(),
                    reference: history.read().as_ref().ok().and_then(|h| {
                        h.iter().find(|e| e.source != MANUAL_SOURCE).map(|e| e.rate)
                    }),
                    on_saved: move |_| sheet.set(None),
                    on_close: move |_| sheet.set(None),
                }
            },
            None => rsx! {},
        }
    }
}

#[component]
fn HistoryRow(rate: String, date: String, detail: String, manual: bool) -> Element {
    let rate_color = if manual {
        "text-cerulean-200"
    } else {
        "text-floral-white-50"
    };
    rsx! {
        div { class: "flex min-h-14 items-center gap-3 border-b border-jet-black-800 px-4 py-2 last:border-b-0",
            div { class: "flex min-w-0 flex-1 flex-col",
                span { class: "text-base tabular-nums text-floral-white-100", "{date}" }
                span { class: "truncate text-sm text-floral-white-400", "{detail}" }
            }
            span { class: "shrink-0 text-base font-semibold tabular-nums {rate_color}", "{rate}" }
        }
    }
}

/// "Frankfurter · abgerufen vor 2 Std.", "manuell · eingegeben vor 5 Min.".
fn entry_detail(entry: &HistoryEntry, now: i64) -> String {
    let age = age_text(entry.fetched_at, now);
    let when = if entry.source == MANUAL_SOURCE {
        t!("rate_history.entered", age = age)
    } else {
        t!("rate_history.fetched", age = age)
    };
    let mut parts = vec![source_name(&entry.source)];
    if entry.crossed {
        parts.push(t!("rate_history.crossed").to_string());
    }
    parts.push(when.to_string());
    parts.join(" · ")
}

/// Readable name of an `ExchangeRate.source`; a cross rate computed from
/// two providers names both.
fn source_name(source: &str) -> String {
    source
        .split(" + ")
        .map(|part| match part {
            MANUAL_SOURCE => t!("rate_history.source_manual").to_string(),
            CROSS_SOURCE => t!("rate_history.source_cross").to_string(),
            "frankfurter" => "Frankfurter".to_string(),
            "currency-api" => "Exchange API".to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" + ")
}
