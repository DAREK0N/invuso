//! The group overview's breakdowns: by category (GRP-16) and by payment
//! method and kind (GRP-17).

use std::collections::BTreeMap;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdChevronRight, LdCircleHelp},
};
use invuso_core::domain::{
    Category, CategoryId, Currency, GroupId, Money, PaymentMethod, PaymentMethodId,
    PaymentMethodKind, Person, PersonId,
};
use invuso_core::split::{Breakdown, PaymentKey, Slice};

use super::settle::person_label;
use crate::Route;
use crate::components::{
    CardSection, CategoryIcon, ChartSlice, DonutChart, MoneyText, PaymentIconGlyph,
    PaymentMethodIcon, ShareBar, percent_text,
};
use crate::preferences::{category_name, default_payment_icon, payment_kind_name};
use crate::services::timeline::TimelineFilter;
use crate::state::TimelineFilters;

/// Color of the part without a category or method: neutral, so it does
/// not read as one of them.
const NEUTRAL: &str = "floral-white";

/// Donut and list of what each category cost (GRP-16). Tapping a category
/// opens the timeline filtered to it.
#[component]
pub fn CategoryBreakdown(
    group_id: GroupId,
    breakdown: Breakdown<Option<CategoryId>>,
    categories: BTreeMap<CategoryId, Category>,
) -> Element {
    let nav = use_navigator();
    let mut filters = use_context::<TimelineFilters>();
    let total = breakdown.total;
    let category = |key: &Option<CategoryId>| key.as_ref().and_then(|id| categories.get(id));
    let slices: Vec<ChartSlice> = breakdown
        .slices
        .iter()
        .map(|slice| ChartSlice {
            key: slice_key(&slice.key),
            amount_minor: slice.amount_minor,
            color: category(&slice.key).map_or(NEUTRAL.to_string(), |c| c.color.clone()),
        })
        .collect();

    rsx! {
        CardSection { title: t!("breakdown.by_category").to_string(),
            div { class: "border-b border-jet-black-800 px-4 py-5",
                DonutChart { slices, label: t!("breakdown.category_chart").to_string(),
                    MoneyText { amount: total, class: "text-lg font-semibold text-floral-white-50" }
                }
            }
            for slice in breakdown.slices.iter().cloned() {
                {
                    let found = category(&slice.key).cloned();
                    let (name, icon, color) = match &found {
                        Some(category) => (category_name(category), category.icon.clone(), category.color.clone()),
                        None => (t!("breakdown.no_category").to_string(), "tag".to_string(), NEUTRAL.to_string()),
                    };
                    let onclick = slice.key.clone().map(|id| {
                        let group_id = group_id.clone();
                        EventHandler::new(move |_: MouseEvent| {
                            filters.update(&group_id, |filter| {
                                *filter = TimelineFilter::default();
                                filter.categories = [id.clone()].into();
                            });
                            nav.push(Route::GroupTimeline { id: group_id.as_str().to_string() });
                        })
                    });
                    rsx! {
                        ShareRow {
                            key: "{slice_key(&slice.key)}",
                            title: name,
                            detail: count_text(&slice, "expenses"),
                            amount: Money::new(slice.amount_minor, total.currency()),
                            total_minor: total.amount_minor(),
                            color: color.clone(),
                            onclick,
                            if found.is_some() {
                                CategoryIcon { icon, color }
                            } else {
                                NeutralBadge {
                                    Icon { icon: LdCircleHelp, class: "h-5 w-5" }
                                }
                            }
                        }
                    }
                }
            }
        }
        p { class: "-mt-3 px-1 text-sm text-floral-white-500", {t!("breakdown.category_hint").to_string()} }
    }
}

/// Bars per kind of method and a list of who paid with what (GRP-17);
/// tapping a method opens its evaluation (PAY-05).
#[component]
pub fn MethodBreakdown(
    breakdown: Breakdown<PaymentKey>,
    methods: BTreeMap<PaymentMethodId, PaymentMethod>,
    people: BTreeMap<PersonId, Person>,
) -> Element {
    let nav = use_navigator();
    let total = breakdown.total;
    let currency = total.currency();
    let kinds = by_kind(&breakdown, &methods);

    rsx! {
        CardSection { title: t!("breakdown.by_method").to_string(),
            if kinds.slices.len() > 1 {
                SubHeading { text: t!("breakdown.by_kind").to_string() }
                for slice in kinds.slices.iter() {
                    KindRow { key: "{slice.key:?}", slice: slice.clone(), currency, total_minor: total.amount_minor() }
                }
                SubHeading { text: t!("breakdown.per_method").to_string() }
            }
            for slice in breakdown.slices.iter().cloned() {
                {
                    let method = slice.key.method.as_ref().and_then(|id| methods.get(id)).cloned();
                    let (person, _) = person_label(people.get(&slice.key.person));
                    let detail = format!("{person} · {}", count_text(&slice, "payments"));
                    let onclick = method.as_ref().map(|method| {
                        let id = method.id.as_str().to_string();
                        EventHandler::new(move |_: MouseEvent| {
                            nav.push(Route::PaymentMethodDetail { id: id.clone() });
                        })
                    });
                    rsx! {
                        ShareRow {
                            key: "{slice.key.person.as_str()}/{slice.key.method.as_ref().map(|m| m.as_str()).unwrap_or_default()}",
                            title: method.as_ref().map_or(t!("breakdown.no_method").to_string(), |m| m.name.clone()),
                            detail,
                            amount: Money::new(slice.amount_minor, currency),
                            total_minor: total.amount_minor(),
                            color: method.as_ref().map_or(NEUTRAL.to_string(), |m| m.color.clone()),
                            onclick,
                            match &method {
                                Some(method) => rsx! { PaymentMethodIcon { icon: method.icon.clone(), color: method.color.clone() } },
                                None => rsx! {
                                    NeutralBadge {
                                        Icon { icon: LdCircleHelp, class: "h-5 w-5" }
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The payments merged by kind of method, in the order of
/// [`PaymentMethodKind::ALL`] on ties; `None` for payments without one.
fn by_kind(
    breakdown: &Breakdown<PaymentKey>,
    methods: &BTreeMap<PaymentMethodId, PaymentMethod>,
) -> Breakdown<Option<usize>> {
    let kind_of = |key: &PaymentKey| {
        let kind = key.method.as_ref().and_then(|id| methods.get(id))?.kind;
        PaymentMethodKind::ALL.iter().position(|k| *k == kind)
    };
    // Merging parts of a valid breakdown cannot overflow its total.
    breakdown.regroup(kind_of).unwrap_or(Breakdown {
        total: breakdown.total,
        slices: Vec::new(),
    })
}

/// One kind with its share as a bar.
#[component]
fn KindRow(slice: Slice<Option<usize>>, currency: Currency, total_minor: i64) -> Element {
    let kind = slice
        .key
        .and_then(|index| PaymentMethodKind::ALL.get(index).copied());
    let name = kind.map_or(t!("breakdown.no_method").to_string(), payment_kind_name);
    rsx! {
        div { class: "flex min-h-14 items-center gap-3 px-4 py-2",
            NeutralBadge {
                match kind {
                    Some(kind) => rsx! { PaymentIconGlyph { icon: default_payment_icon(kind).to_string() } },
                    None => rsx! { Icon { icon: LdCircleHelp, class: "h-5 w-5" } },
                }
            }
            span { class: "flex min-w-0 flex-1 flex-col gap-1.5",
                span { class: "flex items-baseline gap-2",
                    span { class: "min-w-0 flex-1 truncate text-base text-floral-white-50", "{name}" }
                    span { class: "shrink-0 text-sm tabular-nums text-floral-white-400",
                        {percent_text(slice.amount_minor, total_minor)}
                    }
                }
                ShareBar { amount_minor: slice.amount_minor, total_minor, color: "cerulean".to_string() }
            }
            MoneyText { amount: Money::new(slice.amount_minor, currency), class: "w-28 shrink-0 text-right text-base font-semibold text-floral-white-100" }
            span { class: "w-5 shrink-0" }
        }
    }
}

/// Icon, title, detail line with share, bar and amount; a button when
/// `onclick` is set. `children` is the icon.
#[component]
fn ShareRow(
    title: String,
    detail: String,
    amount: Money,
    total_minor: i64,
    color: String,
    onclick: Option<EventHandler<MouseEvent>>,
    children: Element,
) -> Element {
    let percent = percent_text(amount.amount_minor(), total_minor);
    let content = rsx! {
        {children}
        span { class: "flex min-w-0 flex-1 flex-col gap-1",
            span { class: "truncate text-base text-floral-white-50", "{title}" }
            span { class: "truncate text-sm tabular-nums text-floral-white-400", "{detail} · {percent}" }
            ShareBar { amount_minor: amount.amount_minor(), total_minor, color }
        }
        // Fixed width, so the bars of all rows end at the same place.
        MoneyText { amount, class: "w-28 shrink-0 text-right text-base font-semibold text-floral-white-100" }
    };
    let row = "flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0";
    match onclick {
        Some(onclick) => rsx! {
            button {
                class: "{row} active:bg-jet-black-800 transition-colors ease-apple",
                r#type: "button",
                onclick: move |event| onclick.call(event),
                {content}
                Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
            }
        },
        None => rsx! {
            div { class: "{row}",
                {content}
                // Lines up with the chevron of the rows that open something.
                span { class: "w-5 shrink-0" }
            }
        },
    }
}

/// Round badge without a color of its own.
#[component]
fn NeutralBadge(children: Element) -> Element {
    rsx! {
        span {
            class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-jet-black-800 text-floral-white-300",
            aria_hidden: "true",
            {children}
        }
    }
}

#[component]
fn SubHeading(text: String) -> Element {
    rsx! {
        h3 { class: "border-b border-jet-black-800 px-4 pb-2 pt-3 text-sm font-medium text-floral-white-400", "{text}" }
    }
}

fn slice_key(key: &Option<CategoryId>) -> String {
    key.as_ref()
        .map_or(String::new(), |id| id.as_str().to_string())
}

/// "3 Ausgaben" / "1 Zahlung"; `noun` is `expenses` or `payments`.
fn count_text<K>(slice: &Slice<K>, noun: &str) -> String {
    match (noun, slice.count) {
        ("expenses", 1) => t!("breakdown.expenses_one").to_string(),
        ("expenses", count) => t!("breakdown.expenses_other", count = count).to_string(),
        (_, 1) => t!("breakdown.payments_one").to_string(),
        (_, count) => t!("breakdown.payments_other", count = count).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method(id: &str, kind: PaymentMethodKind) -> (PaymentMethodId, PaymentMethod) {
        let id = PaymentMethodId::new(id);
        let method = PaymentMethod {
            id: id.clone(),
            name: id.as_str().into(),
            kind,
            owner_person_id: None,
            last4: None,
            color: "cerulean".into(),
            icon: "credit-card".into(),
            archived: false,
        };
        (id, method)
    }

    fn slice(person: &str, method: Option<&str>, amount_minor: i64) -> Slice<PaymentKey> {
        Slice {
            key: PaymentKey {
                person: PersonId::new(person),
                method: method.map(PaymentMethodId::new),
            },
            amount_minor,
            count: 1,
        }
    }

    #[test]
    fn kinds_merge_methods_and_keep_the_total() {
        let methods: BTreeMap<_, _> = [
            method("visa", PaymentMethodKind::CreditCard),
            method("amex", PaymentMethodKind::CreditCard),
            method("cash", PaymentMethodKind::Cash),
        ]
        .into();
        let breakdown = Breakdown {
            total: Money::new(10_000, Currency::from_code("EUR").unwrap()),
            slices: vec![
                slice("anna", Some("visa"), 4_000),
                slice("ben", Some("cash"), 3_500),
                slice("ben", Some("amex"), 1_500),
                slice("ben", None, 1_000),
            ],
        };
        let kinds = by_kind(&breakdown, &methods);
        let cash = PaymentMethodKind::ALL
            .iter()
            .position(|k| *k == PaymentMethodKind::Cash);
        let card = PaymentMethodKind::ALL
            .iter()
            .position(|k| *k == PaymentMethodKind::CreditCard);
        let parts: Vec<(Option<usize>, i64, u32)> = kinds
            .slices
            .iter()
            .map(|s| (s.key, s.amount_minor, s.count))
            .collect();
        assert_eq!(
            parts,
            [(card, 5_500, 2), (cash, 3_500, 1), (None, 1_000, 1)]
        );
        assert_eq!(
            kinds.slices.iter().map(|s| s.amount_minor).sum::<i64>(),
            kinds.total.amount_minor()
        );
    }
}
