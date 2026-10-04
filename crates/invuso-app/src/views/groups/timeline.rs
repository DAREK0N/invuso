use std::collections::BTreeMap;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdChevronDown, LdChevronRight, LdCircleAlert, LdPlus, LdReceipt},
};
use invuso_core::domain::{Category, CategoryId, GroupId, Money, MoneyError, local_date};

use super::form::GroupNotFound;
use crate::Route;
use crate::clock;
use crate::components::{Button, CategoryIconGlyph, EmptyState, MoneyText, TopBar};
use crate::preferences::day_heading;
use crate::state::DataRevision;
use crate::storage::{Db, TimelineEntry, TimelinePayer};

/// Days rendered at first and added each time the end of the list comes
/// into view, so long trips stay smooth (AP-13 step 4).
const DAYS_PER_PAGE: usize = 20;

/// One day of the timeline (GRP-20).
#[derive(Debug, Clone, PartialEq)]
struct TimelineDay {
    /// `YYYY-MM-DD`, local to where the expenses happened.
    date: String,
    /// Sum of the day in the base currency; more than one entry only if the
    /// group's base currency changed between expenses.
    totals: Vec<Money>,
    entries: Vec<TimelineEntry>,
}

/// `/groups/:id/timeline`: the group's expenses by day, newest first, with
/// a button to add one, also with an earlier date (GRP-20, GRP-21, GRP-23).
/// Tapping an entry opens its detail (GRP-22).
#[component]
pub fn GroupTimeline(id: String) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let nav = use_navigator();
    let mut shown_days = use_signal(|| DAYS_PER_PAGE);
    let today = use_hook(|| clock::local_now().0);

    let group_id = use_memo(use_reactive!(|id| GroupId::new(id)));
    let data = use_memo(move || {
        revision.track();
        let id = group_id();
        if db.group(&id).map_err(|e| e.to_string())?.is_none() {
            return Ok(None);
        }
        let categories: BTreeMap<CategoryId, Category> = db
            .categories()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect();
        let entries = db.group_timeline(&id).map_err(|e| e.to_string())?;
        let days = group_by_day(entries).map_err(|e| e.to_string())?;
        Ok::<_, String>(Some((categories, days)))
    });

    rsx! {
        TopBar { title: t!("page.group_timeline").to_string(), show_back: true }
        match &*data.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("timeline.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(None) => rsx! { GroupNotFound {} },
            Ok(Some((categories, days))) => rsx! {
                div { class: "mx-4 flex flex-col gap-5 pt-4 safe-area-x",
                    Button {
                        class: "w-full",
                        onclick: move |_| {
                            nav.push(Route::ExpenseNew {
                                group: group_id().as_str().to_string(),
                            });
                        },
                        Icon { icon: LdPlus, class: "h-5 w-5" }
                        {t!("timeline.add").to_string()}
                    }
                    if days.is_empty() {
                        EmptyState {
                            title: t!("timeline.empty_title").to_string(),
                            text: t!("timeline.empty_text").to_string(),
                            Icon { icon: LdReceipt, class: "h-8 w-8" }
                        }
                    }
                    for day in days.iter().take(shown_days()).cloned() {
                        DaySection {
                            key: "{day.date}",
                            heading: day_heading(&day.date, &today),
                            categories: categories.clone(),
                            day,
                        }
                    }
                    if days.len() > shown_days() {
                        // Loads the next days once it scrolls into view; a
                        // new key per page observes the new element afresh,
                        // so it also fires if it is still visible. Tapping
                        // works as well.
                        button {
                            key: "more-{shown_days()}",
                            class: "flex min-h-12 items-center justify-center gap-2 rounded-2xl text-sm text-floral-white-300 active:bg-jet-black-800 transition-colors ease-apple",
                            r#type: "button",
                            onvisible: move |event| {
                                if event.is_intersecting().unwrap_or(false) {
                                    shown_days += DAYS_PER_PAGE;
                                }
                            },
                            onclick: move |_| shown_days += DAYS_PER_PAGE,
                            Icon { icon: LdChevronDown, class: "h-4 w-4" }
                            {t!("timeline.more").to_string()}
                        }
                    }
                }
            },
        }
    }
}

/// Heading with the day's total and the day's expenses.
#[component]
fn DaySection(
    day: TimelineDay,
    heading: String,
    categories: BTreeMap<CategoryId, Category>,
) -> Element {
    let nav = use_navigator();

    rsx! {
        section { class: "flex flex-col gap-2",
            div { class: "flex items-baseline justify-between gap-3 px-1",
                h2 { class: "text-sm font-medium text-floral-white-300", "{heading}" }
                span { class: "flex flex-wrap justify-end gap-x-2 text-sm font-medium text-floral-white-300",
                    for total in day.totals.iter().copied() {
                        MoneyText { amount: total }
                    }
                }
            }
            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                for entry in day.entries.iter().cloned() {
                    EntryRow {
                        key: "{entry.id.as_str()}",
                        icon: entry
                            .category_id
                            .as_ref()
                            .and_then(|id| categories.get(id))
                            .map(|c| c.icon.clone())
                            .unwrap_or_default(),
                        onclick: {
                            let id = entry.id.as_str().to_string();
                            move |_| {
                                nav.push(Route::ExpenseDetail { id: id.clone() });
                            }
                        },
                        entry,
                    }
                }
            }
        }
    }
}

/// One expense (GRP-21): category, title, time and who paid with what;
/// the amount in the base currency and, if different, in its own.
#[component]
fn EntryRow(entry: TimelineEntry, icon: String, onclick: EventHandler<()>) -> Element {
    let foreign = entry.total.currency() != entry.total_in_base.currency();
    let time = entry
        .occurred_at
        .get(11..16)
        .unwrap_or_default()
        .to_string();
    let payers = payers_text(&entry.payers);
    let subtitle = if payers.is_empty() {
        time
    } else {
        format!("{time} · {payers}")
    };

    rsx! {
        button {
            class: "flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            span {
                class: "flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-jet-black-800 text-floral-white-300",
                aria_hidden: "true",
                CategoryIconGlyph { icon, class: "h-5 w-5".to_string() }
            }
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "truncate text-base text-floral-white-50", "{entry.title}" }
                span { class: "truncate text-sm text-floral-white-400", "{subtitle}" }
            }
            span { class: "flex shrink-0 flex-col items-end",
                MoneyText { amount: entry.total_in_base, class: "text-base font-semibold text-floral-white-100" }
                if foreign {
                    MoneyText { amount: entry.total, class: "text-sm text-floral-white-400" }
                }
            }
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
        }
    }
}

/// "Anna (Visa), Ben": every payer, with the method if known.
fn payers_text(payers: &[TimelinePayer]) -> String {
    payers
        .iter()
        .map(|payer| match &payer.method {
            Some(method) => t!(
                "timeline.payer_with_method",
                name = payer.name,
                method = method
            )
            .to_string(),
            None => payer.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Splits the entries (already newest first) into days and sums each day
/// per base currency, keeping the order.
fn group_by_day(entries: Vec<TimelineEntry>) -> Result<Vec<TimelineDay>, MoneyError> {
    let mut days: Vec<TimelineDay> = Vec::new();
    for entry in entries {
        let date = local_date(&entry.occurred_at).to_string();
        let day = match days.last_mut() {
            Some(day) if day.date == date => day,
            _ => {
                days.push(TimelineDay {
                    date,
                    totals: Vec::new(),
                    entries: Vec::new(),
                });
                days.last_mut().expect("a day was just pushed")
            }
        };
        let base = entry.total_in_base;
        match day
            .totals
            .iter_mut()
            .find(|t| t.currency() == base.currency())
        {
            Some(total) => *total = total.checked_add(base)?,
            None => day.totals.push(base),
        }
        day.entries.push(entry);
    }
    Ok(days)
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::{Currency, ExpenseId};

    use super::*;

    fn entry(id: &str, occurred_at: &str, base: i64, currency: &str) -> TimelineEntry {
        let currency = Currency::from_code(currency).unwrap();
        TimelineEntry {
            id: ExpenseId::new(id),
            title: id.to_string(),
            category_id: None,
            occurred_at: occurred_at.to_string(),
            total: Money::new(base, currency),
            total_in_base: Money::new(base, currency),
            payers: Vec::new(),
        }
    }

    #[test]
    fn groups_consecutive_entries_by_local_day_with_totals() {
        let days = group_by_day(vec![
            entry("c", "2026-10-04T20:00:00+09:00", 1500, "EUR"),
            entry("b", "2026-10-04T08:00:00+09:00", 250, "EUR"),
            entry("a", "2026-10-02T23:30:00+09:00", 999, "EUR"),
        ])
        .unwrap();
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].date, "2026-10-04");
        assert_eq!(
            days[0].totals,
            vec![Money::new(1750, Currency::from_code("EUR").unwrap())]
        );
        let ids: Vec<_> = days[0].entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["c", "b"]);
        assert_eq!(days[1].date, "2026-10-02");
        assert_eq!(days[1].entries.len(), 1);
    }

    #[test]
    fn sums_each_base_currency_separately() {
        let days = group_by_day(vec![
            entry("b", "2026-10-04T20:00:00+02:00", 100, "EUR"),
            entry("a", "2026-10-04T10:00:00+02:00", 5, "CHF"),
            entry("c", "2026-10-04T09:00:00+02:00", 1, "EUR"),
        ])
        .unwrap();
        let totals: Vec<_> = days[0]
            .totals
            .iter()
            .map(|t| (t.currency().code().to_string(), t.amount_minor()))
            .collect();
        assert_eq!(totals, [("EUR".to_string(), 101), ("CHF".to_string(), 5)]);
    }

    #[test]
    fn no_entries_no_days() {
        assert_eq!(group_by_day(Vec::new()), Ok(Vec::new()));
    }
}
