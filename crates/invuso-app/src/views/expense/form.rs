use std::collections::BTreeMap;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus::router::Navigator;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdCamera, LdCheck, LdChevronRight, LdCircleAlert, LdImage, LdPlus, LdTrash2,
        LdTriangleAlert, LdUser, LdX,
    },
};
use invuso_core::Decimal;
use invuso_core::domain::{
    Category, Currency, Expense, ExpenseError, ExpenseId, ExpenseSource, Group, GroupId,
    GroupMember, LineItem, LineItemError, Money, PaymentMethod, PaymentMethodId, Person, PersonId,
    is_iso_date, validate_participants, validate_payments, validate_split,
};
use invuso_core::fx;
use invuso_core::receipt::{ParsedReceipt, detect_language};
use invuso_core::split::allocate;

use super::detail::ReceiptCard;
use super::items::{
    self, ItemAction, ItemDraft, ItemSheet, ReceiptItems, TranslationNote, drafts_from_parsed,
    drafts_from_saved,
};
use super::recognition::ReceiptRecognition;
use super::split::{ShareRow, SplitDraft, SplitKind, split_error_text, sum_hint};
use crate::Route;
use crate::clock;
use crate::components::{
    AmountInput, Avatar, AvatarSize, BottomSheet, Button, ButtonVariant, CategoryIconGlyph, Chip,
    CompactAmountInput, CurrencyPicker, DateTimeField, EmptyState, ErrorBanner, GroupIcon,
    MoneyText, PaymentIconGlyph, PaymentMethodIcon, PersonOption, PersonPicker, TextField, TopBar,
};
use crate::format::{NumberFormat, amount_text, fit_amount_text, format_money, parse_amount};
use crate::platform::{ImageKind, system_translator};
use crate::preferences::{
    category_name, default_home_currency, display_date, language_name, suggested_target_language,
};
use crate::services::expenses::{SaveExpenseError, save_expense, update_expense};
use crate::services::rates::{CurrencyApi, Frankfurter};
use crate::services::receipts::{self, capture_receipt};
use crate::services::translation::{MachineTranslation, remember_review, translate_lines};
use crate::state::{DataRevision, ToastAction, Toaster};
use crate::storage::{
    Db, LAST_EXPENSE_CURRENCY, LAST_EXPENSE_GROUP, NearRate, NewExpense, NewExpensePayment,
    ReceiptFiles, StorageError,
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
    /// Preselected group: the active one (GRP-05), else the one of the last
    /// expense, else the newest; when editing, the expense's group.
    group: Option<GroupId>,
    /// Currency of the last expense, if any.
    currency: Option<Currency>,
    /// The expense being edited (EXP-05); `None` for a new one.
    existing: Option<Expense>,
    /// Group whose timeline opened the form (GRP-23).
    opened_from: Option<GroupId>,
    /// Receipt attached to the expense (RCP-03): the saved one when
    /// editing, or the one just photographed (`/scan`).
    receipt: Option<ReceiptFiles>,
    /// Opened to check a scanned receipt (idee.md 7.2 step 5): starts
    /// split by line items and saves the expense as scanned.
    review: bool,
    /// Global target language of translations (SET-02); a group's own one
    /// takes precedence (TRL-05).
    target_language: String,
    /// Language the attached receipt was detected in (TRL-02).
    receipt_language: Option<String>,
}

/// Translation of the recognized lines (idee.md 7.2 step 4).
#[derive(Debug, Clone, PartialEq)]
enum TranslationState {
    Running,
    Finished {
        source: Option<String>,
        target: String,
        machine: MachineTranslation,
    },
    Failed(String),
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
    /// The line item with this key.
    Item(u64),
}

/// `/expense/new`: records an expense by hand (EXP-01..04, EXP-06, EXP-07;
/// idee.md 7.3), in `group` if given (GRP-23), with `receipt` attached if
/// given (RCP-03). Saving leads to the group's timeline.
#[component]
pub fn ExpenseNew(group: String, receipt: String) -> Element {
    let db = use_context::<Db>();
    let data = use_hook(|| {
        let preset = (!group.is_empty()).then(|| GroupId::new(group));
        let receipt = (!receipt.is_empty()).then_some(receipt);
        load(&db, None, preset, receipt.as_deref()).map_err(|e| e.to_string())
    });

    rsx! {
        TopBar { title: t!("page.expense_new").to_string(), show_back: true }
        match data {
            Err(message) => rsx! { LoadError { message } },
            Ok(data) => rsx! { ExpenseForm { data } },
        }
    }
}

/// `/expense/:id/edit`: the same form, filled with the saved expense
/// (EXP-05); also deletes it with undo.
#[component]
pub fn ExpenseEdit(id: String) -> Element {
    let db = use_context::<Db>();
    // `None` stands for "not found".
    let data = use_hook(|| {
        load(&db, Some(&ExpenseId::new(id)), None, None).map_err(|e| match e {
            StorageError::NotFound => None,
            other => Some(other.to_string()),
        })
    });

    rsx! {
        TopBar { title: t!("page.expense_edit").to_string(), show_back: true }
        match data {
            Err(None) => rsx! {
                EmptyState {
                    title: t!("expense.not_found_title").to_string(),
                    text: t!("expense.not_found_text").to_string(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Err(Some(message)) => rsx! { LoadError { message } },
            Ok(data) => rsx! { ExpenseForm { data } },
        }
    }
}

/// `/scan/:receipt_id/review`: the review of a scanned receipt (idee.md
/// 7.2 steps 5–6, OCR-30..35): the expense form with the receipt, its
/// recognized positions to correct and assign, split by line items.
#[component]
pub fn ReceiptReview(receipt_id: String) -> Element {
    let db = use_context::<Db>();
    let data = use_hook(|| {
        load(&db, None, None, Some(&receipt_id))
            .map(|data| FormData {
                review: true,
                ..data
            })
            .map_err(|e| e.to_string())
    });

    rsx! {
        TopBar { title: t!("page.receipt_review").to_string(), show_back: true }
        match data {
            Err(message) => rsx! { LoadError { message } },
            Ok(data) => rsx! { ExpenseForm { data } },
        }
    }
}

#[component]
fn LoadError(message: String) -> Element {
    rsx! {
        EmptyState {
            title: t!("expense.load_error_title").to_string(),
            text: message,
            Icon { icon: LdCircleAlert, class: "h-8 w-8" }
        }
    }
}

#[component]
fn ExpenseForm(data: FormData) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let nav = use_navigator();
    let existing = data.existing.clone();
    let editing = existing.is_some();
    let review = data.review;
    let opened_from = data.opened_from.clone();

    let initial_people =
        use_hook(|| people_of(&db, data.group.as_ref(), &data.me).map_err(|e| e.to_string()));
    let start_people = initial_people
        .clone()
        .unwrap_or_else(|_| vec![member(data.me.clone())]);

    let start_currency = data
        .currency
        .unwrap_or_else(|| base_of(&data.groups, data.group.as_ref(), data.home_currency));
    let mut amount_text_signal = use_signal(|| {
        let format = NumberFormat::current();
        existing
            .as_ref()
            .map(|e| amount_text(e.total, format))
            .unwrap_or_default()
    });
    let mut currency = use_signal(|| start_currency);
    let mut title = use_signal(|| {
        existing
            .as_ref()
            .map(|e| e.title.clone())
            .unwrap_or_default()
    });
    let mut category = use_signal(|| existing.as_ref().and_then(|e| e.category_id.clone()));
    let (start_date, start_time) = use_hook(|| match &existing {
        Some(expense) => split_occurred_at(&expense.occurred_at),
        None => clock::local_now(),
    });
    let mut date = use_signal(|| start_date.clone());
    let mut time = use_signal(|| start_time.clone());
    let mut group = use_signal(|| data.group.clone());
    let mut people = use_signal(|| start_people.clone());
    let mut payers = use_signal(|| match &existing {
        Some(expense) => saved_payers(expense, &start_people),
        None => vec![default_payer(&start_people, &data.me, &data.methods)],
    });
    let mut payers_manual = use_signal(|| existing.as_ref().is_some_and(|e| e.payments.len() > 1));
    let mut split_draft = use_signal(|| match &existing {
        Some(expense) => SplitDraft::from_mode(
            &expense.split,
            expense.total.currency(),
            NumberFormat::current(),
        ),
        None => {
            let mut draft =
                SplitDraft::equal(start_people.iter().map(|m| m.person.id.clone()).collect());
            if review && data.group.is_some() {
                draft.kind = SplitKind::Items;
            }
            draft
        }
    });
    let mut items = use_signal(|| {
        existing
            .as_ref()
            .map(|e| drafts_from_saved(&e.line_items))
            .unwrap_or_default()
    });
    let mut item_error = use_signal(|| None::<String>);
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
    let mut receipt = use_signal(|| data.receipt.clone());
    let mut picking = use_signal(|| false);
    let mut receipt_error = use_signal(|| None::<String>);
    let can_take_photo = use_hook(|| receipts::supports(ImageKind::Camera));
    // `None` until the recognition is read; then the receipt's language, if
    // it could be told (TRL-02). When editing, the stored one.
    let mut source_language = use_signal(|| editing.then(|| data.receipt_language.clone()));
    let mut translation = use_signal(|| None::<TranslationState>);
    let mut translation_run = use_signal(|| 0_u64);

    let groups = data.groups.clone();
    let home_currency = data.home_currency;
    let base_currency = use_memo(move || base_of(&groups, group().as_ref(), home_currency));
    let groups = data.groups.clone();
    let global_language = data.target_language.clone();
    let target_language = use_memo(move || {
        group()
            .and_then(|id| groups.iter().find(|g| g.id == id))
            .and_then(|g| g.target_language.clone())
            .unwrap_or_else(|| global_language.clone())
    });
    let originals = use_memo(move || {
        let mut texts: Vec<String> = items
            .read()
            .iter()
            .map(|d| d.item.original_text.clone())
            .filter(|text| !text.trim().is_empty())
            .collect();
        texts.sort();
        texts.dedup();
        texts
    });

    // Translates the recognized lines into the target language once the
    // receipt's language is known, again when the group (and so the target
    // language) changes (idee.md 7.2 step 4). A saved expense keeps the
    // translations it was saved with.
    let translate_db = db.clone();
    use_effect(move || {
        let Some(source) = source_language() else {
            return;
        };
        let (target, texts) = (target_language(), originals());
        if editing || texts.is_empty() {
            return;
        }
        let run = *translation_run.peek() + 1;
        translation_run.set(run);
        translation.set(Some(TranslationState::Running));
        let db = translate_db.clone();
        spawn(async move {
            let translator = system_translator();
            let outcome =
                translate_lines(&db, &translator, source.as_deref(), &target, &texts).await;
            // A newer run (other group, other lines) replaced this one.
            if *translation_run.peek() != run {
                return;
            }
            match outcome {
                Ok(result) => {
                    for draft in items.write().iter_mut() {
                        draft.item.translated_text =
                            result.texts.get(&draft.item.original_text).cloned();
                    }
                    translation.set(Some(TranslationState::Finished {
                        source,
                        target,
                        machine: result.machine,
                    }));
                }
                Err(error) => translation.set(Some(TranslationState::Failed(error.to_string()))),
            }
        });
    });
    let default_weights = use_memo(move || {
        people
            .read()
            .iter()
            .map(|m| (m.person.id.clone(), m.default_weight))
            .collect::<BTreeMap<_, _>>()
    });

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
                // A scanned receipt is split by its lines once there is a
                // group to split it in.
                let kind = if review && new_group.is_some() {
                    SplitKind::Items
                } else {
                    split_draft.peek().kind
                };
                let mut draft =
                    SplitDraft::equal(list.iter().map(|m| m.person.id.clone()).collect());
                draft.kind = kind;
                split_draft.set(draft);
                // Lines can only belong to members of the new group.
                for draft in items.write().iter_mut() {
                    draft
                        .item
                        .assigned_to
                        .retain(|person, _| list.iter().any(|m| &m.person.id == person));
                }
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
        split_draft.write().fit_currency(new_currency, format);
        let old_currency = *currency.peek();
        items::fit_currency(&mut items.write(), old_currency, new_currency, format);
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
    let save_existing = existing.clone();
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
        let occurred_at = match &save_existing {
            // Untouched date and time keep their original offset, e.g. the
            // one of the trip's time zone.
            Some(expense) if split_occurred_at(&expense.occurred_at) == (date(), time()) => {
                Some(expense.occurred_at.clone())
            }
            _ => clock::occurred_at(&date(), &time()),
        };
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
        let line_items = items::line_items(&items.read());
        let split = split_draft
            .read()
            .mode(&default_weights.read(), cur, format, &line_items);
        let split_check = match total {
            Some(total) => validate_split(total.amount_minor(), &split).map(|_| ()),
            None => validate_participants(&split.participants()),
        };
        if let Err(error) = split_check {
            participants_error.set(Some(split_error_text(&error, cur, format)));
            valid = false;
        }
        let (Some(total), Some(occurred_at), true) = (total, occurred_at, valid) else {
            return;
        };

        let remember = (
            receipt.read().as_ref().map(|r| r.id.clone()),
            source_language().flatten(),
            target_language(),
            line_items.clone(),
        );
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
            split,
            receipt_id: receipt.read().as_ref().map(|r| r.id.clone()),
            line_items,
            source: if review {
                ExpenseSource::Scan
            } else {
                ExpenseSource::Manual
            },
        };
        save_error.set(None);
        saving.set(true);
        let worker_db = save_db.clone();
        let remember_db = save_db.clone();
        let edit_id = save_existing.as_ref().map(|e| e.id.clone());
        let opened_from = opened_from.clone();
        let (mut revision, mut toaster) = (revision, toaster);
        spawn(async move {
            // The day's rate may have to be fetched; keep the network off
            // the UI thread.
            let editing = edit_id.is_some();
            let outcome = tokio::task::spawn_blocking(move || match edit_id {
                Some(id) => update_expense(&worker_db, &Frankfurter, &CurrencyApi, &id, new),
                None => save_expense(&worker_db, &Frankfurter, &CurrencyApi, new),
            })
            .await;
            match outcome {
                Ok(Ok(saved)) => {
                    // Corrections are a convenience for the next receipt;
                    // the expense is saved either way.
                    let (receipt_id, source, target, lines) = &remember;
                    if remember_review(
                        &remember_db,
                        receipt_id.as_deref(),
                        source.as_deref(),
                        target,
                        lines,
                    )
                    .is_err()
                    {
                        toaster.show(t!("items.remember_error").to_string(), None);
                    }
                    revision.bump();
                    let message = match (saved.later_rate, editing) {
                        (true, _) => t!("expense.saved_later_rate"),
                        (false, true) => t!("expense.updated"),
                        (false, false) => t!("expense.saved"),
                    };
                    toaster.show(message.to_string(), None);
                    let group = saved.expense.group_id.as_ref();
                    leave(nav, group, editing || group == opened_from.as_ref());
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

    let pick_db = db.clone();
    let pick = use_callback(move |kind: ImageKind| {
        picking.set(true);
        receipt_error.set(None);
        let db = pick_db.clone();
        spawn(async move {
            match capture_receipt(db, kind).await {
                Ok(Some(picked)) => receipt.set(Some(picked)),
                Ok(None) => {}
                Err(error) => receipt_error.set(Some(error.to_string())),
            }
            picking.set(false);
        });
    });

    let delete_db = db.clone();
    let delete_existing = existing.clone();
    let delete = move |_| {
        let Some(expense) = delete_existing.clone() else {
            return;
        };
        let (mut revision, mut toaster) = (revision, toaster);
        if let Err(e) = delete_db.delete_expense(&expense.id) {
            save_error.set(Some(format!("{} {e}", t!("expense.delete_error"))));
            return;
        }
        revision.bump();
        let undo_db = delete_db.clone();
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
        leave(nav, expense.group_id.as_ref(), true);
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
    let draft = split_draft();
    let item_list = items();
    let line_items = items::line_items(&item_list);
    let split = draft.mode(&default_weights.read(), cur, format, &line_items);
    let shares = if total_minor > 0 {
        validate_split(total_minor, &split).unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    let hint = sum_hint(&split, Money::new(total_minor, cur), format);
    let selected = draft.participants.clone();
    let current_group = group();
    let group_entry = current_group
        .as_ref()
        .and_then(|id| data.groups.iter().find(|g| &g.id == id))
        .cloned();
    let members = people();
    let candidates: Vec<Person> = members
        .iter()
        .map(|m| m.person.clone())
        .filter(|p| !payer_list.iter().any(|payer| payer.person.id == p.id))
        .collect();
    let all_selected = members.iter().all(|m| selected.contains(&m.person.id));
    let by_items = current_group.is_some() && draft.kind == SplitKind::Items;
    // A personal expense shows the lines it has, without assigning them.
    let show_items = by_items || (current_group.is_none() && (review || !item_list.is_empty()));

    // What the recognition read fills what is still empty (idee.md 7.2
    // step 3): the amount, in review also from the lines' sum, and the lines.
    let on_read = move |parsed: ParsedReceipt| {
        let total = parsed
            .total
            .or_else(|| parsed.items_sum().ok().filter(|_| review))
            .filter(|total| total.amount_minor() > 0);
        if let Some(total) = total
            && amount_text_signal.peek().is_empty()
        {
            amount_text_signal.set(amount_text(total, NumberFormat::current()));
            amount_error.set(None);
        }
        if items.peek().is_empty() {
            items.set(drafts_from_parsed(&parsed));
        }
        let language = detect_language(parsed.rows.iter().map(|row| row.text.as_str()));
        source_language.set(Some(language.map(str::to_string)));
    };

    let add_item = move |_| {
        let draft = ItemDraft::new(LineItem {
            quantity: Decimal::ONE,
            edited_by_user: true,
            ..LineItem::default()
        });
        let key = draft.key;
        items.write().push(draft);
        item_error.set(None);
        sheet.set(Some(Sheet::Item(key)));
    };

    let mut change_item = move |(key, line): (u64, LineItem)| {
        if let Some(draft) = items.write().iter_mut().find(|d| d.key == key) {
            draft.item = line;
        }
        participants_error.set(None);
    };

    let mut item_action = move |(key, action): (u64, ItemAction)| {
        let outcome = items::apply(&mut items.write(), key, action);
        match outcome {
            Ok(stay) => {
                item_error.set(None);
                if !stay {
                    sheet.set(None);
                }
            }
            Err(error) => item_error.set(Some(item_error_text(&error))),
        }
        participants_error.set(None);
    };

    rsx! {
        div { class: "mx-4 flex flex-col gap-5 pt-4 pb-8 safe-area-x",
            div { class: "flex flex-col gap-2",
                AmountInput {
                    id: "expense-amount",
                    label: t!("expense.amount").to_string(),
                    currency_label: t!("expense.currency").to_string(),
                    value: amount_text_signal(),
                    currency: cur,
                    autofocus: !editing,
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
            match receipt() {
                Some(attached) => rsx! {
                    div { class: "flex flex-col gap-2",
                        ReceiptCard { key: "{attached.id}", receipt: attached.clone() }
                        // Replacing the receipt of a saved expense is RCP-08.
                        if !editing {
                            ReceiptRecognition {
                                key: "{attached.id}",
                                receipt_id: attached.id.clone(),
                                currency,
                                on_read,
                            }
                            button {
                                class: "flex min-h-11 items-center gap-2 self-start rounded-full px-3 text-sm font-medium text-floral-white-300 active:bg-jet-black-800 transition-colors",
                                r#type: "button",
                                onclick: move |_| receipt.set(None),
                                Icon { icon: LdX, class: "h-4 w-4" }
                                {t!("receipt.remove").to_string()}
                            }
                        }
                    }
                },
                None if !editing => rsx! {
                    section { class: "flex flex-col gap-2",
                        h2 { class: "px-1 text-sm font-medium text-floral-white-300", {t!("receipt.title").to_string()} }
                        div { class: "flex gap-3",
                            if can_take_photo {
                                Button {
                                    variant: ButtonVariant::Secondary,
                                    class: "flex-1",
                                    disabled: picking(),
                                    onclick: move |_| pick.call(ImageKind::Camera),
                                    Icon { icon: LdCamera, class: "h-5 w-5" }
                                    {t!("receipt.take_photo").to_string()}
                                }
                            }
                            Button {
                                variant: ButtonVariant::Secondary,
                                class: "flex-1",
                                disabled: picking(),
                                onclick: move |_| pick.call(ImageKind::Gallery),
                                Icon { icon: LdImage, class: "h-5 w-5" }
                                {t!("receipt.choose").to_string()}
                            }
                        }
                        if picking() {
                            p { class: "px-1 text-sm text-floral-white-400", role: "status", {t!("scan.working").to_string()} }
                        }
                        if let Some(error) = receipt_error() {
                            p { class: "px-1 text-sm text-watermelon-300", role: "alert", "{error}" }
                        }
                    }
                },
                None => rsx! {},
            }
            div { class: "flex flex-col gap-2",
                span { class: "text-sm font-medium text-floral-white-300", {t!("expense.group").to_string()} }
                // Moving an expense to another group is EXP-11, so editing
                // shows the group without letting it change.
                button {
                    class: "flex min-h-14 w-full items-center gap-3 rounded-2xl border border-jet-black-700 bg-jet-black-900 px-3 text-left transition-colors ease-apple",
                    class: if !editing { "active:bg-jet-black-800" },
                    r#type: "button",
                    disabled: editing,
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
                    if !editing {
                        Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
                    }
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
                    }
                    if current_group.is_some() {
                        button {
                            class: "flex min-h-11 items-center rounded-full px-3 text-sm font-medium text-cerulean-300 active:bg-jet-black-800 transition-colors",
                            r#type: "button",
                            onclick: move |_| {
                                if all_selected {
                                    split_draft.write().participants.clear();
                                } else {
                                    let everyone = people.read().iter().map(|m| m.person.id.clone()).collect();
                                    split_draft.write().participants = everyone;
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
                    div {
                        class: "flex flex-wrap gap-2",
                        role: "radiogroup",
                        aria_label: t!("expense.split_mode").to_string(),
                        for kind in SplitKind::ALL {
                            Chip {
                                key: "{kind:?}",
                                label: kind.label(),
                                selected: draft.kind == kind,
                                onclick: move |_| {
                                    split_draft.write().kind = kind;
                                    participants_error.set(None);
                                },
                            }
                        }
                    }
                    if by_items {
                        p { class: "px-1 text-sm text-floral-white-400", {t!("items.participants_hint").to_string()} }
                    }
                    if draft.kind == SplitKind::Equal || draft.kind == SplitKind::Items {
                        div { class: "overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900 py-1",
                            PersonPicker {
                                multiple: true,
                                options: members
                                    .iter()
                                    .map(|m| PersonOption {
                                        selected: selected.contains(&m.person.id),
                                        detail: shares
                                            .get(&m.person.id)
                                            .map(|share| format_money(Money::new(*share, cur), format)),
                                        person: m.person.clone(),
                                    })
                                    .collect::<Vec<_>>(),
                                on_toggle: move |id: PersonId| {
                                    split_draft.write().toggle(id);
                                    participants_error.set(None);
                                },
                            }
                        }
                    } else {
                        div { class: "overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                            for entry in members.iter().cloned() {
                                ShareRow {
                                    key: "{entry.person.id.as_str()}",
                                    selected: selected.contains(&entry.person.id),
                                    kind: draft.kind,
                                    text: draft.text(&entry.person.id, entry.default_weight, format),
                                    currency: cur,
                                    share: shares.get(&entry.person.id).map(|share| Money::new(*share, cur)),
                                    invalid: participants_error().is_some(),
                                    on_toggle: {
                                        let id = entry.person.id.clone();
                                        move |_| {
                                            split_draft.write().toggle(id.clone());
                                            participants_error.set(None);
                                        }
                                    },
                                    on_input: {
                                        let id = entry.person.id.clone();
                                        move |text| {
                                            split_draft.write().set_text(id.clone(), text);
                                            participants_error.set(None);
                                        }
                                    },
                                    person: entry.person,
                                }
                            }
                        }
                    }
                    if let Some((text, matches)) = hint {
                        p {
                            class: if matches { "px-1 text-sm tabular-nums text-floral-white-400" } else { "px-1 text-sm tabular-nums text-pale-oak-300" },
                            aria_live: "polite",
                            "{text}"
                        }
                    }
                } else {
                    p { class: "px-1 text-sm text-floral-white-400", {t!("expense.no_group_hint").to_string()} }
                }
                if let Some(error) = participants_error() {
                    p { class: "px-1 text-sm text-watermelon-300", role: "alert", "{error}" }
                }
            }
            if show_items {
                ReceiptItems {
                    items: item_list.clone(),
                    currency: cur,
                    total,
                    people: members.iter().map(|m| m.person.clone()).collect::<Vec<_>>(),
                    assignable: by_items,
                    on_open: move |key| {
                        item_error.set(None);
                        sheet.set(Some(Sheet::Item(key)));
                    },
                    on_add: add_item,
                    translation: translation_note(translation.read().as_ref(), &item_list),
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
            if editing {
                Button {
                    variant: ButtonVariant::Danger,
                    class: "w-full",
                    disabled: saving(),
                    onclick: delete,
                    Icon { icon: LdTrash2, class: "h-5 w-5" }
                    {t!("expense.delete").to_string()}
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
            Some(Sheet::Item(key)) => {
                let index = item_list.iter().position(|d| d.key == key);
                match index {
                    Some(index) => rsx! {
                        ItemSheet {
                            key: "{key}",
                            draft: item_list[index].clone(),
                            currency: cur,
                            members: members.clone(),
                            assignable: by_items,
                            has_previous: index > 0,
                            has_next: index + 1 < item_list.len(),
                            error: item_error(),
                            on_change: move |line| change_item((key, line)),
                            on_action: move |action| item_action((key, action)),
                            on_close: move |_| sheet.set(None),
                        }
                    },
                    None => rsx! {},
                }
            }
            None => rsx! {},
        }
    }
}

fn item_error_text(error: &LineItemError) -> String {
    match error {
        LineItemError::CannotSplit => t!("items.cannot_split").to_string(),
        other => other.to_string(),
    }
}

/// After saving or deleting: `back` to the screen the form was opened from
/// when it shows the expense (its detail, or the timeline of its group),
/// otherwise to the group's timeline (GRP-23) or Home.
fn leave(nav: Navigator, group: Option<&GroupId>, back: bool) {
    if back && nav.can_go_back() {
        nav.go_back();
        return;
    }
    match group {
        Some(id) => {
            nav.replace(Route::GroupTimeline {
                id: id.as_str().to_string(),
            });
        }
        None if nav.can_go_back() => nav.go_back(),
        None => {
            nav.replace(Route::Home {});
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

/// Everything the form needs; with `id` also the expense to edit
/// (`StorageError::NotFound` if it is gone). `preset` is the group a new
/// expense starts in, if it still exists; `receipt` the receipt it starts
/// with.
fn load(
    db: &Db,
    id: Option<&ExpenseId>,
    preset: Option<GroupId>,
    receipt: Option<&str>,
) -> Result<FormData, StorageError> {
    let me = db.me()?.ok_or(StorageError::NotFound)?;
    let profile = db.profile()?;
    let home_currency = profile
        .as_ref()
        .map_or_else(default_home_currency, |p| p.home_currency);
    let target_language = profile.map_or_else(
        || suggested_target_language(None).to_string(),
        |p| p.target_language,
    );
    let groups = db.groups()?;
    let existing = match id {
        Some(id) => Some(db.expense(id)?.ok_or(StorageError::NotFound)?),
        None => None,
    };
    let opened_from = preset.filter(|id| groups.iter().any(|g| &g.id == id));
    let (group, currency) = match &existing {
        Some(expense) => (expense.group_id.clone(), Some(expense.total.currency())),
        None => {
            let group = match &opened_from {
                Some(id) => Some(id.clone()),
                None => match db.active_group()? {
                    Some(active) => Some(active.id),
                    None => preselected_group(db.setting(LAST_EXPENSE_GROUP)?.as_deref(), &groups),
                },
            };
            (group, db.currency_setting(LAST_EXPENSE_CURRENCY)?)
        }
    };
    let methods = db
        .payment_methods()?
        .into_iter()
        .filter(|m| !m.archived)
        .collect();
    let receipt = match existing
        .as_ref()
        .map_or(receipt, |e| e.receipt_id.as_deref())
    {
        Some(receipt) => db.receipt(receipt)?,
        None => None,
    };
    let receipt_language = match &receipt {
        Some(receipt) => db.receipt_language(&receipt.id)?,
        None => None,
    };
    Ok(FormData {
        receipt,
        me,
        home_currency,
        groups,
        categories: db.categories()?,
        methods,
        group,
        currency,
        existing,
        opened_from,
        review: false,
        target_language,
        receipt_language,
    })
}

/// The line above the receipt lines saying how the translation went.
fn translation_note(
    state: Option<&TranslationState>,
    items: &[ItemDraft],
) -> Option<TranslationNote> {
    let note = |text: String, warning: bool| Some(TranslationNote { text, warning });
    match state? {
        TranslationState::Running => note(t!("items.translating").to_string(), false),
        TranslationState::Failed(message)
        | TranslationState::Finished {
            machine: MachineTranslation::Failed(message),
            ..
        } => note(
            t!("items.translation_failed", message = message).to_string(),
            true,
        ),
        TranslationState::Finished {
            source: Some(source),
            target,
            machine,
        } if source != target => {
            let (from, to) = (language_name(source), language_name(target));
            match machine {
                MachineTranslation::Unavailable => note(
                    t!("items.translation_unavailable", from = from, to = to).to_string(),
                    true,
                ),
                _ if items.iter().any(|d| d.item.translated_text.is_some()) => {
                    note(t!("items.translated_from", from = from).to_string(), false)
                }
                _ => None,
            }
        }
        TranslationState::Finished { .. } => None,
    }
}

/// Without an active group: the group of the last expense if it still
/// exists (`""` = it had none), otherwise the newest group.
fn preselected_group(last: Option<&str>, groups: &[Group]) -> Option<GroupId> {
    match last {
        Some("") => None,
        Some(id) if groups.iter().any(|g| g.id.as_str() == id) => Some(GroupId::new(id)),
        _ => groups.first().map(|g| g.id.clone()),
    }
}

/// Who can pay and share: the group's members, or only "Ich" for a
/// personal expense (user decision in AP-11).
fn people_of(
    db: &Db,
    group: Option<&GroupId>,
    me: &Person,
) -> Result<Vec<GroupMember>, StorageError> {
    match group {
        Some(id) => db.group_members(id),
        None => Ok(vec![member(me.clone())]),
    }
}

/// A person outside any group, weighing 1.
fn member(person: Person) -> GroupMember {
    GroupMember {
        person,
        default_weight: Decimal::ONE,
    }
}

/// `2026-10-03T19:30:00+09:00` → (`2026-10-03`, `19:30`), what the date
/// and time fields edit.
fn split_occurred_at(occurred_at: &str) -> (String, String) {
    (
        occurred_at.get(0..10).unwrap_or_default().to_string(),
        occurred_at.get(11..16).unwrap_or_default().to_string(),
    )
}

/// The payers of a saved expense, as far as they are still members.
fn saved_payers(expense: &Expense, people: &[GroupMember]) -> Vec<PayerDraft> {
    let format = NumberFormat::current();
    expense
        .payments
        .iter()
        .filter_map(|payment| {
            let person = people.iter().find(|m| m.person.id == payment.person_id)?;
            Some(PayerDraft {
                person: person.person.clone(),
                method: payment.payment_method_id.clone(),
                amount_text: amount_text(payment.amount, format),
            })
        })
        .collect()
}

fn base_of(groups: &[Group], group: Option<&GroupId>, home: Currency) -> Currency {
    group
        .and_then(|id| groups.iter().find(|g| &g.id == id))
        .map_or(home, |g| g.base_currency)
}

/// "Ich" pays by default, or the first person if "Ich" is not there.
fn default_payer(people: &[GroupMember], me: &Person, methods: &[PaymentMethod]) -> PayerDraft {
    let person = people
        .iter()
        .map(|m| &m.person)
        .find(|p| p.id == me.id)
        .or_else(|| people.first().map(|m| &m.person))
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
            target_language: None,
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
    fn active_group_wins_over_the_last_one() {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&crate::storage::Profile {
            name: "Ich".into(),
            home_currency: Currency::from_code("EUR").unwrap(),
            target_language: "de".into(),
        })
        .unwrap();
        let new = |name: &str| {
            db.create_group(crate::storage::NewGroup {
                name: name.into(),
                icon: "plane".into(),
                color: "cerulean".into(),
                base_currency: Currency::from_code("EUR").unwrap(),
                start_date: None,
                end_date: None,
                target_language: None,
            })
            .unwrap()
        };
        let trip = new("Japan");
        let flat = new("WG");
        db.set_setting(LAST_EXPENSE_GROUP, flat.id.as_str())
            .unwrap();
        assert_eq!(
            load(&db, None, None, None).unwrap().group,
            Some(flat.id.clone())
        );

        db.set_active_group(Some(&trip.id)).unwrap();
        assert_eq!(
            load(&db, None, None, None).unwrap().group,
            Some(trip.id.clone())
        );
        // The timeline's group still comes first (GRP-23).
        assert_eq!(
            load(&db, None, Some(flat.id.clone()), None).unwrap().group,
            Some(flat.id.clone())
        );

        // A deleted active group no longer counts.
        db.delete_group(&trip.id).unwrap();
        assert_eq!(load(&db, None, None, None).unwrap().group, Some(flat.id));
    }

    #[test]
    fn occurred_at_splits_into_date_and_time() {
        assert_eq!(
            split_occurred_at("2026-10-03T19:30:00+09:00"),
            ("2026-10-03".to_string(), "19:30".to_string())
        );
        assert_eq!(split_occurred_at(""), (String::new(), String::new()));
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
