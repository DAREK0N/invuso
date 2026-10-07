use std::collections::BTreeMap;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdCircleAlert, LdCopy, LdMapPin, LdPencil, LdReceipt, LdStickyNote, LdTrash2,
    },
};
use invuso_core::Decimal;
use invuso_core::domain::{
    Category, Expense, ExpenseId, Group, Money, Person, PersonId, local_date,
};
use invuso_core::split::SplitMode;

use super::items::{ReceiptItems, drafts_from_saved};
use super::split::SplitKind;
use crate::Route;
use crate::clock;
use crate::components::{
    Avatar, AvatarSize, Button, ButtonVariant, CategoryIconGlyph, EmptyState, ErrorBanner,
    ImageViewer, MoneyText, PaymentIconGlyph, TopBar,
};
use crate::format::{NumberFormat, coordinates_text, format_number, format_rate};
use crate::preferences::{category_name, day_heading, display_date};
use crate::services::receipts;
use crate::state::{DataRevision, ToastAction, Toaster};
use crate::storage::{Db, ExpenseParties, RateQuote, ReceiptFiles};

/// Everything the detail shows of one expense.
#[derive(Debug, Clone, PartialEq)]
struct Detail {
    expense: Expense,
    group: Option<Group>,
    category: Option<Category>,
    parties: ExpenseParties,
    /// The archived rate the expense was converted with (FX-04).
    rate: Option<RateQuote>,
    /// The attached receipt (RCP-03).
    receipt: Option<ReceiptFiles>,
    /// Own-currency shares, and both shares and payments in the base
    /// currency, each adding up exactly (idee.md 8.2, 8.4).
    shares: BTreeMap<PersonId, i64>,
    shares_in_base: BTreeMap<PersonId, i64>,
    payments_in_base: BTreeMap<PersonId, i64>,
}

/// What loading the expense found.
#[derive(Debug, Clone, PartialEq)]
enum Loaded {
    Found(Box<Detail>),
    /// Deleted, here or in the edit form opened from here.
    Deleted,
    Missing,
}

/// `/expense/:id`: all about one expense – amounts, rate, who paid with
/// what and everyone's share, its line items, the receipt in full screen
/// (RCP-04) – with editing and deleting (GRP-22).
#[component]
pub fn ExpenseDetail(id: String) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let nav = use_navigator();
    let today = use_hook(|| clock::local_now().0);
    let mut delete_error = use_signal(|| None::<String>);

    let expense_id = use_memo(use_reactive!(|id| ExpenseId::new(id)));
    let load_db = db.clone();
    let data = use_memo(move || {
        revision.track();
        load(&load_db, &expense_id()).map_err(|e| e.to_string())
    });

    // A deleted expense leaves its detail: after deleting here, and when
    // coming back from the edit form that deleted it.
    use_effect(move || {
        if matches!(&*data.read(), Ok(Loaded::Deleted)) {
            if nav.can_go_back() {
                nav.go_back();
            } else {
                nav.replace(Route::Home {});
            }
        }
    });

    let delete = use_callback(move |expense: Expense| {
        let (mut revision, mut toaster) = (revision, toaster);
        if let Err(e) = db.delete_expense(&expense.id) {
            delete_error.set(Some(format!("{} {e}", t!("expense.delete_error"))));
            return;
        }
        let undo_db = db.clone();
        let id = expense.id.clone();
        let undo = move || {
            let (mut revision, mut toaster) = (revision, toaster);
            match undo_db.restore_expense(&id) {
                Ok(()) => revision.bump(),
                Err(e) => toaster.show(format!("{} {e}", t!("expense.restore_error")), None),
            }
        };
        toaster.show(
            t!("expense.deleted", title = expense.title).to_string(),
            Some(ToastAction {
                label: t!("common.undo").to_string(),
                run: Rc::new(undo),
            }),
        );
        revision.bump();
    });

    rsx! {
        TopBar { title: t!("page.expense_detail").to_string(), show_back: true }
        match &*data.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("expense_detail.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(Loaded::Missing | Loaded::Deleted) => rsx! {
                EmptyState {
                    title: t!("expense.not_found_title").to_string(),
                    text: t!("expense.not_found_text").to_string(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(Loaded::Found(detail)) => {
                let detail = detail.as_ref().clone();
                let expense = detail.expense.clone();
                let edit_id = expense.id.as_str().to_string();
                let copy_id = edit_id.clone();
                rsx! {
                    div { class: "mx-4 flex flex-col gap-5 pt-6 safe-area-x",
                        Head { detail: detail.clone(), today: today.clone() }
                        NoteAndPlace { expense: expense.clone() }
                        if let Some(receipt) = detail.receipt.clone() {
                            ReceiptCard { receipt }
                        }
                        PaymentsSection { detail: detail.clone() }
                        SharesSection { detail: detail.clone() }
                        if !expense.line_items.is_empty() {
                            ReceiptItems {
                                items: drafts_from_saved(&expense.line_items),
                                currency: expense.total.currency(),
                                total: Some(expense.total),
                                people: detail.parties.people.values().cloned().collect::<Vec<_>>(),
                                assignable: matches!(expense.split, SplitMode::Items { .. }),
                            }
                        }
                        ErrorBanner { error: delete_error() }
                        div { class: "flex gap-3",
                            Button {
                                variant: ButtonVariant::Secondary,
                                class: "flex-1",
                                onclick: move |_| {
                                    nav.push(Route::ExpenseEdit { id: edit_id.clone() });
                                },
                                Icon { icon: LdPencil, class: "h-5 w-5" }
                                {t!("common.edit").to_string()}
                            }
                            // A new expense filled from this one (EXP-12).
                            Button {
                                variant: ButtonVariant::Secondary,
                                class: "flex-1",
                                onclick: move |_| {
                                    nav.push(Route::ExpenseNew {
                                        // The copy's group comes with it; none
                                        // given, saving leads to its timeline.
                                        group: String::new(),
                                        receipt: String::new(),
                                        copy: copy_id.clone(),
                                    });
                                },
                                Icon { icon: LdCopy, class: "h-5 w-5" }
                                {t!("expense.duplicate").to_string()}
                            }
                        }
                        Button {
                            variant: ButtonVariant::Danger,
                            class: "w-full",
                            onclick: move |_| delete.call(expense.clone()),
                            Icon { icon: LdTrash2, class: "h-5 w-5" }
                            {t!("common.delete").to_string()}
                        }
                    }
                }
            }
        }
    }
}

/// Category, title, when and where, the amount in both currencies and the
/// rate it was converted with.
#[component]
fn Head(detail: Detail, today: String) -> Element {
    let expense = &detail.expense;
    let format = NumberFormat::current();
    let time = expense.occurred_at.get(11..16).unwrap_or_default();
    let when = format!(
        "{} · {time}",
        day_heading(local_date(&expense.occurred_at), &today)
    );
    let context = detail
        .category
        .iter()
        .map(category_name)
        .chain(detail.group.iter().map(|g| g.name.clone()))
        .collect::<Vec<_>>()
        .join(" · ");
    let icon = detail
        .category
        .as_ref()
        .map(|c| c.icon.clone())
        .unwrap_or_default();
    let foreign = expense.total.currency() != expense.total_in_base.currency();
    let rate_text = detail.rate.as_ref().map(|quote| {
        // Shown in the direction with a rate ≥ 1, e.g. "1 EUR = 177,71 JPY"
        // rather than "1 JPY = 0,0056271 EUR".
        let rate = if quote.rate.value() < Decimal::ONE {
            quote.rate.inverse()
        } else {
            quote.rate
        };
        let line = t!(
            "converter.rate_line",
            base = rate.base().code(),
            rate = format_rate(rate.value(), format),
            quote = rate.quote().code()
        )
        .to_string();
        let line = match &quote.rate_date {
            Some(date) => format!(
                "{line} · {}",
                t!("converter.rate_date", date = display_date(date))
            ),
            None => line,
        };
        if quote.is_manual() {
            format!("{line} · {}", t!("expense_detail.own_rate"))
        } else {
            line
        }
    });

    rsx! {
        div { class: "flex flex-col items-center gap-2 text-center",
            span {
                class: "flex h-14 w-14 items-center justify-center rounded-full bg-jet-black-800 text-floral-white-200",
                aria_hidden: "true",
                CategoryIconGlyph { icon, class: "h-7 w-7".to_string() }
            }
            h2 { class: "text-2xl font-semibold break-words text-floral-white-50", "{expense.title}" }
            p { class: "text-sm text-floral-white-400", "{when}" }
            if !context.is_empty() {
                p { class: "text-sm text-floral-white-400", "{context}" }
            }
            MoneyText { amount: expense.total_in_base, class: "mt-2 text-3xl font-semibold text-floral-white-50" }
            if foreign {
                MoneyText { amount: expense.total, class: "text-base text-floral-white-300" }
            }
            if let Some(rate_text) = rate_text {
                p { class: "text-sm tabular-nums text-floral-white-400", "{rate_text}" }
            }
        }
    }
}

/// Where the expense happened and the note about it (EXP-10); nothing when
/// neither was given.
#[component]
fn NoteAndPlace(expense: Expense) -> Element {
    let point = expense
        .coordinates
        .map(|p| coordinates_text(p, NumberFormat::current()));
    if expense.location.is_none() && point.is_none() && expense.note.is_none() {
        return rsx! {};
    }
    let row = "flex items-start gap-3 px-4 py-3";
    rsx! {
        div { class: "flex flex-col divide-y divide-jet-black-800 overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
            if expense.location.is_some() || point.is_some() {
                div { class: row,
                    Icon { icon: LdMapPin, class: "mt-0.5 h-5 w-5 shrink-0 text-floral-white-400" }
                    div { class: "flex min-w-0 flex-col",
                        span { class: "sr-only", {t!("expense_detail.location").to_string()} }
                        if let Some(place) = &expense.location {
                            span { class: "text-base break-words text-floral-white-100", "{place}" }
                        }
                        if let Some(point) = &point {
                            span { class: "text-sm tabular-nums text-floral-white-400", "{point}" }
                        }
                    }
                }
            }
            if let Some(note) = &expense.note {
                div { class: row,
                    Icon { icon: LdStickyNote, class: "mt-0.5 h-5 w-5 shrink-0 text-floral-white-400" }
                    span { class: "sr-only", {t!("expense_detail.note").to_string()} }
                    p { class: "min-w-0 whitespace-pre-line break-words text-base text-floral-white-100", "{note}" }
                }
            }
        }
    }
}

/// The receipt's thumbnail; tapping shows the image in full screen with
/// zoom (RCP-04): the corrected copy if there is one (RCP-05). With
/// `on_open` the caller shows the image itself, e.g. with the read lines
/// marked (OCR-37).
#[component]
pub(super) fn ReceiptCard(
    receipt: ReceiptFiles,
    #[props(default)] on_open: Option<EventHandler<()>>,
) -> Element {
    let mut open = use_signal(|| false);
    let original = receipt.page(0).map(receipts::file_url);
    let has_original = original.is_some();
    let label = t!("receipt.open").to_string();

    rsx! {
        section { class: "flex flex-col gap-2",
            h2 { class: "px-1 text-sm font-medium text-floral-white-300", {t!("receipt.title").to_string()} }
            button {
                class: "flex w-full items-center justify-center overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900 active:opacity-80 transition-opacity ease-apple",
                r#type: "button",
                aria_label: "{label}",
                disabled: !has_original,
                onclick: move |_| match on_open {
                    Some(on_open) => on_open.call(()),
                    None => open.set(true),
                },
                match &receipt.thumbnail_path {
                    Some(path) => rsx! {
                        img {
                            class: "max-h-64 w-full object-cover object-top",
                            src: receipts::file_url(path),
                            alt: "",
                        }
                    },
                    None => rsx! {
                        span { class: "flex min-h-24 items-center gap-3 px-4 text-floral-white-200",
                            Icon { icon: LdReceipt, class: "h-6 w-6" }
                            span { class: "text-base", "{label}" }
                        }
                    },
                }
            }
        }
        if let (true, Some(src)) = (open(), original) {
            ImageViewer {
                src,
                alt: t!("receipt.title").to_string(),
                on_close: move |_| open.set(false),
            }
        }
    }
}

/// Who paid how much with what (EXP-02, EXP-03).
#[component]
fn PaymentsSection(detail: Detail) -> Element {
    let expense = &detail.expense;
    let rows: Vec<PartyLine> = expense
        .payments
        .iter()
        .map(|payment| {
            let method = payment
                .payment_method_id
                .as_ref()
                .and_then(|id| detail.parties.methods.get(id));
            PartyLine {
                key: payment.person_id.as_str().to_string(),
                person: detail.parties.people.get(&payment.person_id).cloned(),
                note: method.map(|m| m.name.clone()),
                method_icon: method.map(|m| m.icon.clone()),
                base: base_amount(expense, &detail.payments_in_base, &payment.person_id),
                original: payment.amount,
            }
        })
        .collect();

    rsx! {
        PartySection { title: t!("expense.paid_by").to_string(), rows }
    }
}

/// Everyone's share and how the split mode got there (EXP-04, idee.md 8.1).
#[component]
fn SharesSection(detail: Detail) -> Element {
    let expense = &detail.expense;
    let format = NumberFormat::current();
    let currency = expense.total.currency();
    let rows: Vec<PartyLine> = detail
        .shares
        .iter()
        .map(|(person, share)| PartyLine {
            key: person.as_str().to_string(),
            person: detail.parties.people.get(person).cloned(),
            note: mode_note(&expense.split, person, format),
            method_icon: None,
            base: base_amount(expense, &detail.shares_in_base, person),
            original: Money::new(*share, currency),
        })
        .collect();
    let title = format!(
        "{} · {}",
        t!("expense.split_between"),
        mode_kind(&expense.split).label()
    );

    rsx! {
        PartySection { title, rows }
    }
}

/// A person's line in the payments or shares list.
#[derive(Debug, Clone, PartialEq)]
struct PartyLine {
    key: String,
    /// `None` if the person cannot be found at all.
    person: Option<Person>,
    /// Payment method or split detail, e.g. "70 %".
    note: Option<String>,
    method_icon: Option<String>,
    base: Money,
    original: Money,
}

#[component]
fn PartySection(title: String, rows: Vec<PartyLine>) -> Element {
    rsx! {
        section { class: "flex flex-col gap-2",
            h2 { class: "px-1 text-sm font-medium text-floral-white-300", "{title}" }
            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                for row in rows {
                    PartyRow { key: "{row.key}", row: row.clone() }
                }
            }
        }
    }
}

#[component]
fn PartyRow(row: PartyLine) -> Element {
    let (name, color) = match &row.person {
        Some(person) => (person.name.clone(), person.color.clone()),
        None => (
            t!("expense_detail.unknown_person").to_string(),
            String::new(),
        ),
    };
    let foreign = row.base.currency() != row.original.currency();

    rsx! {
        div { class: "flex min-h-16 items-center gap-3 border-b border-jet-black-800 px-4 py-2 last:border-b-0",
            Avatar { name: name.clone(), color, size: AvatarSize::Md }
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "truncate text-base text-floral-white-50", "{name}" }
                if let Some(note) = row.note {
                    span { class: "flex min-w-0 items-center gap-1 text-sm text-floral-white-400",
                        if let Some(icon) = row.method_icon {
                            PaymentIconGlyph { icon, class: "h-4 w-4 shrink-0".to_string() }
                        }
                        span { class: "truncate", "{note}" }
                    }
                }
            }
            span { class: "flex shrink-0 flex-col items-end",
                MoneyText { amount: row.base, class: "text-base font-semibold text-floral-white-100" }
                if foreign {
                    MoneyText { amount: row.original, class: "text-sm text-floral-white-400" }
                }
            }
        }
    }
}

/// Loads the expense with everything around it.
fn load(db: &Db, id: &ExpenseId) -> Result<Loaded, Box<dyn std::error::Error>> {
    let Some(expense) = db.expense(id)? else {
        return Ok(if db.is_expense_deleted(id)? {
            Loaded::Deleted
        } else {
            Loaded::Missing
        });
    };
    let group = match &expense.group_id {
        Some(group) => db.group(group)?,
        None => None,
    };
    let category = match &expense.category_id {
        Some(category) => db.all_categories()?.into_iter().find(|c| &c.id == category),
        None => None,
    };
    let rate = match &expense.fx_rate_id {
        Some(rate) => db.archived_rate(
            rate,
            expense.total.currency(),
            expense.total_in_base.currency(),
        )?,
        None => None,
    };
    let receipt = match &expense.receipt_id {
        Some(receipt) => db.receipt(receipt)?,
        None => None,
    };
    Ok(Loaded::Found(Box::new(Detail {
        receipt,
        parties: db.expense_parties(&expense)?,
        group,
        category,
        rate,
        shares: expense.shares()?,
        shares_in_base: expense.shares_in_base()?,
        payments_in_base: expense.payments_in_base()?,
        expense,
    })))
}

/// The person's part in the base currency (0 if not part of `parts`).
fn base_amount(expense: &Expense, parts: &BTreeMap<PersonId, i64>, person: &PersonId) -> Money {
    Money::new(
        parts.get(person).copied().unwrap_or_default(),
        expense.total_in_base.currency(),
    )
}

fn mode_kind(mode: &SplitMode) -> SplitKind {
    match mode {
        SplitMode::Equal(_) => SplitKind::Equal,
        SplitMode::Weights(_) => SplitKind::Weights,
        SplitMode::Percent(_) => SplitKind::Percent,
        SplitMode::Exact(_) => SplitKind::Exact,
        SplitMode::Items { .. } => SplitKind::Items,
    }
}

/// What the split mode says about the person: "Anteile: 2", "70 %"; nothing
/// for equal parts and exact amounts, where the amount says it all.
fn mode_note(mode: &SplitMode, person: &PersonId, format: NumberFormat) -> Option<String> {
    match mode {
        SplitMode::Weights(weights) => weights
            .get(person)
            .map(|w| t!("expense_detail.weight", value = format_number(*w, format)).to_string()),
        SplitMode::Percent(percents) => percents
            .get(person)
            .map(|p| t!("expense_detail.percent", value = format_number(*p, format)).to_string()),
        SplitMode::Equal(_) | SplitMode::Exact(_) | SplitMode::Items { .. } => None,
    }
}
