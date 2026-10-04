use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdSearch, LdStar},
};
use invuso_core::domain::Currency;

use crate::components::{ErrorBanner, OptionRow};
use crate::format::currency_symbol;
use crate::preferences::{default_favorite_currencies, toggle_favorite, with_recent};
use crate::storage::{Db, FAVORITE_CURRENCY_LIST, RECENT_CURRENCY_LIST};

/// Currency list with search by code or name, the recently picked ones and
/// favorites on top; the star of each row adds or removes a favorite (UI-15).
/// Every pick counts as recently used, whichever screen the picker is on.
#[component]
pub fn CurrencyPicker(selected: Currency, on_select: EventHandler<Currency>) -> Element {
    let db = use_context::<Db>();
    let mut query = use_signal(String::new);
    let all = use_hook(Currency::selectable);
    let loaded = use_hook(|| {
        let favorites = db.currency_list(FAVORITE_CURRENCY_LIST);
        let recent = db.currency_list(RECENT_CURRENCY_LIST);
        let error = favorites
            .as_ref()
            .err()
            .or(recent.as_ref().err())
            .map(|e| format!("{} {e}", t!("currency_picker.load_error")));
        (
            favorites
                .ok()
                .flatten()
                .unwrap_or_else(default_favorite_currencies),
            recent.ok().flatten().unwrap_or_default(),
            error,
        )
    });
    let mut favorites = use_signal(|| loaded.0.clone());
    let mut error = use_signal(|| loaded.2.clone());
    let recent = loaded.1;

    let pick_db = db.clone();
    let remembered = recent.clone();
    let pick = EventHandler::new(move |currency: Currency| {
        // Remembering the pick is a convenience for the next picker; if it
        // fails, the pick itself still stands and needs no error message.
        let _ =
            pick_db.set_currency_list(RECENT_CURRENCY_LIST, &with_recent(&remembered, currency));
        on_select.call(currency);
    });
    let toggle = EventHandler::new(move |currency: Currency| {
        let updated = toggle_favorite(&favorites.read(), currency);
        match db.set_currency_list(FAVORITE_CURRENCY_LIST, &updated) {
            Ok(()) => {
                favorites.set(updated);
                error.set(None);
            }
            Err(e) => error.set(Some(format!(
                "{} {e}",
                t!("currency_picker.favorite_error")
            ))),
        }
    });

    let query_text = query();
    let searching = !query_text.trim().is_empty();
    let favorite_list = favorites();
    let hits: Vec<Currency> = all
        .iter()
        .filter(|currency| currency.matches(&query_text))
        .copied()
        .collect();
    let recent_only: Vec<Currency> = recent
        .iter()
        .filter(|currency| !favorite_list.contains(currency))
        .copied()
        .collect();

    rsx! {
        div { class: "flex flex-col gap-3",
            label { class: "flex min-h-12 items-center gap-2 rounded-2xl border border-jet-black-700 bg-jet-black-900 px-4 focus-within:border-cerulean-500 transition-colors",
                Icon { icon: LdSearch, class: "h-5 w-5 shrink-0 text-floral-white-400" }
                input {
                    class: "min-w-0 flex-1 bg-transparent text-base text-floral-white-50 placeholder:text-floral-white-600 outline-none",
                    r#type: "search",
                    autocomplete: "off",
                    placeholder: t!("currency_picker.search").to_string(),
                    aria_label: t!("currency_picker.search").to_string(),
                    value: "{query_text}",
                    oninput: move |event| query.set(event.value()),
                }
            }
            ErrorBanner { error: error() }
            div { role: "listbox", class: "flex flex-col gap-1",
                if searching {
                    if hits.is_empty() {
                        p { class: "px-3 py-6 text-center text-sm text-floral-white-400",
                            {t!("currency_picker.no_results").to_string()}
                        }
                    }
                    for currency in hits {
                        CurrencyRow {
                            key: "{currency.code()}",
                            currency,
                            selected,
                            favorite: favorite_list.contains(&currency),
                            on_select: pick,
                            on_toggle_favorite: toggle,
                        }
                    }
                } else {
                    if !recent_only.is_empty() {
                        SectionLabel { text: t!("currency_picker.recent").to_string() }
                        for currency in recent_only {
                            CurrencyRow {
                                key: "recent-{currency.code()}",
                                currency,
                                selected,
                                favorite: false,
                                on_select: pick,
                                on_toggle_favorite: toggle,
                            }
                        }
                    }
                    SectionLabel { text: t!("currency_picker.favorites").to_string() }
                    if favorite_list.is_empty() {
                        p { class: "px-3 pb-2 text-sm text-floral-white-400",
                            {t!("currency_picker.favorites_empty").to_string()}
                        }
                    }
                    for currency in favorite_list.iter().copied() {
                        CurrencyRow {
                            key: "fav-{currency.code()}",
                            currency,
                            selected,
                            favorite: true,
                            on_select: pick,
                            on_toggle_favorite: toggle,
                        }
                    }
                    SectionLabel { text: t!("currency_picker.all").to_string() }
                    for currency in all.iter().copied() {
                        CurrencyRow {
                            key: "{currency.code()}",
                            currency,
                            selected,
                            favorite: favorite_list.contains(&currency),
                            on_select: pick,
                            on_toggle_favorite: toggle,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn CurrencyRow(
    currency: Currency,
    selected: Currency,
    favorite: bool,
    on_select: EventHandler<Currency>,
    on_toggle_favorite: EventHandler<Currency>,
) -> Element {
    let symbol = currency_symbol(currency);
    let detail = (symbol != currency.code()).then(|| symbol.to_string());
    let (star_label, star_class) = if favorite {
        (
            t!("currency_picker.remove_favorite", code = currency.code()),
            "fill-current text-pale-oak-300",
        )
    } else {
        (
            t!("currency_picker.add_favorite", code = currency.code()),
            "text-floral-white-600",
        )
    };

    rsx! {
        div { class: "flex items-center gap-1",
            div { class: "min-w-0 flex-1",
                OptionRow {
                    code: currency.code().to_string(),
                    name: currency.name().to_string(),
                    detail,
                    selected: currency == selected,
                    onclick: move |_| on_select.call(currency),
                }
            }
            button {
                class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full active:bg-jet-black-800 transition-colors",
                r#type: "button",
                aria_label: "{star_label}",
                aria_pressed: if favorite { "true" } else { "false" },
                onclick: move |_| on_toggle_favorite.call(currency),
                Icon { icon: LdStar, class: "h-5 w-5 {star_class}" }
            }
        }
    }
}

#[component]
fn SectionLabel(text: String) -> Element {
    rsx! {
        p { class: "px-3 pt-2 pb-1 text-xs font-semibold uppercase tracking-wide text-floral-white-400",
            "{text}"
        }
    }
}
