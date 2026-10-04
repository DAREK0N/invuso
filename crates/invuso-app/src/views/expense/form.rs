use std::collections::{BTreeMap, BTreeSet};

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdCheck, LdChevronRight, LdCircleAlert, LdPlus, LdTriangleAlert, LdUser, LdX,
    },
};
use invuso_core::Decimal;
use invuso_core::domain::{
    Category, CategoryId, Currency, ExpenseError, Group, GroupId, Money, PaymentMethod,
    PaymentMethodId, Person, PersonId, is_iso_date, validate_participants, validate_payments,
};
use invuso_core::fx;
use invuso_core::split::{SplitMode, allocate, split};

use crate::Route;
use crate::clock;
use crate::components::{
    AmountInput, Avatar, AvatarSize, BottomSheet, Button, CategoryIconGlyph, Chip,
    CompactAmountInput, CurrencyPicker, DateTimeField, EmptyState, ErrorBanner, GroupIcon,
    MoneyText, PaymentIconGlyph, PaymentMethodIcon, PersonOption, PersonPicker, TextField, TopBar,
};
use crate::format::{NumberFormat, amount_text, fit_amount_text, format_money, parse_amount};
use crate::preferences::{category_name, default_home_currency, display_date};
use crate::services::expenses::{SaveExpenseError, save_expense};
use crate::services::rates::{CurrencyApi, Frankfurter};
use crate::state::{DataRevision, Toaster};
use crate::storage::{
    Db, LAST_EXPENSE_CURRENCY, LAST_EXPENSE_GROUP, NearRate, NewExpense, NewExpensePayment,
    StorageError,
};

/// What the form reads from the database once; it keeps its own state
/// while open.
#[derive(Debug, Clone, PartialEq)]
struct FormData {
    me: Person,
    home_currency: Currency,
    groups: Vec<Group>,
    categories: Vec<Category>,
    /// Active payment methods of everyone.
    methods: Vec<PaymentMethod>,
    /// Preselected group: the one of the last expense, else the newest.
    group: Option<GroupId>,
    /// Currency of the last expense, if any.
    currency: Option<Currency>,
}

/// Someone who paid (part of) the expense (EXP-02, EXP-03).
#[derive(Debug, Clone, PartialEq)]
struct PayerDraft {
    person: Person,
    method: Option<PaymentMethodId>,
    /// Canonical amount text; only used once the parts are edited by hand.
    amount_text: String,
}

/// Which sheet the form shows.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Sheet {
    Currency,
    Group,
    AddPayer,
    /// Payment method of the payer at this index.
    Method(usize),
}

/// `/expense/new`: records an expense by hand (EXP-01..04, EXP-06, EXP-07;
/// idee.md 7.3), split equally. Saving leads to the group.
#[component]
pub fn ExpenseNew() -> Element {
    let db = use_context::<Db>();
    let data = use_hook(|| load(&db).map_err(|e| e.to_string()));

    rsx! {
        TopBar { title: t!("page.expense_new").to_string(), show_back: true }
        match data {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("expense.load_error_title").to_string(),
                    text: message,
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(data) => rsx! { ExpenseForm { data } },
        }
    }
}

#[component]
fn ExpenseForm(data: FormData) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let nav = use_navigator();

    let initial_people =
        use_hook(|| people_of(&db, data.group.as_ref(), &data.me).map_err(|e| e.to_string()));
    let start_people = initial_people
        .clone()
        .unwrap_or_else(|_| vec![data.me.clone()]);

    let mut amount_text_signal = use_signal(String::new);
    let start_currency = data
        .currency
        .unwrap_or_else(|| base_of(&data.groups, data.group.as_ref(), data.home_currency));
    let mut currency = use_signal(|| start_currency);
    let mut title = use_signal(String::new);
    let mut category = use_signal(|| None::<CategoryId>);
    let (today, now) = use_hook(clock::local_now);
    let mut date = use_signal(|| today);
    let mut time = use_signal(|| now);
    let mut group = use_signal(|| data.group.clone());
    let mut people = use_signal(|| start_people.clone());
    let start_payer = default_payer(&start_people, &data.me, &data.methods);
    let mut payers = use_signal(|| vec![start_payer]);
    let mut payers_manual = use_signal(|| false);
    let mut participants = use_signal(|| {
        start_people
            .iter()
            .map(|p| p.id.clone())
            .collect::<BTreeSet<_>>()
    });
    let mut sheet = use_signal(|| None::<Sheet>);
    let mut amount_error = use_signal(|| None::<String>);
    let mut title_error = use_signal(|| None::<String>);
    let mut date_error = use_signal(|| None::<String>);
    let mut payers_error = use_signal(|| None::<String>);
    let mut participants_error = use_signal(|| None::<String>);
    let mut save_error = use_signal(|| {
        initial_people
            .err()
            .map(|e| format!("{} {e}", t!("expense.people_error")))
    });
    let mut saving = use_signal(|| false);

    let groups = data.groups.clone();
    let home_currency = data.home_currency;
    let base_currency = use_memo(move || base_of(&groups, group().as_ref(), home_currency));

    let preview_db = db.clone();
    let preview = use_memo(move || {
        revision.track();
        let (from, to, day) = (currency(), base_currency(), date());
        if from == to || !is_iso_date(&day) {
            return Ok(None);
        }
        preview_db
            .rate_near(from, to, &day)
            .map(Some)
            .map_err(|e| format!("{} {e}", t!("converter.rate_error")))
    });

    let group_db = db.clone();
    let me = data.me.clone();
    let methods = data.methods.clone();
    let select_group = use_callback(move |new_group: Option<GroupId>| {
        sheet.set(None);
        match people_of(&group_db, new_group.as_ref(), &me) {
            Ok(list) => {
                payers.set(vec![default_payer(&list, &me, &methods)]);
                payers_manual.set(false);
                participants.set(list.iter().map(|p| p.id.clone()).collect());
                people.set(list);
                payers_error.set(None);
                participants_error.set(None);
            }
            Err(e) => save_error.set(Some(format!("{} {e}", t!("expense.people_error")))),
        }
        group.set(new_group);
    });

    let pick_currency = move |new_currency: Currency| {
        sheet.set(None);
        let format = NumberFormat::current();
        let fitted = fit_amount_text(&amount_text_signal.peek(), new_currency, format);
        amount_text_signal.set(fitted);
        for payer in payers.write().iter_mut() {
            payer.amount_text = fit_amount_text(&payer.amount_text, new_currency, format);
        }
        currency.set(new_currency);
    };

    let edit_payer = use_callback(move |(index, text): (usize, String)| {
        let format = NumberFormat::current();
        let cur = currency();
        let mut list = payers.write();
        if !*payers_manual.peek() {
            // Start from the equal parts shown so far.
            let total = parse_amount(&amount_text_signal.peek(), cur, format)
                .map_or(0, |m| m.amount_minor());
            let parts = equal_parts(total, list.len());
            for (payer, part) in list.iter_mut().zip(parts) {
                payer.amount_text = amount_text(Money::new(part, cur), format);
            }
            payers_manual.set(true);
        }
        if let Some(payer) = list.get_mut(index) {
            payer.amount_text = text;
        }
        payers_error.set(None);
    });

    let add_methods = data.methods.clone();
    let add_payer = use_callback(move |person: Person| {
        sheet.set(None);
        let method = default_method(&person, &add_methods);
        payers.write().push(PayerDraft {
            person,
            method,
            amount_text: String::new(),
        });
        payers_manual.set(false);
        payers_error.set(None);
    });

    let save_db = db.clone();
    let save = move |_| {
        let format = NumberFormat::current();
        let cur = currency();
        let mut valid = true;
        let total =
            parse_amount(&amount_text_signal(), cur, format).filter(|m| m.amount_minor() > 0);
        if total.is_none() {
            amount_error.set(Some(t!("expense.amount_required").to_string()));
            valid = false;
        }
        if title.read().trim().is_empty() {
            title_error.set(Some(t!("expense.title_required").to_string()));
            valid = false;
        }
        let occurred_at = clock::occurred_at(&date(), &time());
        if occurred_at.is_none() {
            date_error.set(Some(t!("expense.date_time_invalid").to_string()));
            valid = false;
        }
        let list = payers();
        let amounts = payer_amounts(
            &list,
            payers_manual(),
            total.map_or(0, |m| m.amount_minor()),
            cur,
            format,
        );
        if let Some(total) = total {
            let pairs: Vec<(PersonId, i64)> = list
                .iter()
                .zip(&amounts)
                .map(|(payer, amount)| (payer.person.id.clone(), *amount))
                .collect();
            if let Err(error) = validate_payments(total.amount_minor(), &pairs) {
                payers_error.set(Some(payments_error_text(&error, cur, format)));
                valid = false;
            }
        }
        if validate_participants(&participants.read()).is_err() {
            participants_error.set(Some(t!("expense.participants_required").to_string()));
            valid = false;
        }
        let (Some(total), Some(occurred_at), true) = (total, occurred_at, valid) else {
            return;
        };

        let new = NewExpense {
            group_id: group(),
            title: title(),
            category_id: category(),
            occurred_at,
            total,
            payments: list
                .iter()
                .zip(&amounts)
                .map(|(payer, amount)| NewExpensePayment {
                    person_id: payer.person.id.clone(),
                    payment_method_id: payer.method.clone(),
                    amount_minor: *amount,
                })
                .collect(),
            participants: participants(),
        };
        save_error.set(None);
        saving.set(true);
        let worker_db = save_db.clone();
        let (mut revision, mut toaster) = (revision, toaster);
        spawn(async move {
            // The day's rate may have to be fetched; keep the network off
            // the UI thread.
            let outcome = tokio::task::spawn_blocking(move || {
                save_expense(&worker_db, &Frankfurter, &CurrencyApi, new)
            })
            .await;
            match outcome {
                Ok(Ok(saved)) => {
                    revision.bump();
                    let message = if saved.later_rate {
                        t!("expense.saved_later_rate")
                    } else {
                        t!("expense.saved")
                    };
                    toaster.show(message.to_string(), None);
                    match saved.expense.group_id {
                        Some(id) => {
                            nav.replace(Route::GroupOverview {
                                id: id.as_str().to_string(),
                            });
                        }
                        None if nav.can_go_back() => nav.go_back(),
                        None => {
                            nav.replace(Route::Home {});
                        }
                    }
                }
                Ok(Err(error)) => {
                    saving.set(false);
                    save_error.set(Some(save_error_text(&error)));
                }
                Err(error) => {
                    saving.set(false);
                    save_error.set(Some(format!("{} {error}", t!("expense.save_error"))));
                }
            }
        });
    };

    // Values for this render.
    let format = NumberFormat::current();
    let cur = currency();
    let total = parse_amount(&amount_text_signal(), cur, format);
    let total_minor = total.map_or(0, |m| m.amount_minor());
    let payer_list = payers();
    let manual = payers_manual();
    let amounts = payer_amounts(&payer_list, manual, total_minor, cur, format);
    let paid: i64 = amounts.iter().sum();
    let selected = participants();
    let shares = if total_minor > 0 && !selected.is_empty() {
        split(total_minor, &SplitMode::Equal(selected.clone())).unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    let current_group = group();
    let group_entry = current_group
        .as_ref()
        .and_then(|id| data.groups.iter().find(|g| &g.id == id))
        .cloned();
    let candidates: Vec<Person> = people()
        .into_iter()
        .filter(|p| !payer_list.iter().any(|payer| payer.person.id == p.id))
        .collect();
    let all_selected = people().iter().all(|p| selected.contains(&p.id));

    rsx! {
        div { class: "mx-4 flex flex-col gap-5 pt-4 pb-8 safe-area-x",
            div { class: "flex flex-col gap-2",
                AmountInput {
                    id: "expense-amount",
                    label: t!("expense.amount").to_string(),
                    currency_label: t!("expense.currency").to_string(),
                    value: amount_text_signal(),
                    currency: cur,
                    autofocus: true,
                    oninput: move |text| {
                        amount_text_signal.set(text);
                        amount_error.set(None);
                    },
                    on_currency_click: move |_| sheet.set(Some(Sheet::Currency)),
                }
                if let Some(error) = amount_error() {
                    p { class: "px-1 text-sm text-watermelon-300", role: "alert", "{error}" }
                }
                match &*preview.read() {
                    Err(message) => rsx! { ErrorBanner { error: Some(message.clone()) } },
                    Ok(None) => rsx! {},
                    Ok(Some(near)) => rsx! {
                        BasePreview { total, near: near.clone(), base: base_currency() }
                    },
                }
            }
            TextField {
                id: "expense-title",
                label: t!("expense.title").to_string(),
                value: title(),
                placeholder: t!("expense.title_placeholder").to_string(),
                error: title_error(),
                oninput: move |value| {
                    title.set(value);
                    title_error.set(None);
                },
            }
            div { class: "flex flex-col gap-2",
                span { class: "text-sm font-medium text-floral-white-300", {t!("expense.category").to_string()} }
                div { class: "flex flex-wrap gap-2", role: "radiogroup",
                    for entry in data.categories.iter().cloned() {
                        Chip {
                            key: "{entry.id.as_str()}",
                            label: category_name(&entry),
                            selected: category().as_ref() == Some(&entry.id),
                            onclick: move |_| {
                                let id = entry.id.clone();
                                // A second tap clears the optional choice.
                                category.with_mut(|c| *c = if c.as_ref() == Some(&id) { None } else { Some(id) });
                            },
                            CategoryIconGlyph { icon: entry.icon.clone() }
                        }
                    }
                }
            }
            DateTimeField {
                id: "expense-when",
                label: t!("expense.date_time").to_string(),
                date: date(),
                time: time(),
                date_label: t!("expense.date").to_string(),
                time_label: t!("expense.time").to_string(),
                error: date_error(),
                on_date: move |value| {
                    date.set(value);
                    date_error.set(None);
                },
                on_time: move |value| {
                    time.set(value);
                    date_error.set(None);
                },
            }
            div { class: "flex flex-col gap-2",
                span { class: "text-sm font-medium text-floral-white-300", {t!("expense.group").to_string()} }
                button {
                    class: "flex min-h-14 w-full items-center gap-3 rounded-2xl border border-jet-black-700 bg-jet-black-900 px-3 text-left active:bg-jet-black-800 transition-colors ease-apple",
                    r#type: "button",
                    onclick: move |_| sheet.set(Some(Sheet::Group)),
                    match &group_entry {
                        Some(entry) => rsx! {
                            GroupIcon { icon: entry.icon.clone(), color: entry.color.clone() }
                            span { class: "min-w-0 flex-1 truncate text-base text-floral-white-50", "{entry.name}" }
                        },
                        None => rsx! {
                            NoGroupIcon {}
                            span { class: "min-w-0 flex-1 truncate text-base text-floral-white-50", {t!("expense.no_group").to_string()} }
                        },
                    }
                    Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
                }
            }
            section { class: "flex flex-col gap-2",
                h2 { class: "text-sm font-medium text-floral-white-300", {t!("expense.paid_by").to_string()} }
                div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                    for (index, payer) in payer_list.iter().cloned().enumerate() {
                        PayerRow {
                            key: "{payer.person.id.as_str()}",
                            payer: payer.clone(),
                            amount: Money::new(amounts.get(index).copied().unwrap_or(0), cur),
                            text: payer.amount_text.clone(),
                            editable: payer_list.len() > 1,
                            manual,
                            invalid: payers_error().is_some(),
                            method: payer.method.as_ref().and_then(|id| data.methods.iter().find(|m| &m.id == id)).cloned(),
                            on_amount: move |text| edit_payer.call((index, text)),
                            on_method: move |_| sheet.set(Some(Sheet::Method(index))),
                            on_remove: move |_| {
                                payers.write().remove(index);
                                payers_manual.set(false);
                                payers_error.set(None);
                            },
                        }
                    }
                }
                if payer_list.len() > 1 && manual {
                    p {
                        class: if paid == total_minor { "px-1 text-sm text-floral-white-400" } else { "px-1 text-sm text-pale-oak-300" },
                        aria_live: "polite",
                        {t!(
                            "expense.payments_sum",
                            paid = format_money(Money::new(paid, cur), format),
                            total = format_money(Money::new(total_minor, cur), format)
                        ).to_string()}
                    }
                }
                if let Some(error) = payers_error() {
                    p { class: "px-1 text-sm text-watermelon-300", role: "alert", "{error}" }
                }
                if current_group.is_some() && !candidates.is_empty() {
                    button {
                        class: "flex min-h-11 items-center gap-2 self-start rounded-full px-3 text-sm font-medium text-cerulean-300 active:bg-jet-black-800 transition-colors",
                        r#type: "button",
                        onclick: move |_| sheet.set(Some(Sheet::AddPayer)),
                        Icon { icon: LdPlus, class: "h-4 w-4" }
                        {t!("expense.add_payer").to_string()}
                    }
                }
            }
            section { class: "flex flex-col gap-2",
                div { class: "flex items-center justify-between gap-2",
                    h2 { class: "text-sm font-medium text-floral-white-300",
                        {t!("expense.split_between").to_string()}
                        if current_group.is_some() {
                            span { class: "text-floral-white-500", " · {t!(\"expense.split_equal\")}" }
                        }
                    }
                    if current_group.is_some() {
                        button {
                            class: "flex min-h-11 items-center rounded-full px-3 text-sm font-medium text-cerulean-300 active:bg-jet-black-800 transition-colors",
                            r#type: "button",
                            onclick: move |_| {
                                if all_selected {
                                    participants.set(BTreeSet::new());
                                } else {
                                    participants.set(people.read().iter().map(|p| p.id.clone()).collect());
                                    participants_error.set(None);
                                }
                            },
                            if all_selected {
                                {t!("expense.select_none").to_string()}
                            } else {
                                {t!("expense.select_all").to_string()}
                            }
                        }
                    }
                }
                if current_group.is_some() {
                    div { class: "overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900 py-1",
                        PersonPicker {
                            multiple: true,
                            options: people()
                                .into_iter()
                                .map(|person| PersonOption {
                                    selected: selected.contains(&person.id),
                                    detail: shares
                                        .get(&person.id)
                                        .map(|share| format_money(Money::new(*share, cur), format)),
                                    person,
                                })
                                .collect::<Vec<_>>(),
                            on_toggle: move |id: PersonId| {
                                participants.with_mut(|set| {
                                    if !set.remove(&id) {
                                        set.insert(id);
                                    }
                                });
                                participants_error.set(None);
                            },
                        }
                    }
                } else {
                    p { class: "px-1 text-sm text-floral-white-400", {t!("expense.no_group_hint").to_string()} }
                }
                if let Some(error) = participants_error() {
                    p { class: "px-1 text-sm text-watermelon-300", role: "alert", "{error}" }
                }
            }
            ErrorBanner { error: save_error() }
            Button {
                class: "w-full",
                disabled: saving(),
                onclick: save,
                if saving() {
                    {t!("expense.saving").to_string()}
                } else {
                    {t!("common.save").to_string()}
                }
            }
        }
        match sheet() {
            Some(Sheet::Currency) => rsx! {
                BottomSheet {
                    title: t!("expense.currency").to_string(),
                    on_close: move |_| sheet.set(None),
                    div { class: "flex max-h-[70vh] flex-col gap-2 overflow-y-auto overscroll-contain px-3 pt-2",
                        CurrencyPicker { selected: cur, on_select: pick_currency }
                    }
                }
            },
            Some(Sheet::Group) => rsx! {
                BottomSheet {
                    title: t!("expense.group").to_string(),
                    on_close: move |_| sheet.set(None),
                    div { class: "flex max-h-[70vh] flex-col gap-1 overflow-y-auto overscroll-contain px-3 pt-2", role: "listbox",
                        ChoiceRow {
                            label: t!("expense.no_group").to_string(),
                            selected: current_group.is_none(),
                            onclick: move |_| select_group.call(None),
                            NoGroupIcon {}
                        }
                        for entry in data.groups.iter().cloned() {
                            ChoiceRow {
                                key: "{entry.id.as_str()}",
                                label: entry.name.clone(),
                                selected: current_group.as_ref() == Some(&entry.id),
                                onclick: move |_| select_group.call(Some(entry.id.clone())),
                                GroupIcon { icon: entry.icon.clone(), color: entry.color.clone() }
                            }
                        }
                    }
                }
            },
            Some(Sheet::AddPayer) => rsx! {
                BottomSheet {
                    title: t!("expense.add_payer").to_string(),
                    on_close: move |_| sheet.set(None),
                    div { class: "flex max-h-[70vh] flex-col overflow-y-auto overscroll-contain px-3 pt-2",
                        PersonPicker {
                            options: candidates
                                .iter()
                                .cloned()
                                .map(|person| PersonOption { person, selected: false, detail: None })
                                .collect::<Vec<_>>(),
                            on_toggle: {
                                let candidates = candidates.clone();
                                move |id: PersonId| {
                                    if let Some(person) = candidates.iter().find(|p| p.id == id) {
                                        add_payer.call(person.clone());
                                    }
                                }
                            },
                        }
                    }
                }
            },
            Some(Sheet::Method(index)) => match payer_list.get(index).cloned() {
                Some(payer) => rsx! {
                    MethodSheet {
                        payer: payer.clone(),
                        methods: methods_for(&payer.person, &data.methods),
                        on_select: move |method: Option<PaymentMethodId>| {
                            sheet.set(None);
                            if let Some(entry) = payers.write().get_mut(index) {
                                entry.method = method;
                            }
                        },
                        on_close: move |_| sheet.set(None),
                    }
                },
                None => rsx! {},
            },
            None => rsx! {},
        }
    }
}

/// The total in the base currency with the rate's day, and a warning when
/// the archive has no rate of the expense's day yet (EXP-07).
#[component]
fn BasePreview(total: Option<Money>, near: Option<NearRate>, base: Currency) -> Element {
    let format = NumberFormat::current();
    let line = near.as_ref().map(|near| {
        let converted = total
            .and_then(|t| fx::convert(t, &near.quote.rate).ok())
            .unwrap_or(Money::zero(base));
        t!(
            "expense.base_preview",
            amount = format_money(converted, format),
            date = display_date(near.quote.rate_date.as_deref().unwrap_or_default())
        )
        .to_string()
    });
    let missing = near.as_ref().is_none_or(|near| near.later);

    rsx! {
        if let Some(line) = line {
            p { class: "px-1 text-sm tabular-nums text-floral-white-400", aria_live: "polite", "{line}" }
        }
        if missing {
            div {
                class: "flex items-start gap-3 rounded-2xl bg-pale-oak-900 px-4 py-3 text-sm text-pale-oak-200",
                role: "status",
                Icon { icon: LdTriangleAlert, class: "mt-0.5 h-5 w-5 shrink-0" }
                span { {t!("expense.rate_missing").to_string()} }
            }
        }
    }
}

/// One payer: who, how much and with what. With a single payer the amount
/// is the total and not editable.
#[component]
fn PayerRow(
    payer: PayerDraft,
    amount: Money,
    text: String,
    editable: bool,
    manual: bool,
    invalid: bool,
    method: Option<PaymentMethod>,
    on_amount: EventHandler<String>,
    on_method: EventHandler<()>,
    on_remove: EventHandler<()>,
) -> Element {
    let format = NumberFormat::current();
    let name = payer.person.name.clone();
    // Until a part is edited, the fields show the equal parts.
    let shown = if manual {
        text
    } else {
        amount_text(amount, format)
    };
    let method_label = method.as_ref().map_or_else(
        || t!("expense.choose_method").to_string(),
        |m| m.name.clone(),
    );
    let method_icon = method.as_ref().map(|m| m.icon.clone());

    rsx! {
        div { class: "flex flex-col gap-1 border-b border-jet-black-800 px-3 py-2 last:border-b-0",
            div { class: "flex min-h-12 items-center gap-3",
                Avatar { name: name.clone(), color: payer.person.color.clone(), size: AvatarSize::Sm }
                span { class: "min-w-0 flex-1 truncate text-base text-floral-white-50", "{name}" }
                if editable {
                    CompactAmountInput {
                        id: "payer-{payer.person.id.as_str()}",
                        label: t!("expense.payer_amount", name = name).to_string(),
                        value: shown,
                        currency: amount.currency(),
                        invalid,
                        oninput: on_amount,
                    }
                    button {
                        class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-floral-white-400 active:bg-jet-black-800 transition-colors",
                        r#type: "button",
                        aria_label: t!("expense.remove_payer", name = name).to_string(),
                        onclick: move |_| on_remove.call(()),
                        Icon { icon: LdX, class: "h-5 w-5" }
                    }
                } else {
                    MoneyText { amount, class: "text-base font-semibold text-floral-white-100" }
                }
            }
            button {
                class: "flex min-h-11 items-center gap-2 self-start rounded-full bg-jet-black-800 px-3 text-sm text-floral-white-200 active:bg-jet-black-700 transition-colors ease-apple",
                r#type: "button",
                aria_label: "{t!(\"expense.method\")}: {method_label}",
                onclick: move |_| on_method.call(()),
                if let Some(icon) = method_icon {
                    PaymentIconGlyph { icon, class: "h-4 w-4".to_string() }
                }
                span { class: if method.is_some() { "" } else { "text-floral-white-400" }, "{method_label}" }
                Icon { icon: LdChevronRight, class: "h-4 w-4 text-floral-white-500" }
            }
        }
    }
}

/// Picks the payment method of one payer (EXP-03); "no answer" is allowed,
/// because other people's methods are often unknown.
#[component]
fn MethodSheet(
    payer: PayerDraft,
    methods: Vec<PaymentMethod>,
    on_select: EventHandler<Option<PaymentMethodId>>,
    on_close: EventHandler<()>,
) -> Element {
    rsx! {
        BottomSheet { title: t!("expense.method").to_string(), on_close,
            div { class: "flex max-h-[70vh] flex-col gap-1 overflow-y-auto overscroll-contain px-3 pt-2", role: "listbox",
                ChoiceRow {
                    label: t!("expense.no_method").to_string(),
                    selected: payer.method.is_none(),
                    onclick: move |_| on_select.call(None),
                    span { class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-jet-black-800 text-floral-white-400",
                        Icon { icon: LdX, class: "h-5 w-5" }
                    }
                }
                for method in methods.iter().cloned() {
                    ChoiceRow {
                        key: "{method.id.as_str()}",
                        label: method.name.clone(),
                        selected: payer.method.as_ref() == Some(&method.id),
                        onclick: move |_| on_select.call(Some(method.id.clone())),
                        PaymentMethodIcon { icon: method.icon.clone(), color: method.color.clone() }
                    }
                }
                if methods.is_empty() {
                    p { class: "px-3 py-2 text-sm text-floral-white-400",
                        {t!("expense.no_methods_text", name = payer.person.name).to_string()}
                    }
                }
            }
        }
    }
}

/// Selectable row of a sheet with a leading icon (`children`).
#[component]
fn ChoiceRow(
    label: String,
    selected: bool,
    onclick: EventHandler<()>,
    children: Element,
) -> Element {
    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-3 rounded-2xl px-3 text-left active:bg-jet-black-800 transition-colors ease-apple",
            class: if selected { "bg-cerulean-900" },
            r#type: "button",
            role: "option",
            aria_selected: if selected { "true" } else { "false" },
            onclick: move |_| onclick.call(()),
            {children}
            span { class: "min-w-0 flex-1 truncate text-base text-floral-white-50", "{label}" }
            if selected {
                Icon { icon: LdCheck, class: "h-5 w-5 shrink-0 text-cerulean-300" }
            }
        }
    }
}

/// Stands for "no group" where a group would show its icon.
#[component]
fn NoGroupIcon() -> Element {
    rsx! {
        span { class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl bg-jet-black-800 text-floral-white-300",
            aria_hidden: "true",
            Icon { icon: LdUser, class: "h-5 w-5" }
        }
    }
}

fn load(db: &Db) -> Result<FormData, StorageError> {
    let me = db.me()?.ok_or(StorageError::NotFound)?;
    let home_currency = db
        .profile()?
        .map_or_else(default_home_currency, |p| p.home_currency);
    let groups = db.groups()?;
    let group = preselected_group(db.setting(LAST_EXPENSE_GROUP)?.as_deref(), &groups);
    let methods = db
        .payment_methods()?
        .into_iter()
        .filter(|m| !m.archived)
        .collect();
    Ok(FormData {
        me,
        home_currency,
        groups,
        categories: db.categories()?,
        methods,
        group,
        currency: db.currency_setting(LAST_EXPENSE_CURRENCY)?,
    })
}

/// The group of the last expense if it still exists (`""` = it had none),
/// otherwise the newest group. `GRP-05` (active group) will replace this.
fn preselected_group(last: Option<&str>, groups: &[Group]) -> Option<GroupId> {
    match last {
        Some("") => None,
        Some(id) if groups.iter().any(|g| g.id.as_str() == id) => Some(GroupId::new(id)),
        _ => groups.first().map(|g| g.id.clone()),
    }
}

/// Who can pay and share: the group's members, or only "Ich" for a
/// personal expense (user decision in AP-11).
fn people_of(db: &Db, group: Option<&GroupId>, me: &Person) -> Result<Vec<Person>, StorageError> {
    match group {
        Some(id) => Ok(db
            .group_members(id)?
            .into_iter()
            .map(|member| member.person)
            .collect()),
        None => Ok(vec![me.clone()]),
    }
}

fn base_of(groups: &[Group], group: Option<&GroupId>, home: Currency) -> Currency {
    group
        .and_then(|id| groups.iter().find(|g| &g.id == id))
        .map_or(home, |g| g.base_currency)
}

/// "Ich" pays by default, or the first person if "Ich" is not there.
fn default_payer(people: &[Person], me: &Person, methods: &[PaymentMethod]) -> PayerDraft {
    let person = people
        .iter()
        .find(|p| p.id == me.id)
        .or_else(|| people.first())
        .unwrap_or(me)
        .clone();
    PayerDraft {
        method: default_method(&person, methods),
        person,
        amount_text: String::new(),
    }
}

/// Preselected only when the choice is obvious: the person has exactly one
/// method.
fn default_method(person: &Person, methods: &[PaymentMethod]) -> Option<PaymentMethodId> {
    match methods_for(person, methods).as_slice() {
        [only] => Some(only.id.clone()),
        _ => None,
    }
}

/// Methods a person can pay with: their own and those without owner.
fn methods_for(person: &Person, methods: &[PaymentMethod]) -> Vec<PaymentMethod> {
    methods
        .iter()
        .filter(|m| {
            m.owner_person_id
                .as_ref()
                .is_none_or(|owner| owner == &person.id)
        })
        .cloned()
        .collect()
}

/// What each payer paid: equal parts of the total until edited by hand.
fn payer_amounts(
    payers: &[PayerDraft],
    manual: bool,
    total_minor: i64,
    currency: Currency,
    format: NumberFormat,
) -> Vec<i64> {
    if manual {
        payers
            .iter()
            .map(|p| parse_amount(&p.amount_text, currency, format).map_or(0, |m| m.amount_minor()))
            .collect()
    } else {
        equal_parts(total_minor, payers.len())
    }
}

/// `total` split into `count` parts without losing a unit (idee.md 8.4);
/// the first parts get the leftover units.
fn equal_parts(total: i64, count: usize) -> Vec<i64> {
    let weights: BTreeMap<usize, Decimal> = (0..count).map(|i| (i, Decimal::ONE)).collect();
    match allocate(total, &weights) {
        Ok(parts) => parts.into_values().collect(),
        Err(_) => vec![0; count],
    }
}

fn payments_error_text(error: &ExpenseError, currency: Currency, format: NumberFormat) -> String {
    match error {
        ExpenseError::PaymentsMismatch { expected, actual } => t!(
            "expense.payments_mismatch",
            paid = format_money(Money::new(*actual, currency), format),
            total = format_money(Money::new(*expected, currency), format)
        )
        .to_string(),
        ExpenseError::NonPositivePayment => t!("expense.payment_positive").to_string(),
        ExpenseError::NoPayer => t!("expense.payer_required").to_string(),
        other => other.to_string(),
    }
}

fn save_error_text(error: &SaveExpenseError) -> String {
    match error {
        SaveExpenseError::NoRate { from, to } => {
            t!("expense.no_rate", from = from.code(), to = to.code()).to_string()
        }
        SaveExpenseError::Storage(error) => format!("{} {error}", t!("expense.save_error")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: &str) -> Group {
        Group {
            id: GroupId::new(id),
            name: id.into(),
            icon: "plane".into(),
            color: "cerulean".into(),
            base_currency: Currency::from_code("JPY").unwrap(),
            start_date: None,
            end_date: None,
        }
    }

    #[test]
    fn equal_parts_lose_no_unit() {
        assert_eq!(equal_parts(4_000, 2), [2_000, 2_000]);
        assert_eq!(equal_parts(1_000, 3), [334, 333, 333]);
        assert_eq!(equal_parts(0, 2), [0, 0]);
        assert!(equal_parts(100, 0).is_empty());
    }

    #[test]
    fn preselects_last_or_newest_group() {
        let groups = [group("new"), group("old")];
        assert_eq!(preselected_group(None, &groups), Some(GroupId::new("new")));
        assert_eq!(
            preselected_group(Some("old"), &groups),
            Some(GroupId::new("old"))
        );
        assert_eq!(preselected_group(Some(""), &groups), None);
        assert_eq!(
            preselected_group(Some("deleted"), &groups),
            Some(GroupId::new("new"))
        );
        assert_eq!(preselected_group(None, &[]), None);
    }

    #[test]
    fn base_currency_of_group_or_home() {
        let eur = Currency::from_code("EUR").unwrap();
        let groups = [group("japan")];
        assert_eq!(
            base_of(&groups, Some(&GroupId::new("japan")), eur).code(),
            "JPY"
        );
        assert_eq!(base_of(&groups, None, eur), eur);
    }
}
