use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdSearch};
use invuso_core::domain::Currency;

use crate::components::OptionRow;
use crate::preferences::favorite_currencies;

/// Currency list with search by code or name and favorites on top
/// (simple form of UI-15).
#[component]
pub fn CurrencyPicker(selected: Currency, on_select: EventHandler<Currency>) -> Element {
    let mut query = use_signal(String::new);
    let all = use_hook(Currency::selectable);
    let favorites = use_hook(favorite_currencies);

    let query_text = query();
    let searching = !query_text.trim().is_empty();
    let hits: Vec<Currency> = all
        .iter()
        .filter(|currency| currency.matches(&query_text))
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
            div { role: "listbox", class: "flex flex-col gap-1",
                if searching {
                    if hits.is_empty() {
                        p { class: "px-3 py-6 text-center text-sm text-floral-white-400",
                            {t!("currency_picker.no_results").to_string()}
                        }
                    }
                    for currency in hits {
                        CurrencyRow { key: "{currency.code()}", currency, selected, on_select }
                    }
                } else {
                    SectionLabel { text: t!("currency_picker.favorites").to_string() }
                    for currency in favorites {
                        CurrencyRow { key: "fav-{currency.code()}", currency, selected, on_select }
                    }
                    SectionLabel { text: t!("currency_picker.all").to_string() }
                    for currency in all {
                        CurrencyRow { key: "{currency.code()}", currency, selected, on_select }
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
    on_select: EventHandler<Currency>,
) -> Element {
    rsx! {
        OptionRow {
            code: currency.code().to_string(),
            name: currency.name().to_string(),
            selected: currency == selected,
            onclick: move |_| on_select.call(currency),
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
