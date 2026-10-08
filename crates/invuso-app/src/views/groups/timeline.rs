use std::collections::{BTreeMap, BTreeSet};

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdChevronDown, LdChevronRight, LdCircleAlert, LdHandCoins, LdPlus, LdReceipt, LdSearch,
        LdSearchX, LdSquare, LdSquareCheck,
    },
};
use invuso_core::domain::{
    Category, CategoryId, GroupId, Money, MoneyError, PaymentMethod, Person, PersonId, local_date,
};

use super::form::GroupNotFound;
use crate::Route;
use crate::clock;
use crate::components::{
    BottomSheet, Button, ButtonVariant, CategoryIcon, DateField, EmptyState, ExpenseRow,
    FilterChip, MoneyText, PaymentMethodIcon, PersonOption, PersonPicker, RemovableChip, TopBar,
};
use crate::preferences::{category_name, day_heading, display_date};
use crate::services::receipts;
use crate::services::timeline::{
    TimelineFilter, TimelineItem, TimelineSettlement, filter_timeline, group_timeline,
};
use crate::state::{DataRevision, TimelineFilters};
use crate::storage::{Db, StorageError, TimelineEntry, TimelinePayer};

/// Days rendered at first and added each time the end of the list comes
/// into view, so long trips stay smooth (AP-13 step 4).
const DAYS_PER_PAGE: usize = 20;

/// One day of the timeline (GRP-20).
#[derive(Debug, Clone, PartialEq)]
struct TimelineDay {
    /// `YYYY-MM-DD`, local to where the expenses happened.
    date: String,
    /// Sum of the day's expenses in the base currency, settlements left out
    /// (user decision in AP-29); more than one entry only if the group's
    /// base currency changed between expenses.
    totals: Vec<Money>,
    items: Vec<TimelineItem>,
}

/// Everything the timeline shows and filters by.
#[derive(Debug, Clone, PartialEq)]
struct TimelineData {
    items: Vec<TimelineItem>,
    categories: BTreeMap<CategoryId, Category>,
    /// Members and everyone else taking part in an entry, "Ich" first.
    people: Vec<Person>,
    /// Categories used by the group's expenses, in the usual order.
    used_categories: Vec<Category>,
    /// Methods used by the group's expenses and settlements.
    used_methods: Vec<PaymentMethod>,
}

/// Which filter the sheet shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterSheet {
    People,
    Categories,
    Methods,
    Period,
}

/// `/groups/:id/timeline`: the group's expenses and settlements by day,
/// newest first, with a button to add an expense, also with an earlier
/// date (GRP-20, GRP-21, GRP-23, GRP-26). A search and filters narrow the
/// list (GRP-24, GRP-25). Tapping an expense opens its detail (GRP-22),
/// tapping a settlement the debts page.
#[component]
pub fn GroupTimeline(id: String) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let mut filters = use_context::<TimelineFilters>();
    let nav = use_navigator();
    let mut shown_days = use_signal(|| DAYS_PER_PAGE);
    let mut sheet = use_signal(|| None::<FilterSheet>);
    let today = use_hook(|| clock::local_now().0);

    let group_id = use_memo(use_reactive!(|id| GroupId::new(id)));
    let data = use_memo(move || {
        revision.track();
        load(&db, &group_id()).map_err(|e| e.to_string())
    });
    let filter = use_memo(move || filters.get(&group_id()));
    let days = use_memo(move || match &*data.read() {
        Ok(Some(data)) => {
            group_by_day(filter_timeline(&data.items, &filter())).map_err(|e| e.to_string())
        }
        _ => Ok(Vec::new()),
    });
    let mut change = move |edit: Box<dyn FnOnce(&mut TimelineFilter)>| {
        filters.update(&group_id(), edit);
        shown_days.set(DAYS_PER_PAGE);
    };

    let data_ref = data.read();
    let data = match &*data_ref {
        Err(message) => {
            return rsx! {
                TopBar { title: t!("page.group_timeline").to_string(), show_back: true }
                LoadError { message: message.clone() }
            };
        }
        Ok(None) => {
            return rsx! {
                TopBar { title: t!("page.group_timeline").to_string(), show_back: true }
                GroupNotFound {}
            };
        }
        Ok(Some(data)) => data.clone(),
    };
    drop(data_ref);
    let filter_now = filter();
    let days_ref = days.read();
    let days_now = match &*days_ref {
        Ok(days) => days.clone(),
        Err(message) => {
            return rsx! {
                TopBar { title: t!("page.group_timeline").to_string(), show_back: true }
                LoadError { message: message.clone() }
            };
        }
    };
    drop(days_ref);
    let hits: usize = days_now.iter().map(|day| day.items.len()).sum();

    rsx! {
        TopBar { title: t!("page.group_timeline").to_string(), show_back: true }
        div { class: "mx-4 flex flex-col gap-5 pt-4 safe-area-x",
            Button {
                class: "w-full",
                onclick: move |_| {
                    nav.push(Route::ExpenseNew {
                        group: group_id().as_str().to_string(),
                        receipt: String::new(),
                        copy: String::new(),
                    });
                },
                Icon { icon: LdPlus, class: "h-5 w-5" }
                {t!("timeline.add").to_string()}
            }
            if data.items.is_empty() {
                EmptyState {
                    title: t!("timeline.empty_title").to_string(),
                    text: t!("timeline.empty_text").to_string(),
                    Icon { icon: LdReceipt, class: "h-8 w-8" }
                }
            } else {
                div { class: "flex flex-col gap-3",
                    SearchField {
                        value: filter_now.query.clone(),
                        oninput: move |query: String| change(Box::new(move |f: &mut TimelineFilter| f.query = query)),
                    }
                    FilterBar {
                        filter: filter_now.clone(),
                        on_open: move |which| sheet.set(Some(which)),
                        on_receipt: move |_| change(Box::new(|f: &mut TimelineFilter| f.receipt_only = !f.receipt_only)),
                    }
                    ActiveFilters {
                        filter: filter_now.clone(),
                        data: data.clone(),
                        on_change: move |edit| change(edit),
                    }
                    if filter_now.is_active() && hits > 0 {
                        p { class: "px-1 text-sm text-floral-white-400", role: "status",
                            {t!("timeline.hits", count = hits).to_string()}
                        }
                    }
                }
                if days_now.is_empty() {
                    EmptyState {
                        title: t!("timeline.no_hits_title").to_string(),
                        text: t!("timeline.no_hits_text").to_string(),
                        Icon { icon: LdSearchX, class: "h-8 w-8" }
                    }
                    Button {
                        variant: ButtonVariant::Secondary,
                        class: "w-full",
                        onclick: move |_| change(Box::new(|f: &mut TimelineFilter| *f = TimelineFilter::default())),
                        {t!("timeline.reset").to_string()}
                    }
                }
                for day in days_now.iter().take(shown_days()).cloned() {
                    DaySection {
                        key: "{day.date}",
                        heading: day_heading(&day.date, &today),
                        categories: data.categories.clone(),
                        group: group_id().as_str().to_string(),
                        day,
                    }
                }
                if days_now.len() > shown_days() {
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
        }
        if let Some(which) = sheet() {
            FilterSheetView {
                which,
                filter: filter_now.clone(),
                data: data.clone(),
                on_change: move |edit| change(edit),
                on_close: move |_| sheet.set(None),
            }
        }
    }
}

/// Loads the timeline and what its filters offer; `None` if the group
/// does not exist.
fn load(db: &Db, group: &GroupId) -> Result<Option<TimelineData>, StorageError> {
    if db.group(group)?.is_none() {
        return Ok(None);
    }
    let all_categories = db.all_categories()?;
    let items = group_timeline(db, group)?;

    let mut used_categories = BTreeSet::new();
    let mut used_methods = BTreeSet::new();
    let mut involved: BTreeSet<PersonId> = db
        .group_members(group)?
        .into_iter()
        .map(|member| member.person.id)
        .collect();
    for item in &items {
        match item {
            TimelineItem::Expense(entry) => {
                used_categories.extend(entry.category_id.clone());
                used_methods.extend(entry.method_ids.iter().cloned());
                involved.extend(entry.people.iter().cloned());
            }
            TimelineItem::Settlement(item) => {
                used_methods.extend(item.settlement.payment_method_id.clone());
                involved.insert(item.settlement.from.clone());
                involved.insert(item.settlement.to.clone());
            }
        }
    }
    let mut people: Vec<Person> = db.people_any(&involved)?.into_values().collect();
    people.sort_by(|a, b| {
        b.is_me
            .cmp(&a.is_me)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(Some(TimelineData {
        items,
        used_categories: all_categories
            .iter()
            .filter(|c| used_categories.contains(&c.id))
            .cloned()
            .collect(),
        categories: all_categories
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect(),
        people,
        used_methods: db
            .payment_methods()?
            .into_iter()
            .filter(|m| used_methods.contains(&m.id))
            .collect(),
    }))
}

#[component]
fn LoadError(message: String) -> Element {
    rsx! {
        EmptyState {
            title: t!("timeline.load_error_title").to_string(),
            text: message,
            Icon { icon: LdCircleAlert, class: "h-8 w-8" }
        }
    }
}

/// Search over titles, merchants and line texts (GRP-25).
#[component]
fn SearchField(value: String, oninput: EventHandler<String>) -> Element {
    rsx! {
        label { class: "flex min-h-12 items-center gap-2 rounded-2xl border border-jet-black-700 bg-jet-black-900 px-4 focus-within:border-cerulean-500 transition-colors",
            Icon { icon: LdSearch, class: "h-5 w-5 shrink-0 text-floral-white-400" }
            input {
                class: "min-h-12 min-w-0 flex-1 bg-transparent text-base text-floral-white-50 placeholder:text-floral-white-600 outline-none",
                r#type: "search",
                autocomplete: "off",
                placeholder: t!("timeline.search").to_string(),
                aria_label: t!("timeline.search").to_string(),
                value: "{value}",
                oninput: move |event| oninput.call(event.value()),
            }
        }
    }
}

/// One chip per kind of filter (GRP-24); counts show how many values are
/// chosen.
#[component]
fn FilterBar(
    filter: TimelineFilter,
    on_open: EventHandler<FilterSheet>,
    on_receipt: EventHandler<()>,
) -> Element {
    let label = |name: String, count: usize| {
        if count == 0 {
            name
        } else {
            format!("{name} · {count}")
        }
    };
    let period = filter.from.is_some() || filter.to.is_some();

    rsx! {
        div { class: "flex flex-wrap gap-2", role: "group", aria_label: t!("timeline.filters").to_string(),
            FilterChip {
                label: label(t!("timeline.filter_people").to_string(), filter.people.len()),
                active: !filter.people.is_empty(),
                menu: true,
                onclick: move |_| on_open.call(FilterSheet::People),
            }
            FilterChip {
                label: label(t!("timeline.filter_categories").to_string(), filter.categories.len()),
                active: !filter.categories.is_empty(),
                menu: true,
                onclick: move |_| on_open.call(FilterSheet::Categories),
            }
            FilterChip {
                label: label(t!("timeline.filter_methods").to_string(), filter.methods.len()),
                active: !filter.methods.is_empty(),
                menu: true,
                onclick: move |_| on_open.call(FilterSheet::Methods),
            }
            FilterChip {
                label: t!("timeline.filter_period").to_string(),
                active: period,
                menu: true,
                onclick: move |_| on_open.call(FilterSheet::Period),
            }
            FilterChip {
                label: t!("timeline.filter_receipt").to_string(),
                active: filter.receipt_only,
                onclick: move |_| on_receipt.call(()),
            }
        }
    }
}

type FilterEdit = Box<dyn FnOnce(&mut TimelineFilter)>;

/// Every chosen value as a chip of its own, each removable (GRP-24).
#[component]
fn ActiveFilters(
    filter: TimelineFilter,
    data: TimelineData,
    on_change: EventHandler<FilterEdit>,
) -> Element {
    if !filter.has_filters() {
        return rsx! {};
    }
    let mut chips: Vec<(String, FilterEdit)> = Vec::new();
    for person in data.people.iter().filter(|p| filter.people.contains(&p.id)) {
        let id = person.id.clone();
        chips.push((
            person.name.clone(),
            Box::new(move |f: &mut TimelineFilter| {
                f.people.remove(&id);
            }),
        ));
    }
    for id in &filter.categories {
        let name = data
            .categories
            .get(id)
            .map(category_name)
            .unwrap_or_else(|| id.as_str().to_string());
        let id = id.clone();
        chips.push((
            name,
            Box::new(move |f: &mut TimelineFilter| {
                f.categories.remove(&id);
            }),
        ));
    }
    for method in data
        .used_methods
        .iter()
        .filter(|m| filter.methods.contains(&m.id))
    {
        let id = method.id.clone();
        chips.push((
            method.name.clone(),
            Box::new(move |f: &mut TimelineFilter| {
                f.methods.remove(&id);
            }),
        ));
    }
    if let Some(period) = period_text(&filter) {
        chips.push((
            period,
            Box::new(|f: &mut TimelineFilter| {
                f.from = None;
                f.to = None;
            }),
        ));
    }
    if filter.receipt_only {
        chips.push((
            t!("timeline.filter_receipt").to_string(),
            Box::new(|f: &mut TimelineFilter| f.receipt_only = false),
        ));
    }
    let many = chips.len() > 1;
    // Each edit runs once; the cell hands it over on the first tap.
    let chips: Vec<(String, std::rc::Rc<std::cell::Cell<Option<FilterEdit>>>)> = chips
        .into_iter()
        .map(|(label, edit)| (label, std::rc::Rc::new(std::cell::Cell::new(Some(edit)))))
        .collect();

    rsx! {
        div { class: "flex flex-wrap items-center gap-2",
            for (index, (label, edit)) in chips.into_iter().enumerate() {
                RemovableChip {
                    key: "{index}-{label}",
                    remove_label: t!("timeline.remove_filter", name = label.clone()).to_string(),
                    label,
                    on_remove: move |_| {
                        if let Some(edit) = edit.take() {
                            on_change.call(edit);
                        }
                    },
                }
            }
            if many {
                button {
                    class: "min-h-11 rounded-full px-3 text-sm font-medium text-cerulean-300 active:bg-jet-black-800 transition-colors",
                    r#type: "button",
                    onclick: move |_| {
                        on_change.call(Box::new(|f: &mut TimelineFilter| {
                            *f = TimelineFilter {
                                query: std::mem::take(&mut f.query),
                                ..TimelineFilter::default()
                            };
                        }))
                    },
                    {t!("timeline.clear_filters").to_string()}
                }
            }
        }
    }
}

/// "01.10.2026 – 03.10.2026", "ab 01.10.2026" or "bis 03.10.2026".
fn period_text(filter: &TimelineFilter) -> Option<String> {
    match (&filter.from, &filter.to) {
        (Some(from), Some(to)) if from == to => Some(display_date(from)),
        (Some(from), Some(to)) => Some(format!("{} – {}", display_date(from), display_date(to))),
        (Some(from), None) => {
            Some(t!("timeline.period_from", date = display_date(from)).to_string())
        }
        (None, Some(to)) => Some(t!("timeline.period_to", date = display_date(to)).to_string()),
        (None, None) => None,
    }
}

/// Sheet to choose the values of one filter; choices apply at once.
#[component]
fn FilterSheetView(
    which: FilterSheet,
    filter: TimelineFilter,
    data: TimelineData,
    on_change: EventHandler<FilterEdit>,
    on_close: EventHandler<()>,
) -> Element {
    let title = match which {
        FilterSheet::People => t!("timeline.filter_people"),
        FilterSheet::Categories => t!("timeline.filter_categories"),
        FilterSheet::Methods => t!("timeline.filter_methods"),
        FilterSheet::Period => t!("timeline.filter_period"),
    }
    .to_string();

    rsx! {
        BottomSheet { title, on_close: move |_| on_close.call(()),
            div { class: "flex max-h-[70vh] flex-col gap-1 overflow-y-auto overscroll-contain px-3 pt-2",
                match which {
                    FilterSheet::People => rsx! {
                        PersonPicker {
                            multiple: true,
                            options: data
                                .people
                                .iter()
                                .map(|person| PersonOption {
                                    selected: filter.people.contains(&person.id),
                                    person: person.clone(),
                                    detail: None,
                                })
                                .collect::<Vec<_>>(),
                            on_toggle: move |id: PersonId| {
                                on_change.call(Box::new(move |f: &mut TimelineFilter| toggle(&mut f.people, id)))
                            },
                        }
                    },
                    FilterSheet::Categories => rsx! {
                        if data.used_categories.is_empty() {
                            p { class: "px-3 py-4 text-base text-floral-white-400", {t!("timeline.no_categories").to_string()} }
                        }
                        for category in data.used_categories.iter().cloned() {
                            CheckRow {
                                key: "{category.id.as_str()}",
                                label: category_name(&category),
                                selected: filter.categories.contains(&category.id),
                                onclick: {
                                    let id = category.id.clone();
                                    move |_| {
                                        let id = id.clone();
                                        on_change.call(Box::new(move |f: &mut TimelineFilter| toggle(&mut f.categories, id)))
                                    }
                                },
                                CategoryIcon { icon: category.icon.clone(), color: category.color.clone() }
                            }
                        }
                    },
                    FilterSheet::Methods => rsx! {
                        if data.used_methods.is_empty() {
                            p { class: "px-3 py-4 text-base text-floral-white-400", {t!("timeline.no_methods").to_string()} }
                        }
                        for method in data.used_methods.iter().cloned() {
                            CheckRow {
                                key: "{method.id.as_str()}",
                                label: method.name.clone(),
                                selected: filter.methods.contains(&method.id),
                                onclick: {
                                    let id = method.id.clone();
                                    move |_| {
                                        let id = id.clone();
                                        on_change.call(Box::new(move |f: &mut TimelineFilter| toggle(&mut f.methods, id)))
                                    }
                                },
                                PaymentMethodIcon { icon: method.icon.clone(), color: method.color.clone() }
                            }
                        }
                    },
                    FilterSheet::Period => rsx! {
                        div { class: "flex flex-col gap-4 px-2 pb-2",
                            DateField {
                                id: "timeline-from",
                                label: t!("timeline.period_start").to_string(),
                                value: filter.from.clone().unwrap_or_default(),
                                oninput: move |date: String| {
                                    on_change.call(Box::new(move |f: &mut TimelineFilter| {
                                        f.from = (!date.is_empty()).then_some(date);
                                        // A start after the end would hide everything.
                                        if f.to.is_some() && f.from > f.to {
                                            f.to = None;
                                        }
                                    }))
                                },
                            }
                            DateField {
                                id: "timeline-to",
                                label: t!("timeline.period_end").to_string(),
                                value: filter.to.clone().unwrap_or_default(),
                                min: filter.from.clone(),
                                oninput: move |date: String| {
                                    on_change.call(Box::new(move |f: &mut TimelineFilter| f.to = (!date.is_empty()).then_some(date)))
                                },
                            }
                        }
                    },
                }
            }
            div { class: "px-5 pt-3",
                Button { class: "w-full", onclick: move |_| on_close.call(()),
                    {t!("timeline.filter_done").to_string()}
                }
            }
        }
    }
}

fn toggle<T: Ord>(set: &mut BTreeSet<T>, value: T) {
    if !set.remove(&value) {
        set.insert(value);
    }
}

/// Row with a check box, a leading icon (`children`) and a label.
#[component]
fn CheckRow(
    label: String,
    selected: bool,
    onclick: EventHandler<()>,
    children: Element,
) -> Element {
    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-3 rounded-2xl px-3 text-left active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            role: "option",
            aria_selected: if selected { "true" } else { "false" },
            onclick: move |_| onclick.call(()),
            if selected {
                Icon { icon: LdSquareCheck, class: "h-6 w-6 shrink-0 text-cerulean-300" }
            } else {
                Icon { icon: LdSquare, class: "h-6 w-6 shrink-0 text-floral-white-500" }
            }
            {children}
            span { class: "min-w-0 flex-1 truncate text-base text-floral-white-50", "{label}" }
        }
    }
}

/// Heading with the day's total and the day's entries.
#[component]
fn DaySection(
    day: TimelineDay,
    heading: String,
    categories: BTreeMap<CategoryId, Category>,
    group: String,
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
                for item in day.items.iter().cloned() {
                    match item {
                        TimelineItem::Expense(entry) => rsx! {
                            ExpenseRow {
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
                                title: entry.title.clone(),
                                subtitle: entry_subtitle(&entry),
                                total: entry.total,
                                total_in_base: entry.total_in_base,
                                thumbnail: entry.thumbnail_path.as_deref().map(receipts::file_url),
                            }
                        },
                        TimelineItem::Settlement(item) => rsx! {
                            SettlementRow {
                                key: "{item.settlement.id.as_str()}",
                                item,
                                onclick: {
                                    let group = group.clone();
                                    move |_| {
                                        nav.push(Route::GroupSettle { id: group.clone(), record: false });
                                    }
                                },
                            }
                        },
                    }
                }
            }
        }
    }
}

/// A settlement in the timeline (GRP-26): set apart from expenses by the
/// "positive" color, an icon instead of a category and its label.
#[component]
fn SettlementRow(item: TimelineSettlement, onclick: EventHandler<()>) -> Element {
    let unknown = || t!("expense_detail.unknown_person").to_string();
    let from = item.from.clone().unwrap_or_else(unknown);
    let to = item.to.clone().unwrap_or_else(unknown);
    let title = t!("summary.transfer", from = from, to = to).to_string();
    let mut detail = vec![
        item.settlement
            .occurred_at
            .get(11..16)
            .unwrap_or_default()
            .to_string(),
        t!("timeline.settlement").to_string(),
    ];
    detail.extend(item.method.clone());
    let detail = detail.join(" · ");

    rsx! {
        button {
            class: "flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 bg-muted-teal-950 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            span {
                class: "flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-muted-teal-800 text-muted-teal-200",
                aria_hidden: "true",
                Icon { icon: LdHandCoins, class: "h-5 w-5" }
            }
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "truncate text-base text-floral-white-50", "{title}" }
                span { class: "truncate text-sm text-muted-teal-300", "{detail}" }
            }
            MoneyText { amount: item.settlement.amount, class: "shrink-0 text-base font-semibold text-muted-teal-200" }
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
        }
    }
}

/// Time and who paid with what, under the title of an entry (GRP-21).
fn entry_subtitle(entry: &TimelineEntry) -> String {
    let time = entry
        .occurred_at
        .get(11..16)
        .unwrap_or_default()
        .to_string();
    let payers = payers_text(&entry.payers);
    if payers.is_empty() {
        time
    } else {
        format!("{time} · {payers}")
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

/// Splits the items (already newest first) into days and sums each day's
/// expenses per base currency, keeping the order.
fn group_by_day(items: Vec<TimelineItem>) -> Result<Vec<TimelineDay>, MoneyError> {
    let mut days: Vec<TimelineDay> = Vec::new();
    for item in items {
        let date = local_date(item.occurred_at()).to_string();
        let day = match days.last_mut() {
            Some(day) if day.date == date => day,
            _ => {
                days.push(TimelineDay {
                    date,
                    totals: Vec::new(),
                    items: Vec::new(),
                });
                days.last_mut().expect("a day was just pushed")
            }
        };
        if let TimelineItem::Expense(entry) = &item {
            let base = entry.total_in_base;
            match day
                .totals
                .iter_mut()
                .find(|t| t.currency() == base.currency())
            {
                Some(total) => *total = total.checked_add(base)?,
                None => day.totals.push(base),
            }
        }
        day.items.push(item);
    }
    Ok(days)
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::{Currency, ExpenseId, GroupId, Settlement, SettlementId};

    use super::*;

    fn entry(id: &str, occurred_at: &str, base: i64, currency: &str) -> TimelineItem {
        let currency = Currency::from_code(currency).unwrap();
        TimelineItem::Expense(TimelineEntry {
            id: ExpenseId::new(id),
            title: id.to_string(),
            category_id: None,
            occurred_at: occurred_at.to_string(),
            total: Money::new(base, currency),
            total_in_base: Money::new(base, currency),
            payers: Vec::new(),
            thumbnail_path: None,
            merchant: None,
            has_receipt: false,
            people: Default::default(),
            method_ids: Default::default(),
            item_texts: Vec::new(),
        })
    }

    fn settlement(occurred_at: &str, amount: i64) -> TimelineItem {
        TimelineItem::Settlement(TimelineSettlement {
            settlement: Settlement {
                id: SettlementId::new("s"),
                group_id: GroupId::new("g"),
                from: PersonId::new("a"),
                to: PersonId::new("b"),
                amount: Money::new(amount, Currency::from_code("EUR").unwrap()),
                payment_method_id: None,
                occurred_at: occurred_at.to_string(),
                note: None,
            },
            from: None,
            to: None,
            method: None,
        })
    }

    fn ids(day: &TimelineDay) -> Vec<&str> {
        day.items
            .iter()
            .map(|item| match item {
                TimelineItem::Expense(e) => e.id.as_str(),
                TimelineItem::Settlement(s) => s.settlement.id.as_str(),
            })
            .collect()
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
        assert_eq!(ids(&days[0]), ["c", "b"]);
        assert_eq!(days[1].date, "2026-10-02");
        assert_eq!(days[1].items.len(), 1);
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
    fn settlements_show_in_their_day_but_not_in_its_total() {
        let days = group_by_day(vec![
            settlement("2026-10-05T10:00:00+09:00", 2_000),
            entry("b", "2026-10-04T20:00:00+09:00", 100, "EUR"),
            settlement("2026-10-04T12:00:00+09:00", 5_000),
        ])
        .unwrap();
        assert_eq!(days.len(), 2);
        assert!(days[0].totals.is_empty());
        assert_eq!(days[0].items.len(), 1);
        assert_eq!(
            days[1].totals,
            vec![Money::new(100, Currency::from_code("EUR").unwrap())]
        );
        assert_eq!(ids(&days[1]), ["b", "s"]);
    }

    #[test]
    fn no_entries_no_days() {
        assert_eq!(group_by_day(Vec::new()), Ok(Vec::new()));
    }
}
