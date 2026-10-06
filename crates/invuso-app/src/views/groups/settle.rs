use std::collections::BTreeMap;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdArrowRight, LdChevronRight, LdCircleAlert, LdHandCoins, LdShare2, LdTrash2, LdX,
    },
};
use invuso_core::domain::{
    Group, GroupId, Money, PaymentMethod, PaymentMethodId, Person, PersonId, Settlement,
    SettlementError, local_date,
};
use invuso_core::split::{GroupSummary, Transfer};

use super::form::GroupNotFound;
use crate::clock::{local_now, occurred_at};
use crate::components::{
    Avatar, AvatarSize, BottomSheet, Button, ButtonVariant, CardSection, CompactAmountInput,
    DateTimeField, EmptyState, ErrorBanner, MethodChoice, MoneyText, PaymentIconGlyph,
    PaymentMethodIcon, PersonOption, PersonPicker, TopBar,
};
use crate::format::{NumberFormat, amount_text, format_money, parse_amount};
use crate::platform::{TextShare, text_share};
use crate::preferences::display_date;
use crate::services::settlements::settlement_text;
use crate::services::summary::group_summary;
use crate::state::{DataRevision, ToastAction, Toaster};
use crate::storage::{Db, NewSettlement, StorageError};

/// What the settle screen shows of one group.
#[derive(Debug, Clone, PartialEq)]
struct SettleData {
    group: Group,
    summary: GroupSummary,
    settlements: Vec<Settlement>,
    /// Everyone named in the debts and settlements, also former members.
    people: BTreeMap<PersonId, Person>,
    choices: SettleChoices,
    /// All methods by id, archived ones too, to name old settlements.
    method_names: BTreeMap<PaymentMethodId, String>,
}

/// Who and what the settlement sheet offers.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SettleChoices {
    /// Current members of the group.
    pub members: Vec<Person>,
    /// Payment methods that are not archived.
    pub methods: Vec<PaymentMethod>,
}

impl SettleChoices {
    pub(super) fn load(db: &Db, group: &GroupId) -> Result<Self, StorageError> {
        Ok(Self {
            members: db
                .group_members(group)?
                .into_iter()
                .map(|member| member.person)
                .collect(),
            methods: db
                .payment_methods()?
                .into_iter()
                .filter(|m| !m.archived)
                .collect(),
        })
    }
}

/// A payment opened in the settlement sheet: prefilled from a debt, or
/// empty for one entered freely.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SettleDraft {
    pub from: Option<PersonId>,
    pub to: Option<PersonId>,
    /// The open debt, prefilled and named as the full amount.
    pub open: Option<Money>,
}

impl SettleDraft {
    pub(super) fn from_debt(debt: &Transfer, group: &Group) -> Self {
        Self {
            from: Some(debt.from.clone()),
            to: Some(debt.to.clone()),
            open: Some(Money::new(debt.amount_minor, group.base_currency)),
        }
    }

    fn empty() -> Self {
        Self {
            from: None,
            to: None,
            open: None,
        }
    }
}

/// `/groups/:id/settle`: open debts, simplified or pairwise (SPL-04,
/// SPL-07), marking them as paid in full or in part and recording other
/// settlements (SPL-06), the recorded settlements, and sharing the
/// settlement as text (SPL-09). `record` opens the sheet for a free
/// settlement right away (plus button, idee.md 7.1).
#[component]
pub fn GroupSettle(id: String, record: bool) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let mut toaster = use_context::<Toaster>();
    let mut pairwise = use_signal(|| false);
    let mut sheet = use_signal(move || record.then(SettleDraft::empty));
    let can_share = use_hook(|| text_share().supported());

    let group_id = use_memo(use_reactive!(|id| GroupId::new(id)));
    let data = use_memo(move || {
        revision.track();
        load(&db, &group_id()).map_err(|e| e.to_string())
    });

    rsx! {
        TopBar { title: t!("page.group_settle").to_string(), show_back: true }
        match &*data.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("summary.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(None) => rsx! { GroupNotFound {} },
            Ok(Some(data)) => {
                let group = data.group.clone();
                let debts = if pairwise() { data.summary.pairwise.clone() } else { data.summary.transfers.clone() };
                let base = group.base_currency;
                let share = {
                    let text = settlement_text(
                        &group.name,
                        data.summary.total,
                        &debts,
                        &data.people,
                        NumberFormat::current(),
                    );
                    move |_| {
                        if let Err(e) = text_share().share(&text) {
                            toaster.show(format!("{} {e}", t!("settle.share_error")), None);
                        }
                    }
                };
                rsx! {
                    div { class: "mx-4 flex flex-col gap-5 pt-6 safe-area-x",
                        ModeSwitch { pairwise: pairwise(), on_change: move |value| pairwise.set(value) }
                        CardSection { title: t!("summary.who_owes_whom").to_string(),
                            if debts.is_empty() {
                                p { class: "px-4 py-4 text-base text-floral-white-300", {t!("summary.all_settled").to_string()} }
                            }
                            for (index, debt) in debts.iter().enumerate() {
                                DebtRow {
                                    key: "{index}",
                                    from: data.people.get(&debt.from).cloned(),
                                    to: data.people.get(&debt.to).cloned(),
                                    amount: Money::new(debt.amount_minor, base),
                                    onclick: {
                                        let draft = SettleDraft::from_debt(debt, &group);
                                        move |_| sheet.set(Some(draft.clone()))
                                    },
                                }
                            }
                        }
                        if !debts.is_empty() {
                            p { class: "-mt-3 px-1 text-sm text-floral-white-400", {t!("settle.tap_hint").to_string()} }
                        }
                        div { class: "flex flex-col gap-2",
                            Button {
                                class: "w-full",
                                onclick: move |_| sheet.set(Some(SettleDraft::empty())),
                                Icon { icon: LdHandCoins, class: "h-5 w-5" }
                                {t!("settle.record").to_string()}
                            }
                            if can_share {
                                Button {
                                    variant: ButtonVariant::Secondary,
                                    class: "w-full",
                                    onclick: share,
                                    Icon { icon: LdShare2, class: "h-5 w-5" }
                                    {t!("settle.share").to_string()}
                                }
                            }
                        }
                        CardSection { title: t!("settle.recorded").to_string(),
                            if data.settlements.is_empty() {
                                p { class: "px-4 py-4 text-base text-floral-white-300", {t!("settle.none_recorded").to_string()} }
                            }
                            for settlement in data.settlements.iter().cloned() {
                                SettlementRow {
                                    key: "{settlement.id.as_str()}",
                                    from: data.people.get(&settlement.from).cloned(),
                                    to: data.people.get(&settlement.to).cloned(),
                                    method: settlement
                                        .payment_method_id
                                        .as_ref()
                                        .and_then(|id| data.method_names.get(id))
                                        .cloned(),
                                    settlement,
                                }
                            }
                        }
                    }
                    if let Some(draft) = sheet() {
                        SettleSheet {
                            group,
                            choices: data.choices.clone(),
                            draft,
                            on_close: move |_| sheet.set(None),
                        }
                    }
                }
            }
        }
    }
}

fn load(db: &Db, id: &GroupId) -> Result<Option<SettleData>, StorageError> {
    let Some(group) = db.group(id)? else {
        return Ok(None);
    };
    let summary = group_summary(db, &group)?;
    let settlements = db.group_settlements(id)?;
    let named = summary
        .people
        .keys()
        .chain(settlements.iter().flat_map(|s| [&s.from, &s.to]));
    let people = db.people_any(named)?;
    let method_names = db
        .payment_methods()?
        .into_iter()
        .map(|method| (method.id, method.name))
        .collect();
    Ok(Some(SettleData {
        choices: SettleChoices::load(db, id)?,
        group,
        summary,
        settlements,
        people,
        method_names,
    }))
}

/// "Vereinfacht" or "Einzeln" (SPL-07), with a line on what it means.
#[component]
fn ModeSwitch(pairwise: bool, on_change: EventHandler<bool>) -> Element {
    let option = |value: bool, label: String| {
        let selected = value == pairwise;
        rsx! {
            button {
                class: "flex min-h-11 flex-1 items-center justify-center rounded-xl px-3 text-sm font-medium transition-colors ease-apple",
                class: if selected { "bg-cerulean-700 text-floral-white-50" } else { "text-floral-white-300 active:bg-jet-black-800" },
                r#type: "button",
                role: "radio",
                aria_checked: if selected { "true" } else { "false" },
                onclick: move |_| on_change.call(value),
                "{label}"
            }
        }
    };
    let hint = if pairwise {
        t!("settle.pairwise_hint")
    } else {
        t!("settle.simplified_hint")
    };
    rsx! {
        div { class: "flex flex-col gap-2",
            div {
                class: "flex gap-1 rounded-2xl border border-jet-black-800 bg-jet-black-900 p-1",
                role: "radiogroup",
                aria_label: t!("settle.mode").to_string(),
                {option(false, t!("settle.simplified").to_string())}
                {option(true, t!("settle.pairwise").to_string())}
            }
            p { class: "px-1 text-sm text-floral-white-400", "{hint}" }
        }
    }
}

/// One open debt; tapping it opens the sheet to mark it as paid (idee.md
/// 7.4).
#[component]
pub(super) fn DebtRow(
    from: Option<Person>,
    to: Option<Person>,
    amount: Money,
    onclick: EventHandler<()>,
) -> Element {
    let (from, from_color) = person_label(from.as_ref());
    let (to, to_color) = person_label(to.as_ref());
    let label = t!(
        "settle.mark_paid_label",
        from = from.clone(),
        to = to.clone(),
        amount = format_money(amount, NumberFormat::current())
    )
    .to_string();

    rsx! {
        button {
            class: "flex min-h-16 w-full items-center gap-2 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            aria_label: "{label}",
            onclick: move |_| onclick.call(()),
            Avatar { name: from.clone(), color: from_color, size: AvatarSize::Sm }
            span { class: "min-w-0 flex-1 truncate text-base text-floral-white-50", "{from}" }
            Icon { icon: LdArrowRight, class: "h-4 w-4 shrink-0 text-floral-white-500" }
            Avatar { name: to.clone(), color: to_color, size: AvatarSize::Sm }
            span { class: "min-w-0 flex-1 truncate text-base text-floral-white-50", "{to}" }
            MoneyText { amount, class: "shrink-0 text-base font-semibold text-floral-white-100" }
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
        }
    }
}

/// A recorded settlement with date and method; deleting it can be undone.
#[component]
fn SettlementRow(
    settlement: Settlement,
    from: Option<Person>,
    to: Option<Person>,
    method: Option<String>,
) -> Element {
    let db = use_context::<Db>();
    let mut revision = use_context::<DataRevision>();
    let mut toaster = use_context::<Toaster>();
    let (from, from_color) = person_label(from.as_ref());
    let (to, _) = person_label(to.as_ref());
    let mut detail = vec![display_date(local_date(&settlement.occurred_at))];
    detail.extend(method);
    let detail = detail.join(" · ");
    let title = t!("summary.transfer", from = from.clone(), to = to.clone()).to_string();

    let delete = move |_| {
        let id = settlement.id.clone();
        match db.delete_settlement(&id) {
            Ok(()) => {
                revision.bump();
                let db = db.clone();
                let undo = move || {
                    let (mut revision, mut toaster) = (revision, toaster);
                    match db.restore_settlement(&id) {
                        Ok(()) => revision.bump(),
                        Err(e) => toaster.show(format!("{} {e}", t!("settle.restore_error")), None),
                    }
                };
                toaster.show(
                    t!("settle.deleted").to_string(),
                    Some(ToastAction {
                        label: t!("common.undo").to_string(),
                        run: Rc::new(undo),
                    }),
                );
            }
            Err(e) => toaster.show(format!("{} {e}", t!("settle.delete_error")), None),
        }
    };

    rsx! {
        div { class: "flex min-h-16 items-center gap-3 border-b border-jet-black-800 py-2 pl-4 pr-1 last:border-b-0",
            Avatar { name: from.clone(), color: from_color, size: AvatarSize::Sm }
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "truncate text-base text-floral-white-50", "{title}" }
                span { class: "truncate text-sm text-floral-white-400", "{detail}" }
            }
            MoneyText { amount: settlement.amount, class: "shrink-0 text-base font-semibold text-floral-white-100" }
            button {
                class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full text-floral-white-400 active:bg-jet-black-800 transition-colors",
                r#type: "button",
                aria_label: t!("settle.delete_label", title = title.clone()).to_string(),
                onclick: delete,
                Icon { icon: LdTrash2, class: "h-5 w-5" }
            }
        }
    }
}

/// Which list the settlement sheet shows instead of its form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choosing {
    From,
    To,
    Method,
}

/// Records a settlement (SPL-06): who paid whom, how much – the full open
/// debt or a part of it –, with which method and when.
#[component]
pub(super) fn SettleSheet(
    group: Group,
    choices: SettleChoices,
    draft: SettleDraft,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let mut revision = use_context::<DataRevision>();
    let mut toaster = use_context::<Toaster>();
    let format = NumberFormat::current();
    let base = group.base_currency;

    let mut from = use_signal(|| draft.from.clone());
    let mut to = use_signal(|| draft.to.clone());
    let mut amount = use_signal(|| {
        draft
            .open
            .map(|open| amount_text(open, format))
            .unwrap_or_default()
    });
    let mut method = use_signal({
        let choices = choices.clone();
        let from = draft.from.clone();
        move || from.and_then(|from| default_method(&from, &choices.methods))
    });
    let mut date = use_signal(|| local_now().0);
    let mut time = use_signal(|| local_now().1);
    let mut choosing = use_signal(|| None::<Choosing>);
    let mut error = use_signal(|| None::<String>);

    // People in the draft stay choosable even if they have left the group.
    let mut people = choices.members.clone();
    let missing: Vec<PersonId> = [draft.from.clone(), draft.to.clone()]
        .into_iter()
        .flatten()
        .filter(|id| !people.iter().any(|p| &p.id == id))
        .collect();
    if !missing.is_empty()
        && let Ok(found) = db.people_any(&missing)
    {
        people.extend(found.into_values());
    }
    let find = |id: &Option<PersonId>| {
        id.as_ref()
            .and_then(|id| people.iter().find(|p| &p.id == id))
            .cloned()
    };
    let from_person = find(&from());
    let to_person = find(&to());
    let methods = from_person
        .as_ref()
        .map(|p| methods_for(&p.id, &choices.methods))
        .unwrap_or_default();
    let chosen_method =
        method().and_then(|id| choices.methods.iter().find(|m| m.id == id).cloned());

    let title = if draft.open.is_some() {
        t!("settle.mark_paid")
    } else {
        t!("settle.record")
    };

    let save = {
        let group_id = group.id.clone();
        move |_| {
            let (Some(payer), Some(payee)) = (from(), to()) else {
                error.set(Some(t!("settle.choose_people").to_string()));
                return;
            };
            let Some(money) =
                parse_amount(&amount(), base, format).filter(|m| m.amount_minor() > 0)
            else {
                error.set(Some(t!("settle.enter_amount").to_string()));
                return;
            };
            let Some(at) = occurred_at(&date(), &time()) else {
                error.set(Some(t!("expense.date_time_invalid").to_string()));
                return;
            };
            let new = NewSettlement {
                group_id: group_id.clone(),
                from: payer,
                to: payee,
                amount: money,
                payment_method_id: method(),
                occurred_at: at,
                note: None,
            };
            match db.create_settlement(new) {
                Ok(saved) => {
                    revision.bump();
                    on_close.call(());
                    let db = db.clone();
                    let undo = move || {
                        let (mut revision, mut toaster) = (revision, toaster);
                        match db.delete_settlement(&saved.id) {
                            Ok(()) => revision.bump(),
                            Err(e) => {
                                toaster.show(format!("{} {e}", t!("settle.delete_error")), None)
                            }
                        }
                    };
                    toaster.show(
                        t!("settle.saved").to_string(),
                        Some(ToastAction {
                            label: t!("common.undo").to_string(),
                            run: Rc::new(undo),
                        }),
                    );
                }
                Err(StorageError::Settlement(SettlementError::SamePerson)) => {
                    error.set(Some(t!("settle.same_person").to_string()));
                }
                Err(e) => error.set(Some(format!("{} {e}", t!("settle.save_error")))),
            }
        }
    };

    let body = match choosing() {
        Some(side @ (Choosing::From | Choosing::To)) => {
            let current = if side == Choosing::From { from() } else { to() };
            rsx! {
                div { class: "flex max-h-[70vh] flex-col overflow-y-auto overscroll-contain px-3 pt-2",
                    PersonPicker {
                        options: people
                            .iter()
                            .cloned()
                            .map(|person| PersonOption {
                                selected: current.as_ref() == Some(&person.id),
                                person,
                                detail: None,
                            })
                            .collect::<Vec<_>>(),
                        on_toggle: {
                            let all_methods = choices.methods.clone();
                            move |id: PersonId| {
                                if side == Choosing::From {
                                    method.set(default_method(&id, &all_methods));
                                    from.set(Some(id));
                                } else {
                                    to.set(Some(id));
                                }
                                error.set(None);
                                choosing.set(None);
                            }
                        },
                    }
                }
            }
        }
        Some(Choosing::Method) => rsx! {
            div { class: "flex max-h-[70vh] flex-col gap-1 overflow-y-auto overscroll-contain px-3 pt-2", role: "listbox",
                MethodChoice {
                    label: t!("expense.no_method").to_string(),
                    selected: method().is_none(),
                    onclick: move |_| {
                        method.set(None);
                        choosing.set(None);
                    },
                    span { class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-jet-black-800 text-floral-white-400",
                        Icon { icon: LdX, class: "h-5 w-5" }
                    }
                }
                for entry in methods.iter().cloned() {
                    MethodChoice {
                        key: "{entry.id.as_str()}",
                        label: entry.name.clone(),
                        selected: method().as_ref() == Some(&entry.id),
                        onclick: move |_| {
                            method.set(Some(entry.id.clone()));
                            choosing.set(None);
                        },
                        PaymentMethodIcon { icon: entry.icon.clone(), color: entry.color.clone() }
                    }
                }
            }
        },
        None => rsx! {
            div { class: "flex max-h-[75vh] flex-col gap-4 overflow-y-auto overscroll-contain px-5 pt-3",
                div { class: "flex items-center gap-2",
                    PersonButton {
                        label: t!("settle.from").to_string(),
                        person: from_person.clone(),
                        onclick: move |_| choosing.set(Some(Choosing::From)),
                    }
                    Icon { icon: LdArrowRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
                    PersonButton {
                        label: t!("settle.to").to_string(),
                        person: to_person.clone(),
                        onclick: move |_| choosing.set(Some(Choosing::To)),
                    }
                }
                div { class: "flex flex-col gap-1",
                    div { class: "flex min-h-12 items-center gap-3",
                        span { class: "flex-1 text-base text-floral-white-200", {t!("settle.amount").to_string()} }
                        CompactAmountInput {
                            id: "settle-amount",
                            label: t!("settle.amount").to_string(),
                            value: amount(),
                            currency: base,
                            invalid: false,
                            oninput: move |text| {
                                amount.set(text);
                                error.set(None);
                            },
                        }
                    }
                    if let Some(open) = draft.open {
                        p { class: "text-sm text-floral-white-400",
                            {t!("settle.open_hint", amount = format_money(open, format)).to_string()}
                        }
                    }
                }
                div { class: "flex min-h-12 items-center gap-3",
                    span { class: "flex-1 text-base text-floral-white-200", {t!("expense.method").to_string()} }
                    button {
                        class: "flex min-h-11 items-center gap-2 rounded-full bg-jet-black-800 px-3 text-sm text-floral-white-200 active:bg-jet-black-700 transition-colors ease-apple disabled:opacity-50",
                        r#type: "button",
                        disabled: from_person.is_none(),
                        onclick: move |_| choosing.set(Some(Choosing::Method)),
                        if let Some(entry) = &chosen_method {
                            PaymentIconGlyph { icon: entry.icon.clone(), class: "h-4 w-4".to_string() }
                            span { "{entry.name}" }
                        } else {
                            span { class: "text-floral-white-400", {t!("expense.choose_method").to_string()} }
                        }
                        Icon { icon: LdChevronRight, class: "h-4 w-4 text-floral-white-500" }
                    }
                }
                DateTimeField {
                    id: "settle-time",
                    label: t!("expense.date_time").to_string(),
                    date: date(),
                    time: time(),
                    date_label: t!("expense.date").to_string(),
                    time_label: t!("expense.time").to_string(),
                    on_date: move |value| date.set(value),
                    on_time: move |value| time.set(value),
                }
                ErrorBanner { error: error() }
                Button { class: "w-full", onclick: save,
                    Icon { icon: LdHandCoins, class: "h-5 w-5" }
                    {t!("settle.save").to_string()}
                }
            }
        },
    };

    let sheet_title = match choosing() {
        Some(Choosing::From) => t!("settle.from").to_string(),
        Some(Choosing::To) => t!("settle.to").to_string(),
        Some(Choosing::Method) => t!("expense.method").to_string(),
        None => title.to_string(),
    };
    rsx! {
        BottomSheet {
            title: sheet_title,
            on_close: move |_| {
                // Inside a list, closing goes back to the form first.
                if choosing().is_some() {
                    choosing.set(None);
                } else {
                    on_close.call(());
                }
            },
            {body}
        }
    }
}

/// Payer or payee of the sheet; empty until chosen.
#[component]
fn PersonButton(label: String, person: Option<Person>, onclick: EventHandler<()>) -> Element {
    rsx! {
        button {
            class: "flex min-h-14 min-w-0 flex-1 items-center gap-2 rounded-2xl border border-jet-black-700 bg-jet-black-950 px-3 text-left active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            aria_label: "{label}",
            onclick: move |_| onclick.call(()),
            if let Some(person) = &person {
                Avatar { name: person.name.clone(), color: person.color.clone(), size: AvatarSize::Sm }
            }
            span { class: "flex min-w-0 flex-col",
                span { class: "text-xs text-floral-white-400", "{label}" }
                if let Some(person) = &person {
                    span { class: "truncate text-base text-floral-white-50", "{person.name}" }
                } else {
                    span { class: "truncate text-base text-floral-white-400", {t!("settle.choose").to_string()} }
                }
            }
        }
    }
}

/// Methods the payer can use: their own and those without owner.
fn methods_for(person: &PersonId, methods: &[PaymentMethod]) -> Vec<PaymentMethod> {
    methods
        .iter()
        .filter(|m| {
            m.owner_person_id
                .as_ref()
                .is_none_or(|owner| owner == person)
        })
        .cloned()
        .collect()
}

/// Preselected only when the payer has exactly one method.
fn default_method(person: &PersonId, methods: &[PaymentMethod]) -> Option<PaymentMethodId> {
    match methods_for(person, methods).as_slice() {
        [only] => Some(only.id.clone()),
        _ => None,
    }
}

/// Name and color of a person, or a stand-in if the person is unknown.
pub(super) fn person_label(person: Option<&Person>) -> (String, String) {
    match person {
        Some(person) => (person.name.clone(), person.color.clone()),
        None => (
            t!("expense_detail.unknown_person").to_string(),
            String::new(),
        ),
    }
}
