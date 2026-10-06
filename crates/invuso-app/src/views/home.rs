use std::collections::BTreeMap;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdBanknote, LdChevronRight, LdCircleAlert, LdPin, LdPlus, LdReceipt, LdUsers,
    },
};
use invuso_core::domain::{Category, CategoryId, Group, Money, local_date};

use crate::Route;
use crate::clock;
use crate::components::{
    Button, ButtonVariant, CardSection, EmptyState, ExpenseRow, GroupIcon, MoneyText, OwnBalance,
    TopBar,
};
use crate::preferences::{day_heading, period_text};
use crate::services::cash::{CashValue, cash_value};
use crate::services::receipts;
use crate::services::summary::group_summary;
use crate::state::DataRevision;
use crate::storage::{Db, RecentExpense, StorageError};

/// How many expenses "Letzte Ausgaben" lists (user decision in AP-15).
const RECENT_EXPENSES: u32 = 10;

/// The active group as its card on Home shows it (HOME-02).
#[derive(Debug, Clone, PartialEq)]
struct ActiveGroup {
    group: Group,
    /// Total spent in the base currency (GRP-10).
    total: Money,
    /// Balance of "Ich", if a member or still part of an expense (PER-02).
    own: Option<Money>,
    expense_count: u32,
}

/// Everything Home shows.
#[derive(Debug, Clone, PartialEq)]
struct HomeData {
    active: Option<ActiveGroup>,
    /// Whether any group exists, to tell "none yet" from "none active".
    has_groups: bool,
    recent: Vec<RecentExpense>,
    categories: BTreeMap<CategoryId, Category>,
    /// Cash of "Ich" (HOME-01).
    cash: HomeCash,
}

/// Cash of "Ich" as its card on Home shows it (HOME-01).
#[derive(Debug, Clone, PartialEq)]
struct HomeCash {
    /// Currencies with cash on hand; empty ones are left out.
    balances: Vec<Money>,
    value: CashValue,
}

/// `/`: the active group with total and own balance (HOME-02), the cash of
/// "Ich" (HOME-01) and the latest expenses of all groups (HOME-03). All
/// lead on by tapping.
#[component]
pub fn Home() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let today = use_hook(|| clock::local_now().0);

    let data = use_memo(move || {
        revision.track();
        load(&db).map_err(|e| e.to_string())
    });

    rsx! {
        TopBar { title: t!("page.home").to_string() }
        match &*data.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("home.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(data) => rsx! {
                div { class: "mx-4 flex flex-col gap-5 pt-4 safe-area-x",
                    match data.active.clone() {
                        Some(active) => rsx! { ActiveGroupCard { active } },
                        None => rsx! { NoActiveGroupCard { has_groups: data.has_groups } },
                    }
                    CashCard { cash: data.cash.clone() }
                    RecentExpenses {
                        recent: data.recent.clone(),
                        categories: data.categories.clone(),
                        today: today.clone(),
                    }
                }
            },
        }
    }
}

fn load(db: &Db) -> Result<HomeData, StorageError> {
    let active = match db.active_group()? {
        Some(group) => {
            let summary = group_summary(db, &group)?;
            let own = db
                .me()?
                .and_then(|me| summary.people.get(&me.id).copied())
                .map(|totals| Money::new(totals.balance, group.base_currency));
            Some(ActiveGroup {
                total: summary.total,
                own,
                expense_count: summary.expense_count,
                group,
            })
        }
        None => None,
    };
    let categories = db
        .categories()?
        .into_iter()
        .map(|c| (c.id.clone(), c))
        .collect();
    let cash = match db.me()? {
        Some(me) => {
            let balances: Vec<Money> = db
                .cash_balances(&me.id)?
                .into_iter()
                .filter(|b| !b.is_zero())
                .collect();
            let value = cash_value(db, &balances, db.expense_base_currency(None)?)?;
            HomeCash { balances, value }
        }
        None => HomeCash {
            balances: Vec::new(),
            value: CashValue {
                total: Money::zero(db.expense_base_currency(None)?),
                missing: Vec::new(),
            },
        },
    };
    Ok(HomeData {
        cash,
        active,
        has_groups: !db.groups()?.is_empty(),
        recent: db.recent_expenses(RECENT_EXPENSES)?,
        categories,
    })
}

/// Name, period, total and own balance of the active group; tapping opens
/// its overview.
#[component]
fn ActiveGroupCard(active: ActiveGroup) -> Element {
    let nav = use_navigator();
    let ActiveGroup {
        group,
        total,
        own,
        expense_count,
    } = active;
    let period = period_text(group.start_date.as_deref(), group.end_date.as_deref());
    let id = group.id.as_str().to_string();

    rsx! {
        section { class: "flex flex-col gap-2",
            h2 { class: "flex items-center gap-1.5 px-1 text-sm font-medium text-floral-white-300",
                Icon { icon: LdPin, class: "h-4 w-4" }
                {t!("home.active_group").to_string()}
            }
            button {
                class: "flex w-full flex-col gap-4 rounded-2xl border border-jet-black-800 bg-jet-black-900 px-4 py-4 text-left active:bg-jet-black-800 transition-colors ease-apple",
                r#type: "button",
                onclick: move |_| {
                    nav.push(Route::GroupOverview { id: id.clone() });
                },
                span { class: "flex w-full items-center gap-3",
                    GroupIcon { icon: group.icon.clone(), color: group.color.clone() }
                    span { class: "flex min-w-0 flex-1 flex-col",
                        span { class: "truncate text-lg font-semibold text-floral-white-50", "{group.name}" }
                        if let Some(period) = period {
                            span { class: "truncate text-sm text-floral-white-400", "{period}" }
                        }
                    }
                    Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
                }
                span { class: "flex flex-col gap-1",
                    span { class: "text-sm text-floral-white-400", {t!("summary.total").to_string()} }
                    MoneyText { amount: total, class: "text-3xl font-semibold text-floral-white-50" }
                    if expense_count == 0 {
                        span { class: "text-sm text-floral-white-400", {t!("home.no_group_expenses").to_string()} }
                    } else if let Some(own) = own {
                        OwnBalance { balance: own }
                    }
                }
            }
        }
    }
}

/// Cash per currency and its total value (HOME-01); tapping opens the cash
/// screen.
#[component]
fn CashCard(cash: HomeCash) -> Element {
    let nav = use_navigator();
    // One currency needs no total next to it.
    let show_total = cash.balances.len() > 1
        || cash
            .balances
            .first()
            .is_some_and(|b| b.currency() != cash.value.total.currency());

    rsx! {
        section { class: "flex flex-col gap-2",
            h2 { class: "flex items-center gap-1.5 px-1 text-sm font-medium text-floral-white-300",
                Icon { icon: LdBanknote, class: "h-4 w-4" }
                {t!("cash.home_title").to_string()}
            }
            button {
                class: "flex w-full items-center gap-3 rounded-2xl border border-jet-black-800 bg-jet-black-900 px-4 py-4 text-left active:bg-jet-black-800 transition-colors ease-apple",
                r#type: "button",
                onclick: move |_| {
                    nav.push(Route::Cash { person: String::new() });
                },
                if cash.balances.is_empty() {
                    span { class: "min-w-0 flex-1 text-sm text-floral-white-400", {t!("cash.home_empty").to_string()} }
                } else {
                    span { class: "flex min-w-0 flex-1 flex-col gap-1",
                        for balance in cash.balances.iter().copied() {
                            MoneyText {
                                key: "{balance.currency().code()}",
                                amount: balance,
                                class: if balance.is_negative() { "text-2xl font-semibold text-watermelon-300" } else { "text-2xl font-semibold text-floral-white-50" },
                            }
                        }
                        if show_total {
                            span { class: "text-sm text-floral-white-400",
                                {t!("cash.value").to_string()}
                                " "
                                MoneyText { amount: cash.value.total }
                            }
                        }
                    }
                }
                Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
            }
        }
    }
}

/// Stand-in for the active group: the way to the first group, or to the
/// list to mark one as active (user decision in AP-15).
#[component]
fn NoActiveGroupCard(has_groups: bool) -> Element {
    let nav = use_navigator();
    let (title, text, button) = if has_groups {
        (
            t!("home.no_active_title"),
            t!("home.no_active_text"),
            t!("home.choose_group"),
        )
    } else {
        (
            t!("home.no_groups_title"),
            t!("home.no_groups_text"),
            t!("group.add"),
        )
    };

    rsx! {
        div { class: "flex flex-col items-center gap-2 rounded-2xl border border-jet-black-800 bg-jet-black-900 px-4 py-5 text-center",
            div { class: "mb-1 flex h-12 w-12 items-center justify-center rounded-full bg-jet-black-800 text-cerulean-400",
                if has_groups {
                    Icon { icon: LdPin, class: "h-6 w-6" }
                } else {
                    Icon { icon: LdUsers, class: "h-6 w-6" }
                }
            }
            h2 { class: "text-base font-semibold text-floral-white-100", "{title}" }
            p { class: "max-w-xs text-sm text-floral-white-400", "{text}" }
            Button {
                variant: if has_groups { ButtonVariant::Secondary } else { ButtonVariant::Primary },
                class: "mt-2 w-full",
                onclick: move |_| {
                    if has_groups {
                        nav.push(Route::GroupList {});
                    } else {
                        nav.push(Route::GroupNew {});
                    }
                },
                if !has_groups {
                    Icon { icon: LdPlus, class: "h-5 w-5" }
                }
                "{button}"
            }
        }
    }
}

/// The latest expenses of all groups (HOME-03); tapping one opens its
/// detail.
#[component]
fn RecentExpenses(
    recent: Vec<RecentExpense>,
    categories: BTreeMap<CategoryId, Category>,
    today: String,
) -> Element {
    let nav = use_navigator();

    if recent.is_empty() {
        return rsx! {
            section { class: "flex flex-col gap-2",
                h2 { class: "px-1 text-sm font-medium text-floral-white-300", {t!("home.recent").to_string()} }
                EmptyState {
                    title: t!("home.no_expenses_title").to_string(),
                    text: t!("home.no_expenses_text").to_string(),
                    Icon { icon: LdReceipt, class: "h-8 w-8" }
                }
            }
        };
    }

    rsx! {
        CardSection { title: t!("home.recent").to_string(),
            for expense in recent {
                ExpenseRow {
                    key: "{expense.id.as_str()}",
                    icon: expense
                        .category_id
                        .as_ref()
                        .and_then(|id| categories.get(id))
                        .map(|c| c.icon.clone())
                        .unwrap_or_default(),
                    title: expense.title.clone(),
                    subtitle: recent_subtitle(&expense, &today),
                    total: expense.total,
                    total_in_base: expense.total_in_base,
                    thumbnail: expense.thumbnail_path.as_deref().map(receipts::file_url),
                    onclick: {
                        let id = expense.id.as_str().to_string();
                        move |_| {
                            nav.push(Route::ExpenseDetail { id: id.clone() });
                        }
                    },
                }
            }
        }
    }
}

/// "Japan Reise · Gestern": the group (or "Persönlich") and the day.
fn recent_subtitle(expense: &RecentExpense, today: &str) -> String {
    let group = match &expense.group_name {
        Some(name) => name.clone(),
        None => t!("home.personal").to_string(),
    };
    let day = day_heading(local_date(&expense.occurred_at), today);
    format!("{group} · {day}")
}
