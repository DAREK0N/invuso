use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdArchive, LdArchiveRestore, LdChevronDown, LdCircleAlert, LdPencil, LdPlus, LdTrash2,
        LdWallet,
    },
};
use invuso_core::domain::{
    AccountTerms, Currency, PaymentMethod, PaymentMethodError, PaymentMethodKind, Person, PersonId,
    validate_last4,
};

use crate::Route;
use crate::components::{
    Avatar, AvatarSize, BottomSheet, Button, ButtonVariant, Chip, ColorPicker, CompactAmountInput,
    CompactNumberInput, ConfirmSheet, CurrencyButton, CurrencyPicker, EmptyState, ErrorBanner,
    IconPicker, ListItem, MenuRow, PaymentMethodIcon, TextField, TopBar,
};
use crate::format::{
    NumberFormat, amount_text, fit_amount_text, number_text, parse_amount, parse_number,
};
use crate::preferences::{default_payment_icon, payment_kind_name, suggested_person_color};
use crate::state::{DataRevision, ToastAction, Toaster};
use crate::storage::{Db, NewPaymentMethod, StorageError};

/// What the payment method form sheet is doing.
#[derive(Debug, Clone, PartialEq)]
enum Form {
    New,
    Edit(PaymentMethod),
}

/// Active methods of one owner, shown under the owner's name.
#[derive(Debug, Clone, PartialEq)]
struct OwnerSection {
    /// `None` for methods without an owner; the form never creates those,
    /// but the data model allows them (idee.md 4.1).
    owner: Option<Person>,
    methods: Vec<PaymentMethod>,
}

/// Everything the page lists.
#[derive(Debug, Clone, PartialEq)]
struct Overview {
    people: Vec<Person>,
    sections: Vec<OwnerSection>,
    archived: Vec<PaymentMethod>,
}

/// `/settings/payment-methods`: cards, cash, PayPal … grouped by owner,
/// archived ones folded away at the bottom (PAY-01, SET-06). Tap opens the
/// method's evaluation (PAY-05), long-press a menu (UI-09).
#[component]
pub fn SettingsPaymentMethods() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let mut form = use_signal(|| None::<Form>);
    let mut menu = use_signal(|| None::<PaymentMethod>);
    let mut confirm_delete = use_signal(|| None::<PaymentMethod>);
    let mut delete_error = use_signal(|| None::<String>);
    let mut show_archived = use_signal(|| false);

    let nav = use_navigator();
    let menu_db = db.clone();
    let overview = use_memo(move || {
        revision.track();
        let people = db.people().map_err(|e| e.to_string())?;
        let methods = db.payment_methods().map_err(|e| e.to_string())?;
        Ok::<_, String>(overview(people, methods))
    });

    rsx! {
        TopBar { title: t!("page.settings_payment_methods").to_string(), show_back: true }
        match &*overview.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("payment.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(overview) => rsx! {
                div { class: "mx-4 flex flex-col gap-4 pt-4 safe-area-x",
                    Button { class: "w-full", onclick: move |_| form.set(Some(Form::New)),
                        Icon { icon: LdPlus, class: "h-5 w-5" }
                        {t!("payment.add").to_string()}
                    }
                    if overview.sections.is_empty() {
                        EmptyState {
                            title: t!("payment.empty_title").to_string(),
                            text: t!("payment.empty_text").to_string(),
                            Icon { icon: LdWallet, class: "h-8 w-8" }
                        }
                    } else {
                        for section in overview.sections.iter().cloned() {
                            OwnerSectionView {
                                key: "{section.owner.as_ref().map(|o| o.id.as_str()).unwrap_or_default()}",
                                section,
                                on_open: move |method: PaymentMethod| {
                                    nav.push(Route::PaymentMethodDetail { id: method.id.as_str().to_string() });
                                },
                                on_long_press: move |method| menu.set(Some(method)),
                            }
                        }
                        p { class: "px-1 text-sm text-floral-white-500", {t!("payment.long_press_hint").to_string()} }
                    }
                    if !overview.archived.is_empty() {
                        ArchivedSection {
                            methods: overview.archived.clone(),
                            people: overview.people.clone(),
                            open: show_archived(),
                            on_toggle: move |_| show_archived.toggle(),
                            on_open: move |method: PaymentMethod| {
                                nav.push(Route::PaymentMethodDetail { id: method.id.as_str().to_string() });
                            },
                            on_long_press: move |method| menu.set(Some(method)),
                        }
                    }
                }
                if let Some(target) = form() {
                    PaymentMethodFormSheet {
                        method: match target {
                            Form::New => None,
                            Form::Edit(method) => Some(method),
                        },
                        people: overview.people.clone(),
                        on_saved: move |_| form.set(None),
                        on_close: move |_| form.set(None),
                    }
                }
            },
        }
        if let Some(method) = menu() {
            MethodMenu {
                method,
                on_edit: move |method| {
                    menu.set(None);
                    form.set(Some(Form::Edit(method)));
                },
                on_archive: {
                    let db = menu_db.clone();
                    move |method: PaymentMethod| {
                        menu.set(None);
                        set_archived_with_undo(&db, &method, !method.archived, revision, toaster);
                    }
                },
                on_delete: move |method| {
                    menu.set(None);
                    delete_error.set(None);
                    confirm_delete.set(Some(method));
                },
                on_close: move |_| menu.set(None),
            }
        }
        if let Some(method) = confirm_delete() {
            DeleteMethodSheet {
                method,
                error: delete_error(),
                on_deleted: move |_| confirm_delete.set(None),
                on_error: move |message| delete_error.set(Some(message)),
                on_close: move |_| confirm_delete.set(None),
            }
        }
    }
}

/// Splits the methods into active ones per owner (in the order of `people`,
/// "Ich" first) and archived ones.
fn overview(people: Vec<Person>, methods: Vec<PaymentMethod>) -> Overview {
    let (archived, active): (Vec<_>, Vec<_>) = methods.into_iter().partition(|m| m.archived);
    let mut sections: Vec<OwnerSection> = people
        .iter()
        .map(|person| OwnerSection {
            owner: Some(person.clone()),
            methods: active
                .iter()
                .filter(|m| m.owner_person_id.as_ref() == Some(&person.id))
                .cloned()
                .collect(),
        })
        .collect();
    sections.push(OwnerSection {
        owner: None,
        methods: active
            .iter()
            .filter(|m| m.owner_person_id.is_none())
            .cloned()
            .collect(),
    });
    sections.retain(|section| !section.methods.is_empty());
    Overview {
        people,
        sections,
        archived,
    }
}

/// Owner name with avatar above the owner's methods.
#[component]
fn OwnerSectionView(
    section: OwnerSection,
    on_open: EventHandler<PaymentMethod>,
    on_long_press: EventHandler<PaymentMethod>,
) -> Element {
    rsx! {
        section { class: "flex flex-col gap-2",
            h2 { class: "flex items-center gap-2 px-1 text-sm font-semibold text-floral-white-300",
                match &section.owner {
                    Some(owner) => rsx! {
                        Avatar { name: owner.name.clone(), color: owner.color.clone(), size: AvatarSize::Sm }
                        span { class: "truncate", "{owner.name}" }
                    },
                    None => rsx! { span { {t!("payment.no_owner").to_string()} } },
                }
            }
            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                for method in section.methods.iter().cloned() {
                    MethodRow {
                        key: "{method.id.as_str()}",
                        method,
                        owner_name: None,
                        on_open,
                        on_long_press,
                    }
                }
            }
        }
    }
}

/// Folded list of archived methods: hidden from pickers, still viewable
/// (PAY-01).
#[component]
fn ArchivedSection(
    methods: Vec<PaymentMethod>,
    people: Vec<Person>,
    open: bool,
    on_toggle: EventHandler<()>,
    on_open: EventHandler<PaymentMethod>,
    on_long_press: EventHandler<PaymentMethod>,
) -> Element {
    let count = methods.len();
    let owner_name = move |owner: &Option<PersonId>| {
        owner
            .as_ref()
            .and_then(|id| people.iter().find(|p| &p.id == id))
            .map(|p| p.name.clone())
    };

    rsx! {
        section { class: "flex flex-col gap-2",
            button {
                class: "flex min-h-11 items-center gap-2 px-1 text-left text-sm font-semibold text-floral-white-400 active:text-floral-white-200",
                r#type: "button",
                aria_expanded: if open { "true" } else { "false" },
                onclick: move |_| on_toggle.call(()),
                Icon { icon: LdArchive, class: "h-4 w-4" }
                span { class: "flex-1", {t!("payment.archived", count = count).to_string()} }
                Icon {
                    icon: LdChevronDown,
                    class: if open { "h-5 w-5 rotate-180 transition-transform" } else { "h-5 w-5 transition-transform" },
                }
            }
            if open {
                div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900 opacity-70",
                    for method in methods.iter().cloned() {
                        MethodRow {
                            key: "{method.id.as_str()}",
                            owner_name: owner_name(&method.owner_person_id),
                            method,
                            on_open,
                            on_long_press,
                        }
                    }
                }
            }
        }
    }
}

/// List row: icon, name, kind and last digits; `owner_name` is added where
/// the row is not under its owner's heading.
#[component]
fn MethodRow(
    method: PaymentMethod,
    owner_name: Option<String>,
    on_open: EventHandler<PaymentMethod>,
    on_long_press: EventHandler<PaymentMethod>,
) -> Element {
    let subtitle = subtitle(&method, owner_name.as_deref());
    let open_target = method.clone();
    let menu_target = method.clone();

    rsx! {
        ListItem {
            title: method.name.clone(),
            subtitle,
            onclick: move |_| on_open.call(open_target.clone()),
            on_long_press: move |_| on_long_press.call(menu_target.clone()),
            PaymentMethodIcon { icon: method.icon.clone(), color: method.color.clone() }
        }
    }
}

/// "Kreditkarte · •••• 4242 · USD · Ben": only the last digits are ever
/// shown.
pub(super) fn subtitle(method: &PaymentMethod, owner_name: Option<&str>) -> String {
    let mut parts = vec![payment_kind_name(method.kind)];
    if let Some(last4) = &method.last4 {
        parts.push(format!("•••• {last4}"));
    }
    if let Some(currency) = method.account.currency {
        parts.push(currency.code().to_string());
    }
    if let Some(owner) = owner_name {
        parts.push(owner.to_string());
    }
    parts.join(" · ")
}

/// Long-press menu of a method: edit, archive or restore, delete.
#[component]
fn MethodMenu(
    method: PaymentMethod,
    on_edit: EventHandler<PaymentMethod>,
    on_archive: EventHandler<PaymentMethod>,
    on_delete: EventHandler<PaymentMethod>,
    on_close: EventHandler<()>,
) -> Element {
    let edit_target = method.clone();
    let archive_target = method.clone();
    let delete_target = method.clone();

    rsx! {
        BottomSheet { title: method.name.clone(), on_close,
            div { class: "flex flex-col gap-1 px-3 pt-2",
                MenuRow {
                    label: t!("common.edit").to_string(),
                    onclick: move |_| on_edit.call(edit_target.clone()),
                    Icon { icon: LdPencil, class: "h-5 w-5" }
                }
                if method.archived {
                    MenuRow {
                        label: t!("payment.unarchive").to_string(),
                        onclick: move |_| on_archive.call(archive_target.clone()),
                        Icon { icon: LdArchiveRestore, class: "h-5 w-5" }
                    }
                } else {
                    MenuRow {
                        label: t!("payment.archive").to_string(),
                        onclick: move |_| on_archive.call(archive_target.clone()),
                        Icon { icon: LdArchive, class: "h-5 w-5" }
                    }
                }
                MenuRow {
                    label: t!("common.delete").to_string(),
                    danger: true,
                    onclick: move |_| on_delete.call(delete_target.clone()),
                    Icon { icon: LdTrash2, class: "h-5 w-5" }
                }
            }
        }
    }
}

/// Archives or restores a method and offers to undo it from a toast.
fn set_archived_with_undo(
    db: &Db,
    method: &PaymentMethod,
    archived: bool,
    mut revision: DataRevision,
    mut toaster: Toaster,
) {
    if let Err(e) = db.set_payment_method_archived(&method.id, archived) {
        toaster.show(format!("{} {e}", t!("profile.save_error")), None);
        return;
    }
    revision.bump();

    let db = db.clone();
    let id = method.id.clone();
    let undo = move || {
        let (mut revision, mut toaster) = (revision, toaster);
        match db.set_payment_method_archived(&id, !archived) {
            Ok(()) => revision.bump(),
            Err(e) => toaster.show(format!("{} {e}", t!("profile.save_error")), None),
        }
    };
    let message = if archived {
        t!("payment.archived_toast", name = method.name)
    } else {
        t!("payment.unarchived_toast", name = method.name)
    };
    toaster.show(
        message.to_string(),
        Some(ToastAction {
            label: t!("common.undo").to_string(),
            run: Rc::new(undo),
        }),
    );
}

/// Bottom sheet to create (`method: None`) or edit a payment method: name,
/// kind, owner, last digits for cards, color and icon (PAY-01).
#[component]
pub(super) fn PaymentMethodFormSheet(
    method: Option<PaymentMethod>,
    people: Vec<Person>,
    on_saved: EventHandler<PaymentMethod>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let mut revision = use_context::<DataRevision>();
    let is_new = method.is_none();

    let initial = method.clone();
    let mut name = use_signal(|| initial.as_ref().map(|m| m.name.clone()).unwrap_or_default());
    let initial = method.clone();
    let mut kind = use_signal(|| initial.map_or(PaymentMethodKind::Cash, |m| m.kind));
    let initial = method.clone();
    let default_owner = people.iter().find(|p| p.is_me).map(|p| p.id.clone());
    let mut owner = use_signal(|| initial.map_or(default_owner, |m| m.owner_person_id));
    let initial = method.clone();
    let mut last4 = use_signal(|| initial.and_then(|m| m.last4).unwrap_or_default());
    let initial = method.clone();
    let suggest_db = db.clone();
    let mut color = use_signal(move || match &initial {
        Some(method) => method.color.clone(),
        None => {
            let methods = suggest_db.payment_methods().unwrap_or_default();
            suggested_person_color(methods.iter().map(|m| m.color.as_str())).to_string()
        }
    });
    let initial = method.clone();
    let mut icon = use_signal(|| {
        initial.map_or(
            default_payment_icon(PaymentMethodKind::Cash).to_string(),
            |m| m.icon,
        )
    });
    let format = NumberFormat::current();
    let home = use_hook({
        let db = db.clone();
        move || db.expense_base_currency(None).ok()
    });
    let initial = method.clone();
    let mut account_currency = use_signal(|| initial.and_then(|m| m.account.currency));
    let initial = method.clone();
    let mut fee_percent = use_signal(|| {
        initial
            .and_then(|m| m.account.foreign_fee_percent)
            .map(|p| number_text(p, format))
            .unwrap_or_default()
    });
    let initial = method.clone();
    let mut fixed_fee = use_signal(|| {
        initial
            .and_then(|m| m.account.fixed_fee)
            .map(|f| amount_text(f, format))
            .unwrap_or_default()
    });
    let mut picking_currency = use_signal(|| false);
    let mut name_error = use_signal(|| None::<String>);
    let mut owner_error = use_signal(|| None::<String>);
    let mut last4_error = use_signal(|| None::<String>);
    let mut percent_error = use_signal(|| None::<String>);
    let mut save_error = use_signal(|| None::<String>);

    // The withdrawal fee is in the account currency; without one, in the
    // home currency, like a withdrawal with the card (CASH-03).
    // `None` only if the profile cannot be read; the fee waits for a
    // currency then.
    let initial_fee_currency = method
        .as_ref()
        .and_then(|m| m.account.fixed_fee)
        .map(|f| f.currency());
    let fee_currency = account_currency().or(initial_fee_currency).or(home);

    let title = if picking_currency() {
        t!("payment.account_currency").to_string()
    } else if is_new {
        t!("payment.new_title").to_string()
    } else {
        t!("payment.edit_title").to_string()
    };

    let mut choose_kind = move |next: PaymentMethodKind| {
        // Follow the kind with the icon until the user picked one themselves.
        if icon() == default_payment_icon(kind()) {
            icon.set(default_payment_icon(next).to_string());
        }
        if !next.has_card_number() {
            last4.set(String::new());
            last4_error.set(None);
        }
        if !next.has_account_currency() {
            account_currency.set(None);
        }
        if !next.has_fees() {
            fee_percent.set(String::new());
            fixed_fee.set(String::new());
            percent_error.set(None);
        }
        kind.set(next);
    };

    let save = move |_| {
        let mut valid = true;
        if name.read().trim().is_empty() {
            name_error.set(Some(t!("payment.name_required").to_string()));
            valid = false;
        }
        if owner.read().is_none() {
            owner_error.set(Some(t!("payment.owner_required").to_string()));
            valid = false;
        }
        let digits = match validate_last4(kind(), Some(&last4())) {
            Ok(digits) => digits,
            Err(_) => {
                last4_error.set(Some(t!("payment.last4_invalid").to_string()));
                valid = false;
                None
            }
        };
        let account = AccountTerms {
            currency: account_currency(),
            foreign_fee_percent: parse_number(&fee_percent(), format),
            fixed_fee: fee_currency.and_then(|c| parse_amount(&fixed_fee(), c, format)),
        }
        .validate(kind());
        let account = match account {
            Ok(account) => account,
            Err(PaymentMethodError::InvalidFeePercent) => {
                percent_error.set(Some(t!("payment.fee_percent_invalid").to_string()));
                valid = false;
                AccountTerms::default()
            }
            Err(error) => {
                save_error.set(Some(format!("{} {error}", t!("profile.save_error"))));
                valid = false;
                AccountTerms::default()
            }
        };
        if !valid {
            return;
        }
        let result = match &method {
            None => db.create_payment_method(NewPaymentMethod {
                name: name(),
                kind: kind(),
                owner_person_id: owner(),
                last4: digits,
                account,
                color: color(),
                icon: icon(),
            }),
            Some(existing) => {
                let updated = PaymentMethod {
                    name: name().trim().to_string(),
                    kind: kind(),
                    owner_person_id: owner(),
                    last4: digits,
                    account,
                    color: color(),
                    icon: icon(),
                    ..existing.clone()
                };
                db.update_payment_method(&updated).map(|()| updated)
            }
        };
        match result {
            Ok(saved) => {
                revision.bump();
                on_saved.call(saved);
            }
            Err(StorageError::PaymentMethod(
                PaymentMethodError::InvalidLast4 | PaymentMethodError::Last4NotAllowed,
            )) => last4_error.set(Some(t!("payment.last4_invalid").to_string())),
            Err(error) => save_error.set(Some(format!("{} {error}", t!("profile.save_error")))),
        }
    };

    if let (true, Some(shown_currency)) = (picking_currency(), fee_currency) {
        return rsx! {
            BottomSheet { title, on_close: move |_| picking_currency.set(false),
                div { class: "flex max-h-[70vh] flex-col gap-2 overflow-hidden px-3 pt-2",
                    Button {
                        variant: ButtonVariant::Secondary,
                        class: "w-full",
                        onclick: move |_| {
                            account_currency.set(None);
                            picking_currency.set(false);
                        },
                        {t!("payment.no_account_currency").to_string()}
                    }
                    CurrencyPicker {
                        selected: shown_currency,
                        nothing_selected: account_currency().is_none(),
                        on_select: move |picked: Currency| {
                            account_currency.set(Some(picked));
                            fixed_fee.set(fit_amount_text(&fixed_fee(), picked, format));
                            picking_currency.set(false);
                        },
                    }
                }
            }
        };
    }

    rsx! {
        BottomSheet { title, on_close,
            div { class: "flex max-h-[75vh] flex-col gap-5 overflow-y-auto overscroll-contain px-5 pt-3",
                TextField {
                    id: "payment-name",
                    label: t!("payment.name").to_string(),
                    value: name(),
                    placeholder: t!("payment.name_placeholder").to_string(),
                    error: name_error(),
                    oninput: move |value| {
                        name.set(value);
                        name_error.set(None);
                    },
                }
                ChipGroup { label: t!("payment.kind").to_string(),
                    for option in PaymentMethodKind::ALL {
                        Chip {
                            key: "{option.code()}",
                            label: payment_kind_name(option),
                            selected: kind() == option,
                            onclick: move |_| choose_kind(option),
                        }
                    }
                }
                ChipGroup { label: t!("payment.owner").to_string(), error: owner_error(),
                    for person in people.iter().cloned() {
                        Chip {
                            key: "{person.id.as_str()}",
                            label: person.name.clone(),
                            selected: owner.read().as_ref() == Some(&person.id),
                            onclick: move |_| {
                                owner.set(Some(person.id.clone()));
                                owner_error.set(None);
                            },
                            Avatar { name: person.name.clone(), color: person.color.clone(), size: AvatarSize::Sm }
                        }
                    }
                }
                if kind().has_card_number() {
                    TextField {
                        id: "payment-last4",
                        label: t!("payment.last4").to_string(),
                        value: last4(),
                        placeholder: t!("payment.last4_placeholder").to_string(),
                        error: last4_error(),
                        inputmode: "numeric".to_string(),
                        maxlength: 4,
                        oninput: move |value| {
                            last4.set(value);
                            last4_error.set(None);
                        },
                    }
                    p { class: "-mt-3 px-1 text-sm text-floral-white-500", {t!("payment.last4_hint").to_string()} }
                }
                if let (true, Some(shown_currency)) = (kind().has_account_currency(), fee_currency) {
                    div { class: "flex flex-col gap-1",
                        div { class: "flex min-h-12 items-center gap-3",
                            span { class: "min-w-0 flex-1 text-sm font-medium text-floral-white-300",
                                {t!("payment.account_currency").to_string()}
                            }
                            CurrencyButton {
                                currency: shown_currency,
                                unset: account_currency().is_none(),
                                label: t!("payment.account_currency").to_string(),
                                onclick: move |_| picking_currency.set(true),
                            }
                        }
                        p { class: "px-1 text-sm text-floral-white-500", {t!("payment.account_currency_hint").to_string()} }
                    }
                }
                if kind().has_fees() {
                    div { class: "flex flex-col gap-1",
                        div { class: "flex min-h-12 items-center gap-3",
                            label {
                                class: "min-w-0 flex-1 text-sm font-medium text-floral-white-300",
                                r#for: "payment-fee-percent",
                                {t!("payment.foreign_fee").to_string()}
                            }
                            CompactNumberInput {
                                id: "payment-fee-percent",
                                label: t!("payment.foreign_fee").to_string(),
                                value: fee_percent(),
                                decimals: 2,
                                unit: "%".to_string(),
                                invalid: percent_error().is_some(),
                                oninput: move |value| {
                                    fee_percent.set(value);
                                    percent_error.set(None);
                                },
                            }
                        }
                        if let Some(error) = percent_error() {
                            p { class: "px-1 text-sm text-watermelon-300", role: "alert", "{error}" }
                        }
                    }
                    if let Some(currency) = fee_currency {
                        div { class: "flex flex-col gap-1",
                            div { class: "flex min-h-12 items-center gap-3",
                                label {
                                    class: "min-w-0 flex-1 text-sm font-medium text-floral-white-300",
                                    r#for: "payment-fixed-fee",
                                    {t!("payment.fixed_fee").to_string()}
                                }
                                CompactAmountInput {
                                    id: "payment-fixed-fee",
                                    label: t!("payment.fixed_fee").to_string(),
                                    value: fixed_fee(),
                                    currency,
                                    oninput: move |value| fixed_fee.set(value),
                                }
                            }
                            p { class: "px-1 text-sm text-floral-white-500", {t!("payment.fixed_fee_hint").to_string()} }
                        }
                    }
                }
                ColorPicker {
                    label: t!("payment.color").to_string(),
                    selected: color(),
                    on_select: move |value| color.set(value),
                }
                IconPicker {
                    label: t!("payment.icon").to_string(),
                    selected: icon(),
                    color: color(),
                    on_select: move |value| icon.set(value),
                }
                ErrorBanner { error: save_error() }
                Button { class: "w-full", onclick: save, {t!("common.save").to_string()} }
            }
        }
    }
}

/// Labeled wrapping row of [`Chip`]s with an optional error below.
#[component]
fn ChipGroup(label: String, #[props(default)] error: Option<String>, children: Element) -> Element {
    rsx! {
        div { class: "flex flex-col gap-2",
            span { class: "text-sm font-medium text-floral-white-300", "{label}" }
            div { class: "flex flex-wrap gap-2", role: "radiogroup", aria_label: "{label}", {children} }
            if let Some(error) = error {
                p { class: "text-sm text-watermelon-300", role: "alert", "{error}" }
            }
        }
    }
}

/// Confirmation before deleting a method. Deleting is soft and shows a
/// toast with "Undo" (UI-11).
#[component]
fn DeleteMethodSheet(
    method: PaymentMethod,
    error: Option<String>,
    on_deleted: EventHandler<()>,
    on_error: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let name = method.name.clone();

    rsx! {
        ConfirmSheet {
            title: t!("payment.delete_title").to_string(),
            text: t!("payment.delete_text", name = name).to_string(),
            confirm_label: t!("common.delete").to_string(),
            error,
            on_confirm: move |_| match delete_with_undo(&db, &method, revision, toaster) {
                Ok(()) => on_deleted.call(()),
                Err(message) => on_error.call(message),
            },
            on_close,
        }
    }
}

/// Soft-deletes the method and offers to restore it from a toast.
fn delete_with_undo(
    db: &Db,
    method: &PaymentMethod,
    mut revision: DataRevision,
    mut toaster: Toaster,
) -> Result<(), String> {
    db.delete_payment_method(&method.id)
        .map_err(|e| format!("{} {e}", t!("payment.delete_error")))?;
    revision.bump();

    let db = db.clone();
    let id = method.id.clone();
    let undo = move || {
        let (mut revision, mut toaster) = (revision, toaster);
        match db.restore_payment_method(&id) {
            Ok(()) => revision.bump(),
            Err(e) => toaster.show(format!("{} {e}", t!("payment.restore_error")), None),
        }
    };
    toaster.show(
        t!("payment.deleted", name = method.name).to_string(),
        Some(ToastAction {
            label: t!("common.undo").to_string(),
            run: Rc::new(undo),
        }),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::PaymentMethodId;

    use super::*;

    fn person(id: &str, is_me: bool) -> Person {
        Person {
            id: PersonId::from(id),
            name: id.into(),
            color: "cerulean".into(),
            avatar_path: None,
            is_me,
            note: None,
        }
    }

    fn method(id: &str, owner: Option<&str>, archived: bool) -> PaymentMethod {
        PaymentMethod {
            id: PaymentMethodId::new(id),
            name: id.into(),
            kind: PaymentMethodKind::CreditCard,
            owner_person_id: owner.map(PersonId::from),
            last4: None,
            account: Default::default(),
            color: "cerulean".into(),
            icon: "credit-card".into(),
            archived,
        }
    }

    #[test]
    fn groups_active_methods_by_owner_in_people_order() {
        let people = vec![
            person("me", true),
            person("ben", false),
            person("cleo", false),
        ];
        let methods = vec![
            method("visa", Some("me"), false),
            method("old", Some("me"), true),
            method("paypal", Some("ben"), false),
            method("kasse", None, false),
        ];
        let overview = overview(people, methods);

        let sections: Vec<(Option<&str>, Vec<&str>)> = overview
            .sections
            .iter()
            .map(|s| {
                (
                    s.owner.as_ref().map(|o| o.id.as_str()),
                    s.methods.iter().map(|m| m.id.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            sections,
            [
                (Some("me"), vec!["visa"]),
                (Some("ben"), vec!["paypal"]),
                (None, vec!["kasse"]),
            ]
        );
        assert_eq!(overview.archived.len(), 1);
        assert_eq!(overview.archived[0].id.as_str(), "old");
    }

    #[test]
    fn subtitle_shows_only_last_digits() {
        let mut visa = method("visa", Some("me"), false);
        visa.last4 = Some("4242".into());
        let text = subtitle(&visa, Some("Ben"));
        assert!(text.ends_with("•••• 4242 · Ben"), "{text}");
    }
}
