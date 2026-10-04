use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdArrowUpDown, LdCloudOff, LdTriangleAlert},
};
use invuso_core::domain::{Currency, Money};
use invuso_core::fx;

use super::PlaceholderPage;
use crate::components::{
    AmountInput, BottomSheet, CurrencyButton, CurrencyPicker, ErrorBanner, MoneyText, TopBar,
};
use crate::format::{NumberFormat, age_text, fit_amount_text, format_rate, parse_amount};
use crate::preferences::{
    default_converter_pair, default_favorite_currencies, default_home_currency, display_date,
};
use crate::state::{DataRevision, RateStatus};
use crate::storage::{
    CONVERTER_FROM, CONVERTER_TO, Db, FAVORITE_CURRENCY_LIST, StorageError, now_ms,
};

/// A rate fetched longer ago than this gets the warning (FX-03); the app
/// refreshes once a day while online.
const STALE_AFTER_MS: i64 = 24 * 60 * 60 * 1000;

/// Which side of the converter the currency picker is choosing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    From,
    To,
}

/// Currency converter (FX-05): amount, two currencies, swap, live result
/// with the newest archived rate, its date and a warning when it is old or
/// the app is offline (FX-03). The last pair is remembered.
#[component]
pub fn Converter() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let status = use_context::<RateStatus>();
    let initial = use_hook(|| initial_pair(&db).map_err(|e| e.to_string()));
    let (start_from, start_to) = initial.clone().unwrap_or_else(|_| {
        let home = default_home_currency();
        default_converter_pair(home, &default_favorite_currencies())
    });
    let mut from = use_signal(|| start_from);
    let mut to = use_signal(|| start_to);
    let mut amount_text = use_signal(String::new);
    let mut picking = use_signal(|| None::<Side>);
    let mut error = use_signal(|| {
        initial
            .err()
            .map(|e| format!("{} {e}", t!("converter.load_error")))
    });

    let quote_db = db.clone();
    let quote = use_memo(move || {
        revision.track();
        quote_db
            .latest_rate(from(), to())
            .map_err(|e| format!("{} {e}", t!("converter.rate_error")))
    });

    // A `Callback` is `Copy`, so the swap button and the picker can share it.
    let apply = use_callback(move |(new_from, new_to): (Currency, Currency)| {
        let format = NumberFormat::current();
        let fitted = fit_amount_text(&amount_text.peek(), new_from, format);
        amount_text.set(fitted);
        from.set(new_from);
        to.set(new_to);
        let saved = db
            .set_setting(CONVERTER_FROM, new_from.code())
            .and_then(|()| db.set_setting(CONVERTER_TO, new_to.code()));
        error.set(
            saved
                .err()
                .map(|e| format!("{} {e}", t!("converter.save_error"))),
        );
    });
    let mut pick = move |side: Side, currency: Currency| {
        picking.set(None);
        let (current_from, current_to) = (from(), to());
        // Picking the other side's currency swaps instead of showing a
        // pointless EUR → EUR.
        match side {
            Side::From if currency == current_to => apply.call((currency, current_from)),
            Side::From => apply.call((currency, current_to)),
            Side::To if currency == current_from => apply.call((current_to, currency)),
            Side::To => apply.call((current_from, currency)),
        }
    };

    let format = NumberFormat::current();
    let amount = parse_amount(&amount_text(), from(), format);

    rsx! {
        TopBar { title: t!("page.converter").to_string() }
        div { class: "mx-4 flex flex-col gap-4 pt-4 pb-6 safe-area-x",
            ErrorBanner { error: error() }
            div { class: "flex flex-col",
                AmountInput {
                    id: "converter-amount",
                    label: t!("converter.amount").to_string(),
                    currency_label: t!("converter.from").to_string(),
                    value: amount_text(),
                    currency: from(),
                    oninput: move |text| amount_text.set(text),
                    on_currency_click: move |_| picking.set(Some(Side::From)),
                }
                button {
                    class: "relative z-10 -my-3 flex h-11 w-11 items-center justify-center self-center rounded-full border-4 border-jet-black-950 bg-cerulean-600 text-floral-white-50 active:bg-cerulean-700 transition ease-apple active:scale-95",
                    r#type: "button",
                    aria_label: t!("converter.swap").to_string(),
                    onclick: move |_| apply.call((to(), from())),
                    Icon { icon: LdArrowUpDown, class: "h-5 w-5" }
                }
                div { class: "flex min-h-16 items-center gap-3 rounded-2xl border border-jet-black-800 bg-jet-black-900 px-3",
                    CurrencyButton {
                        currency: to(),
                        label: t!("converter.to").to_string(),
                        onclick: move |_| picking.set(Some(Side::To)),
                    }
                    div { class: "min-w-0 flex-1 overflow-x-auto text-right", aria_live: "polite",
                        match &*quote.read() {
                            Ok(Some(quote)) => {
                                let converted = amount
                                    .map(|a| fx::convert(a, &quote.rate))
                                    .unwrap_or(Ok(Money::zero(to())));
                                match converted {
                                    Ok(result) => rsx! {
                                        MoneyText {
                                            amount: result,
                                            class: if amount.is_some() { "text-3xl font-semibold text-floral-white-50" } else { "text-3xl font-semibold text-floral-white-600" },
                                        }
                                    },
                                    Err(_) => rsx! {
                                        span { class: "text-3xl font-semibold text-floral-white-600", "–" }
                                    },
                                }
                            }
                            _ => rsx! {
                                span { class: "text-3xl font-semibold text-floral-white-600", "–" }
                            },
                        }
                    }
                }
            }
            match &*quote.read() {
                Err(message) => rsx! { ErrorBanner { error: Some(message.clone()) } },
                Ok(None) => rsx! { NoRate { from: from(), to: to() } },
                Ok(Some(quote)) if quote.legs.is_empty() => rsx! {
                    p { class: "px-1 text-sm text-floral-white-400", {t!("converter.same_currency").to_string()} }
                },
                Ok(Some(quote)) => rsx! {
                    RateInfo {
                        line: t!(
                            "converter.rate_line",
                            base = quote.rate.base().code(),
                            rate = format_rate(quote.rate.value(), format),
                            quote = quote.rate.quote().code()
                        )
                        .to_string(),
                        rate_date: quote.rate_date.clone().unwrap_or_default(),
                        fetched_at: quote.fetched_at,
                        offline: status.offline(),
                    }
                },
            }
        }
        match picking() {
            Some(side) => rsx! {
                BottomSheet {
                    title: match side {
                        Side::From => t!("converter.from").to_string(),
                        Side::To => t!("converter.to").to_string(),
                    },
                    on_close: move |_| picking.set(None),
                    div { class: "flex max-h-[70vh] flex-col gap-2 overflow-y-auto overscroll-contain px-3 pt-2",
                        CurrencyPicker {
                            selected: if side == Side::From { from() } else { to() },
                            on_select: move |currency| pick(side, currency),
                        }
                    }
                }
            },
            None => rsx! {},
        }
    }
}

/// Rate, its date and age; a warning when it is not current (FX-03).
#[component]
fn RateInfo(line: String, rate_date: String, fetched_at: Option<i64>, offline: bool) -> Element {
    let now = now_ms();
    let stale = rate_is_stale(fetched_at, now, offline);
    let meta = match fetched_at {
        Some(fetched_at) => t!(
            "converter.rate_meta",
            date = display_date(&rate_date),
            age = age_text(fetched_at, now)
        )
        .to_string(),
        None => t!("converter.rate_date", date = display_date(&rate_date)).to_string(),
    };

    rsx! {
        div { class: "flex flex-col gap-2 px-1",
            p { class: "text-base tabular-nums text-floral-white-100", "{line}" }
            p { class: "text-sm text-floral-white-400", "{meta}" }
            if stale {
                div {
                    class: "flex items-start gap-3 rounded-2xl bg-pale-oak-900 px-4 py-3 text-sm text-pale-oak-200",
                    role: "status",
                    Icon { icon: LdTriangleAlert, class: "mt-0.5 h-5 w-5 shrink-0" }
                    span {
                        if offline {
                            {t!("converter.stale_offline").to_string()}
                        } else {
                            {t!("converter.stale_old").to_string()}
                        }
                    }
                }
            }
        }
    }
}

/// Shown when the archive has no rate for the pair yet, e.g. on a first
/// start without internet.
#[component]
fn NoRate(from: Currency, to: Currency) -> Element {
    rsx! {
        div { class: "flex items-start gap-3 rounded-2xl bg-jet-black-900 px-4 py-3",
            Icon { icon: LdCloudOff, class: "mt-0.5 h-5 w-5 shrink-0 text-pale-oak-300" }
            div { class: "flex flex-col gap-1",
                p { class: "text-base font-medium text-floral-white-100", {t!("converter.no_rate_title").to_string()} }
                p { class: "text-sm text-floral-white-400",
                    {t!("converter.no_rate_text", from = from.code(), to = to.code()).to_string()}
                }
            }
        }
    }
}

/// The saved pair, otherwise a foreign favorite into the home currency.
fn initial_pair(db: &Db) -> Result<(Currency, Currency), StorageError> {
    if let (Some(from), Some(to)) = (
        db.currency_setting(CONVERTER_FROM)?,
        db.currency_setting(CONVERTER_TO)?,
    ) {
        return Ok((from, to));
    }
    let home = db
        .profile()?
        .map_or_else(default_home_currency, |profile| profile.home_currency);
    let favorites = db
        .currency_list(FAVORITE_CURRENCY_LIST)?
        .unwrap_or_else(default_favorite_currencies);
    Ok(default_converter_pair(home, &favorites))
}

/// Offline, or fetched more than a day ago. A rate without fetch time
/// (same currency) never is.
fn rate_is_stale(fetched_at: Option<i64>, now: i64, offline: bool) -> bool {
    fetched_at.is_some_and(|fetched_at| offline || now - fetched_at > STALE_AFTER_MS)
}

#[component]
pub fn RateHistory() -> Element {
    rsx! { PlaceholderPage { title: t!("page.rate_history").to_string(), show_back: true } }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use invuso_core::Decimal;
    use invuso_core::fx::Rate;

    use super::*;
    use crate::format::format_money;
    use crate::storage::{NewExchangeRate, Profile};

    const DE: NumberFormat = NumberFormat {
        decimal: ',',
        group: '.',
        symbol_before: false,
    };

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    #[test]
    fn stale_when_offline_or_older_than_a_day() {
        let now = 10 * STALE_AFTER_MS;
        assert!(!rate_is_stale(Some(now - 1000), now, false));
        assert!(rate_is_stale(Some(now - 1000), now, true));
        assert!(rate_is_stale(Some(now - STALE_AFTER_MS - 1), now, false));
        assert!(!rate_is_stale(None, now, true));
    }

    #[test]
    fn thousand_yen_to_euro_rounds_half_to_even() {
        // What the screen does: typed text → archived rate → rounded result.
        let db = Db::open_in_memory().unwrap();
        let rate = Rate::new(cur("EUR"), cur("JPY"), Decimal::from_str("160").unwrap()).unwrap();
        db.archive_rates(
            "frankfurter",
            1,
            &[NewExchangeRate {
                rate,
                rate_date: "2026-10-02".into(),
            }],
        )
        .unwrap();
        let quote = db.latest_rate(cur("JPY"), cur("EUR")).unwrap().unwrap();
        let convert = |text: &str| {
            let amount = parse_amount(text, cur("JPY"), DE).unwrap();
            format_money(fx::convert(amount, &quote.rate).unwrap(), DE)
        };
        // 1 000 / 160 = 6.25 exactly.
        assert_eq!(convert("1000"), "6,25\u{a0}€");
        // Ties go to the even cent: 4 ¥ = 0.025 € → 0.02, 12 ¥ = 0.075 € → 0.08.
        assert_eq!(convert("4"), "0,02\u{a0}€");
        assert_eq!(convert("12"), "0,08\u{a0}€");
        assert_eq!(quote.rate_date.as_deref(), Some("2026-10-02"));
    }

    #[test]
    fn starts_with_saved_pair_or_foreign_favorite() {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&Profile {
            name: "Ich".into(),
            home_currency: cur("CHF"),
            target_language: "de".into(),
        })
        .unwrap();
        assert_eq!(initial_pair(&db).unwrap(), (cur("EUR"), cur("CHF")));
        db.set_setting(CONVERTER_FROM, "JPY").unwrap();
        db.set_setting(CONVERTER_TO, "EUR").unwrap();
        assert_eq!(initial_pair(&db).unwrap(), (cur("JPY"), cur("EUR")));
    }
}
