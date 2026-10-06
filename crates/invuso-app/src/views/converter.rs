use std::collections::BTreeSet;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdArrowUpDown, LdCalendarDays, LdCloudOff, LdHistory, LdPenLine, LdTriangleAlert, LdX,
    },
};
use invuso_core::domain::{Currency, Money};
use invuso_core::fx::{
    self,
    expression::{Expression, ExpressionError, Key},
};

use crate::Route;
use crate::clock::local_now;
use crate::components::{
    BottomSheet, CardSection, CurrencyButton, CurrencyPicker, ErrorBanner, Keypad, MoneyText,
    TopBar,
};
use crate::format::{NumberFormat, age_text, currency_symbol, expression_text, format_rate};
use crate::preferences::{
    default_converter_pair, default_favorite_currencies, default_home_currency, display_date,
};
use crate::services::converter::{
    archived_quote, clear_manual_rate, day_missing, fetch_day, manual_quote, selected_manual_rate,
};
use crate::state::{DataRevision, RateStatus};
use crate::storage::{
    CONVERTER_FROM, CONVERTER_TO, Db, FAVORITE_CURRENCY_LIST, RateQuote, StorageError, now_ms,
};

mod history;
mod manual_rate;

pub use history::RateHistory;
use manual_rate::ManualRateSheet;

/// A rate fetched longer ago than this gets the warning (FX-03); the app
/// refreshes once a day while online.
const STALE_AFTER_MS: i64 = 24 * 60 * 60 * 1000;

/// Which side of the converter the currency picker is choosing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    From,
    To,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sheet {
    Pick(Side),
    ManualRate,
}

/// The rates the converter shows for one pair and day.
#[derive(Debug, Clone, PartialEq)]
struct Rates {
    /// `from → to`; the manual rate while one is selected for this pair.
    main: Option<RateQuote>,
    manual: bool,
    /// The other favorites with their archived rate `from → currency`
    /// (FX-07).
    others: Vec<(Currency, Option<RateQuote>)>,
}

/// Currency converter (FX-05..07, FX-09, FX-10): an amount or a sum typed
/// on the keypad, converted into the target currency and every other
/// favorite with the newest archived rate, the rate of a chosen day or a
/// manual rate. The last pair is remembered.
#[component]
pub fn Converter() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let status = use_context::<RateStatus>();
    let nav = use_navigator();
    let initial = use_hook(|| {
        initial_pair(&db)
            .and_then(|pair| Ok((pair, selected_manual_rate(&db)?)))
            .map_err(|e| e.to_string())
    });
    let ((start_from, start_to), start_manual) = initial.clone().unwrap_or_else(|_| {
        let home = default_home_currency();
        (
            default_converter_pair(home, &default_favorite_currencies()),
            None,
        )
    });
    let mut from = use_signal(|| start_from);
    let mut to = use_signal(|| start_to);
    let mut manual_id = use_signal(|| start_manual);
    let mut expression = use_signal(Expression::new);
    let mut day = use_signal(|| local_now().0);
    let mut sheet = use_signal(|| None::<Sheet>);
    let mut error = use_signal(|| {
        initial
            .err()
            .map(|e| format!("{} {e}", t!("converter.load_error")))
    });
    let mut fetching = use_signal(|| false);
    let mut failed_day = use_signal(|| None::<String>);
    let mut tried_days = use_signal(BTreeSet::<String>::new);

    let rates_db = db.clone();
    let rates = use_memo(move || {
        revision.track();
        let chosen = Some(day()).filter(|d| *d != local_now().0);
        load_rates(
            &rates_db,
            from(),
            to(),
            chosen.as_deref(),
            manual_id().as_deref(),
        )
        .map_err(|e| format!("{} {e}", t!("converter.rate_error")))
    });

    // A past day without archived rates is fetched once (FX-09); while
    // offline the closest earlier rate stays in use.
    let fetch_db = db.clone();
    use_effect(move || {
        let day = day();
        let missing = match &*rates.read() {
            Ok(rates) => !rates.manual && day_missing(rates.main.as_ref(), &day),
            Err(_) => false,
        };
        if day == local_now().0 || !missing || tried_days.peek().contains(&day) {
            return;
        }
        tried_days.write().insert(day.clone());
        fetching.set(true);
        let worker_db = fetch_db.clone();
        let mut revision = revision;
        spawn(async move {
            let worker_day = day.clone();
            // Network calls block; keep them off the UI thread.
            let fetched =
                tokio::task::spawn_blocking(move || fetch_day(&worker_db, &worker_day)).await;
            fetching.set(false);
            match fetched {
                Ok(Ok(true)) => revision.bump(),
                _ => failed_day.set(Some(day)),
            }
        });
    });

    // A `Callback` is `Copy`, so the swap button, the pickers and the
    // favorites can share it.
    let pair_db = db.clone();
    let apply = use_callback(move |(new_from, new_to): (Currency, Currency)| {
        let fitted = expression.peek().fit(new_from);
        expression.set(fitted);
        from.set(new_from);
        to.set(new_to);
        error.set(
            save_pair(&pair_db, new_from, new_to)
                .err()
                .map(|e| format!("{} {e}", t!("converter.save_error"))),
        );
    });
    let mut pick = move |side: Side, currency: Currency| {
        sheet.set(None);
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
    let clear_db = db.clone();
    let mut use_archived = move || {
        manual_id.set(None);
        error.set(
            clear_manual_rate(&clear_db)
                .err()
                .map(|e| format!("{} {e}", t!("converter.save_error"))),
        );
    };

    let format = NumberFormat::current();
    let today = local_now().0;
    let amount = expression().amount(from());
    let typed = expression_text(&expression(), format);
    let has_operator = expression().has_operator();
    let size = match typed.chars().count() {
        0..=12 => "text-3xl",
        13..=18 => "text-2xl",
        _ => "text-xl",
    };
    let symbol = currency_symbol(from());
    let convert = |quote: &RateQuote| -> Option<Money> {
        let base = amount
            .clone()
            .ok()?
            .unwrap_or(Money::zero(quote.rate.base()));
        fx::convert(base, &quote.rate).ok()
    };
    let has_amount = matches!(amount, Ok(Some(_)));
    let result_color = if has_amount {
        "text-floral-white-50"
    } else {
        "text-floral-white-600"
    };

    rsx! {
        TopBar { title: t!("page.converter").to_string(),
            button {
                class: "flex h-11 w-11 items-center justify-center rounded-full text-floral-white-200 active:bg-jet-black-800 transition-colors",
                r#type: "button",
                aria_label: t!("converter.history").to_string(),
                onclick: move |_| {
                    nav.push(Route::RateHistory {});
                },
                Icon { icon: LdHistory, class: "h-6 w-6" }
            }
        }
        div { class: "mx-4 flex flex-col gap-3 pt-4 pb-6 safe-area-x",
            ErrorBanner { error: error() }
            div { class: "flex flex-col",
                div { class: "flex min-h-16 items-center gap-3 rounded-2xl border border-jet-black-700 bg-jet-black-900 px-3 py-2",
                    CurrencyButton {
                        currency: from(),
                        label: t!("converter.from").to_string(),
                        onclick: move |_| sheet.set(Some(Sheet::Pick(Side::From))),
                    }
                    div {
                        class: "flex min-w-0 flex-1 flex-col items-end",
                        aria_label: t!("converter.amount").to_string(),
                        aria_live: "polite",
                        // The end of a long sum stays visible; the start is cut off.
                        div { class: "flex w-full items-baseline justify-end gap-1 overflow-hidden whitespace-nowrap {size} font-semibold tabular-nums",
                            if format.symbol_before {
                                span { class: "text-floral-white-300", "{symbol}" }
                            }
                            if typed.is_empty() {
                                span { class: "text-floral-white-600", "0" }
                            } else {
                                span { class: "text-floral-white-50", "{typed}" }
                            }
                            if !format.symbol_before {
                                span { class: "text-floral-white-300", "{symbol}" }
                            }
                        }
                        match (&amount, has_operator) {
                            (Err(ExpressionError::DivisionByZero), _) => rsx! {
                                span { class: "text-sm text-watermelon-300", role: "alert", {t!("converter.division_by_zero").to_string()} }
                            },
                            (Err(_), _) => rsx! {
                                span { class: "text-sm text-watermelon-300", role: "alert", "–" }
                            },
                            (Ok(Some(sum)), true) => rsx! {
                                MoneyText { amount: *sum, class: "text-sm text-floral-white-400" }
                            },
                            _ => rsx! {},
                        }
                    }
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
                        onclick: move |_| sheet.set(Some(Sheet::Pick(Side::To))),
                    }
                    div { class: "min-w-0 flex-1 overflow-x-auto text-right", aria_live: "polite",
                        match rates.read().as_ref().ok().and_then(|r| r.main.as_ref()).and_then(convert) {
                            Some(result) => rsx! {
                                MoneyText { amount: result, class: "text-3xl font-semibold {result_color}" }
                            },
                            None => rsx! {
                                span { class: "text-3xl font-semibold text-floral-white-600", "–" }
                            },
                        }
                    }
                }
            }
            match &*rates.read() {
                Err(message) => rsx! { ErrorBanner { error: Some(message.clone()) } },
                Ok(rates) => rsx! {
                    RateInfo {
                        rates: rates.clone(),
                        from: from(),
                        to: to(),
                        day: day(),
                        today: today.clone(),
                        offline: status.offline(),
                        fetching: fetching(),
                        failed: failed_day().as_deref() == Some(day().as_str()),
                    }
                },
            }
            div { class: "flex flex-wrap items-center gap-2",
                if rates.read().as_ref().is_ok_and(|r| r.manual) {
                    Chip {
                        label: t!("converter.use_archived").to_string(),
                        onclick: move |_| use_archived(),
                        Icon { icon: LdX, class: "h-4 w-4" }
                    }
                } else {
                    label { class: "relative flex min-h-11 items-center gap-2 rounded-full bg-jet-black-800 px-4 text-sm font-medium text-floral-white-100 active:bg-jet-black-700 transition-colors",
                        Icon { icon: LdCalendarDays, class: "h-4 w-4 text-cerulean-300" }
                        if day() == today {
                            {t!("converter.today").to_string()}
                        } else {
                            {display_date(&day())}
                        }
                        // Invisible over the chip: a tap opens the system date
                        // picker of the WebView.
                        input {
                            class: "absolute inset-0 h-full w-full cursor-pointer opacity-0",
                            r#type: "date",
                            aria_label: t!("converter.day").to_string(),
                            value: "{day}",
                            max: "{today}",
                            oninput: move |event| {
                                let picked = event.value();
                                let today = local_now().0;
                                day.set(if picked.is_empty() || picked > today { today } else { picked });
                            },
                        }
                    }
                    if day() != today {
                        button {
                            class: "flex h-11 w-11 items-center justify-center rounded-full text-floral-white-400 active:bg-jet-black-800 transition-colors",
                            r#type: "button",
                            aria_label: t!("converter.back_to_today").to_string(),
                            onclick: move |_| day.set(local_now().0),
                            Icon { icon: LdX, class: "h-5 w-5" }
                        }
                    }
                    if from() != to() {
                        Chip {
                            label: t!("converter.manual").to_string(),
                            onclick: move |_| sheet.set(Some(Sheet::ManualRate)),
                            Icon { icon: LdPenLine, class: "h-4 w-4 text-cerulean-300" }
                        }
                    }
                }
            }
            Keypad {
                on_key: move |key: Key| {
                    let next = expression.peek().press(key, from());
                    expression.set(next);
                },
                decimal: from().exponent() > 0,
                decimal_label: format.decimal.to_string(),
            }
            if let Ok(rates) = &*rates.read() {
                if !rates.others.is_empty() {
                    CardSection { title: t!("converter.more_currencies").to_string(),
                        for (currency, quote) in rates.others.clone() {
                            button {
                                key: "{currency.code()}",
                                class: "flex min-h-12 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
                                r#type: "button",
                                onclick: move |_| apply.call((from(), currency)),
                                span { class: "w-12 shrink-0 text-sm font-semibold tabular-nums text-cerulean-300", "{currency.code()}" }
                                span { class: "min-w-0 flex-1 truncate text-sm text-floral-white-400", "{currency.name()}" }
                                match quote.as_ref().and_then(convert) {
                                    Some(converted) => rsx! {
                                        MoneyText { amount: converted, class: "shrink-0 text-base font-semibold {result_color}" }
                                    },
                                    None => rsx! {
                                        span { class: "shrink-0 text-sm text-floral-white-500", {t!("converter.no_rate_short").to_string()} }
                                    },
                                }
                            }
                        }
                    }
                    if rates.manual {
                        p { class: "px-1 text-sm text-floral-white-400", {t!("converter.manual_other_hint").to_string()} }
                    }
                }
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
                    reference: rates.read().as_ref().ok().and_then(|r| r.main.as_ref()).map(|q| q.rate),
                    on_saved: move |id| {
                        manual_id.set(Some(id));
                        sheet.set(None);
                    },
                    on_close: move |_| sheet.set(None),
                }
            },
            None => rsx! {},
        }
    }
}

/// Small rounded action next to the rate; `children` is its icon.
#[component]
fn Chip(label: String, onclick: EventHandler<()>, children: Element) -> Element {
    rsx! {
        button {
            class: "flex min-h-11 items-center gap-2 rounded-full bg-jet-black-800 px-4 text-sm font-medium text-floral-white-100 active:bg-jet-black-700 transition-colors",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            {children}
            "{label}"
        }
    }
}

/// Rate, its date and age, and what the user should know about it: an old
/// rate or offline (FX-03), a past day without its own rate (FX-09), a
/// manual rate (FX-10).
#[component]
fn RateInfo(
    rates: Rates,
    from: Currency,
    to: Currency,
    day: String,
    today: String,
    offline: bool,
    fetching: bool,
    failed: bool,
) -> Element {
    let format = NumberFormat::current();
    let Some(quote) = rates.main else {
        if fetching {
            return rsx! { Note { text: t!("converter.loading_day", date = display_date(&day)).to_string() } };
        }
        return rsx! { NoRate { from, to } };
    };
    if quote.legs.is_empty() {
        return rsx! {
            p { class: "px-1 text-sm text-floral-white-400", {t!("converter.same_currency").to_string()} }
        };
    }
    let line = t!(
        "converter.rate_line",
        base = quote.rate.base().code(),
        rate = format_rate(quote.rate.value(), format),
        quote = quote.rate.quote().code()
    )
    .to_string();
    let rate_date = quote.rate_date.clone().unwrap_or_default();
    let now = now_ms();
    let meta = match (rates.manual, quote.fetched_at) {
        (true, _) => t!("converter.manual_meta", date = display_date(&rate_date)).to_string(),
        (false, Some(fetched_at)) => t!(
            "converter.rate_meta",
            date = display_date(&rate_date),
            age = age_text(fetched_at, now)
        )
        .to_string(),
        (false, None) => t!("converter.rate_date", date = display_date(&rate_date)).to_string(),
    };
    let current_day = day == today;
    let warning = if rates.manual {
        None
    } else if current_day && rate_is_stale(quote.fetched_at, now, offline) {
        Some(if offline {
            t!("converter.stale_offline").to_string()
        } else {
            t!("converter.stale_old").to_string()
        })
    } else if !current_day && fetching {
        Some(t!("converter.loading_day", date = display_date(&day)).to_string())
    } else if !current_day && day_missing(Some(&quote), &day) {
        let older = t!(
            "converter.day_older",
            date = display_date(&day),
            rate_date = display_date(&rate_date)
        );
        Some(if failed {
            format!(
                "{} {older}",
                t!("converter.day_fetch_failed", date = display_date(&day))
            )
        } else {
            older.to_string()
        })
    } else {
        None
    };

    rsx! {
        div { class: "flex flex-col gap-1 px-1",
            p { class: "text-base tabular-nums text-floral-white-100", "{line}" }
            p { class: "text-sm text-floral-white-400", "{meta}" }
            if let Some(warning) = warning {
                Note { text: warning }
            }
        }
    }
}

/// Yellow hint about the rate (`pale-oak`, idee.md 3.2).
#[component]
fn Note(text: String) -> Element {
    rsx! {
        div {
            class: "mt-1 flex items-start gap-3 rounded-2xl bg-pale-oak-900 px-4 py-3 text-sm text-pale-oak-200",
            role: "status",
            Icon { icon: LdTriangleAlert, class: "mt-0.5 h-5 w-5 shrink-0" }
            span { "{text}" }
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

/// The rates for the pair on `day` (`None` = newest), the manual rate
/// `manual_id` instead if it belongs to this pair, and the other favorites.
fn load_rates(
    db: &Db,
    from: Currency,
    to: Currency,
    day: Option<&str>,
    manual_id: Option<&str>,
) -> Result<Rates, StorageError> {
    let manual = match manual_id {
        Some(id) => manual_quote(db, id, from, to)?,
        None => None,
    };
    let main = match &manual {
        Some(quote) => Some(quote.clone()),
        None => archived_quote(db, from, to, day)?,
    };
    let favorites = db
        .currency_list(FAVORITE_CURRENCY_LIST)?
        .unwrap_or_else(default_favorite_currencies);
    let others = favorites
        .into_iter()
        .filter(|currency| *currency != from && *currency != to)
        .map(|currency| Ok((currency, archived_quote(db, from, currency, day)?)))
        .collect::<Result<_, StorageError>>()?;
    Ok(Rates {
        main,
        manual: manual.is_some(),
        others,
    })
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

/// Remembers the pair for the next visit; the rate history shares it.
fn save_pair(db: &Db, from: Currency, to: Currency) -> Result<(), StorageError> {
    db.set_setting(CONVERTER_FROM, from.code())?;
    db.set_setting(CONVERTER_TO, to.code())
}

/// Offline, or fetched more than a day ago. A rate without fetch time
/// (same currency) never is.
fn rate_is_stale(fetched_at: Option<i64>, now: i64, offline: bool) -> bool {
    fetched_at.is_some_and(|fetched_at| offline || now - fetched_at > STALE_AFTER_MS)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use invuso_core::Decimal;
    use invuso_core::fx::Rate;
    use invuso_core::fx::expression::Operator;

    use super::*;
    use crate::format::format_money;
    use crate::services::converter::save_manual_rate;
    use crate::storage::{NewExchangeRate, Profile};

    const DE: NumberFormat = NumberFormat {
        decimal: ',',
        group: '.',
        symbol_before: false,
    };

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn eur_to(quote: &str, value: &str, date: &str) -> NewExchangeRate {
        NewExchangeRate {
            rate: Rate::new(cur("EUR"), cur(quote), Decimal::from_str(value).unwrap()).unwrap(),
            rate_date: date.into(),
        }
    }

    fn typed(keys: &str, currency: Currency) -> Expression {
        keys.chars().fold(Expression::new(), |e, c| {
            let key = match c {
                '0'..='9' => Key::Digit(c as u8 - b'0'),
                '.' => Key::Decimal,
                _ => Key::Operator(Operator::Add),
            };
            e.press(key, currency)
        })
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
        // What the screen does: keypad → archived rate → rounded result.
        let db = Db::open_in_memory().unwrap();
        db.archive_rates("frankfurter", 1, &[eur_to("JPY", "160", "2026-10-02")])
            .unwrap();
        let quote = db.latest_rate(cur("JPY"), cur("EUR")).unwrap().unwrap();
        let convert = |keys: &str| {
            let amount = typed(keys, cur("JPY")).amount(cur("JPY")).unwrap().unwrap();
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
    fn a_yen_sum_shows_in_euro_and_dollar_at_once() {
        // Acceptance of AP-24: `1200+850` ¥ in EUR and USD.
        let db = Db::open_in_memory().unwrap();
        db.archive_rates(
            "frankfurter",
            1,
            &[
                eur_to("JPY", "160", "2026-10-02"),
                eur_to("USD", "1.10", "2026-10-02"),
            ],
        )
        .unwrap();
        db.set_currency_list(
            FAVORITE_CURRENCY_LIST,
            &[cur("EUR"), cur("USD"), cur("JPY")],
        )
        .unwrap();
        let rates = load_rates(&db, cur("JPY"), cur("EUR"), None, None).unwrap();
        let amount = typed("1200+850", cur("JPY"))
            .amount(cur("JPY"))
            .unwrap()
            .unwrap();
        assert_eq!(amount, Money::new(2050, cur("JPY")));

        let eur = fx::convert(amount, &rates.main.unwrap().rate).unwrap();
        // 2 050 / 160 = 12.8125 → 12.81 €.
        assert_eq!(eur, Money::new(1281, cur("EUR")));
        assert_eq!(rates.others.len(), 1);
        let (usd_currency, usd_quote) = &rates.others[0];
        assert_eq!(*usd_currency, cur("USD"));
        let usd = fx::convert(amount, &usd_quote.as_ref().unwrap().rate).unwrap();
        // 12.8125 × 1.10 = 14.09375 → 14.09 $.
        assert_eq!(usd, Money::new(1409, cur("USD")));
    }

    #[test]
    fn a_manual_rate_replaces_only_the_main_rate_of_its_pair() {
        let db = Db::open_in_memory().unwrap();
        db.archive_rates(
            "frankfurter",
            1,
            &[
                eur_to("JPY", "160", "2026-10-02"),
                eur_to("USD", "1.10", "2026-10-02"),
            ],
        )
        .unwrap();
        let id = save_manual_rate(
            &db,
            cur("EUR"),
            cur("JPY"),
            Decimal::from(150),
            "2026-10-06",
        )
        .unwrap();
        let rates = load_rates(&db, cur("JPY"), cur("EUR"), None, Some(&id)).unwrap();
        assert!(rates.manual);
        let main = rates.main.unwrap();
        assert_eq!(
            fx::convert(Money::new(1500, cur("JPY")), &main.rate).unwrap(),
            Money::new(1000, cur("EUR"))
        );
        // Other favorites keep the fetched rate (via EUR at 160).
        let usd = rates
            .others
            .iter()
            .find(|(c, _)| *c == cur("USD"))
            .and_then(|(_, q)| q.clone())
            .unwrap();
        assert_eq!(usd.legs.len(), 2);
        // Another pair ignores it.
        let other = load_rates(&db, cur("JPY"), cur("USD"), None, Some(&id)).unwrap();
        assert!(!other.manual);
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
        save_pair(&db, cur("JPY"), cur("EUR")).unwrap();
        assert_eq!(initial_pair(&db).unwrap(), (cur("JPY"), cur("EUR")));
    }
}
