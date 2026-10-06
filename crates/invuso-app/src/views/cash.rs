use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdArrowLeftRight, LdBanknote, LdChevronRight, LdCircleAlert, LdClipboardCheck, LdLandmark,
        LdPiggyBank, LdReceipt, LdTrash2, LdX,
    },
};
use invuso_core::domain::{
    CashError, CashMovementId, CashMovementKind, Currency, Money, PaymentMethod, PaymentMethodKind,
    Person, PersonId, local_date,
};
use invuso_core::fx;

use crate::Route;
use crate::clock::{local_now, occurred_at};
use crate::components::{
    Avatar, AvatarSize, BottomSheet, Button, ButtonVariant, CardSection, Chip, CompactAmountInput,
    CurrencyButton, CurrencyPicker, DateTimeField, EmptyState, ErrorBanner, MethodChoice,
    MoneyText, PaymentIconGlyph, PaymentMethodIcon, TopBar,
};
use crate::format::{NumberFormat, format_money, format_rate, parse_amount};
use crate::preferences::display_date;
use crate::services::cash::{CashValue, cash_value, record_withdrawal};
use crate::services::expenses::SaveExpenseError;
use crate::services::rates::{CurrencyApi, Frankfurter};
use crate::state::{DataRevision, ToastAction, Toaster};
use crate::storage::{
    CashEntry, Db, LAST_EXPENSE_CURRENCY, NewExchange, NewWithdrawal, StorageError,
};

/// Everything the cash screen shows of one person.
#[derive(Debug, Clone, PartialEq)]
struct CashData {
    /// Everyone who can have cash, "Ich" first.
    people: Vec<Person>,
    person: Person,
    home: Currency,
    /// Cash per currency, in currency order (CASH-01).
    balances: Vec<Money>,
    value: CashValue,
    entries: Vec<CashEntry>,
    /// What a withdrawal can be charged to: the person's own and ownerless
    /// methods that are not cash and not archived.
    cards: Vec<PaymentMethod>,
    /// Currency of the last expense, a likely cash currency on a trip.
    last_currency: Option<Currency>,
}

/// Which sheet is open.
#[derive(Debug, Clone, PartialEq)]
enum CashSheet {
    Withdraw,
    Exchange,
    Count,
    Entry(CashEntry),
}

/// `/cash`: one person's cash per currency and its total value in the home
/// currency (CASH-01, CASH-07), withdrawals (CASH-03), exchanges (CASH-04)
/// and cash counts (CASH-05), and every movement, cash payments of
/// expenses included (CASH-02, CASH-06). Every person has their own cash
/// (user decision in AP-22); `person` empty means "Ich".
#[component]
pub fn Cash(person: String) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let nav = use_navigator();
    let mut filter = use_signal(|| None::<Currency>);
    let mut sheet = use_signal(|| None::<CashSheet>);

    let data = use_memo(use_reactive!(|person| {
        revision.track();
        load(&db, &person).map_err(|e| e.to_string())
    }));

    rsx! {
        TopBar { title: t!("page.cash").to_string(), show_back: true }
        match &*data.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("cash.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(data) => {
                let currencies: Vec<Currency> = data.balances.iter().map(|b| b.currency()).collect();
                let shown_filter = filter().filter(|c| currencies.contains(c));
                let entries: Vec<CashEntry> = data
                    .entries
                    .iter()
                    .filter(|e| shown_filter.is_none_or(|c| e.amount.currency() == c))
                    .cloned()
                    .collect();
                let suggested = suggested_currency(data, shown_filter);
                rsx! {
                    div { class: "mx-4 flex flex-col gap-5 pt-6 safe-area-x",
                        if data.people.len() > 1 {
                            div {
                                class: "-mx-4 flex gap-2 overflow-x-auto px-4 pb-1",
                                role: "radiogroup",
                                aria_label: t!("cash.person").to_string(),
                                for p in data.people.iter().cloned() {
                                    Chip {
                                        key: "{p.id.as_str()}",
                                        label: p.name.clone(),
                                        selected: p.id == data.person.id,
                                        onclick: {
                                            let id = if p.is_me { String::new() } else { p.id.as_str().to_string() };
                                            move |_| {
                                                filter.set(None);
                                                nav.replace(Route::Cash { person: id.clone() });
                                            }
                                        },
                                        Avatar { name: p.name.clone(), color: p.color.clone(), size: AvatarSize::Sm }
                                    }
                                }
                            }
                        }
                        BalanceCard {
                            balances: data.balances.clone(),
                            value: data.value.clone(),
                            selected: shown_filter,
                            on_select: move |currency| filter.set(currency),
                        }
                        div { class: "grid grid-cols-3 gap-2",
                            ActionButton {
                                label: t!("cash.withdraw").to_string(),
                                onclick: move |_| sheet.set(Some(CashSheet::Withdraw)),
                                Icon { icon: LdLandmark, class: "h-5 w-5" }
                            }
                            ActionButton {
                                label: t!("cash.exchange").to_string(),
                                onclick: move |_| sheet.set(Some(CashSheet::Exchange)),
                                Icon { icon: LdArrowLeftRight, class: "h-5 w-5" }
                            }
                            ActionButton {
                                label: t!("cash.count").to_string(),
                                onclick: move |_| sheet.set(Some(CashSheet::Count)),
                                Icon { icon: LdClipboardCheck, class: "h-5 w-5" }
                            }
                        }
                        if entries.is_empty() {
                            EmptyState {
                                title: t!("cash.no_movements_title").to_string(),
                                text: t!("cash.no_movements_text").to_string(),
                                Icon { icon: LdBanknote, class: "h-8 w-8" }
                            }
                        } else {
                            CardSection { title: t!("cash.movements").to_string(),
                                for (index, entry) in entries.into_iter().enumerate() {
                                    EntryRow {
                                        key: "{index}",
                                        entry: entry.clone(),
                                        onclick: move |_| match (&entry.movement_id, &entry.expense_id) {
                                            (None, Some(expense)) => {
                                                nav.push(Route::ExpenseDetail { id: expense.as_str().to_string() });
                                            }
                                            _ => sheet.set(Some(CashSheet::Entry(entry.clone()))),
                                        },
                                    }
                                }
                            }
                        }
                    }
                    match sheet() {
                        Some(CashSheet::Withdraw) => rsx! {
                            WithdrawSheet {
                                person: data.person.id.clone(),
                                home: data.home,
                                cards: data.cards.clone(),
                                currency: if suggested == data.home { data.last_currency.unwrap_or(suggested) } else { suggested },
                                on_close: move |_| sheet.set(None),
                            }
                        },
                        Some(CashSheet::Exchange) => rsx! {
                            ExchangeSheet {
                                person: data.person.id.clone(),
                                given: data.home,
                                received: if suggested == data.home { data.last_currency.unwrap_or(suggested) } else { suggested },
                                on_close: move |_| sheet.set(None),
                            }
                        },
                        Some(CashSheet::Count) => rsx! {
                            CountSheet {
                                person: data.person.id.clone(),
                                currency: suggested,
                                on_close: move |_| sheet.set(None),
                            }
                        },
                        Some(CashSheet::Entry(entry)) => rsx! {
                            EntrySheet { entry, on_close: move |_| sheet.set(None) }
                        },
                        None => rsx! {},
                    }
                }
            }
        }
    }
}

fn load(db: &Db, requested: &str) -> Result<CashData, StorageError> {
    let mut people = db.people()?;
    people.sort_by_key(|p| !p.is_me);
    let person = people
        .iter()
        .find(|p| p.id.as_str() == requested)
        .or_else(|| people.iter().find(|p| p.is_me))
        .cloned()
        .ok_or(StorageError::NotFound)?;
    let home = db.expense_base_currency(None)?;
    let balances = db.cash_balances(&person.id)?;
    let value = cash_value(db, &balances, home)?;
    let cards = db
        .payment_methods()?
        .into_iter()
        .filter(|m| !m.archived && m.kind != PaymentMethodKind::Cash)
        .filter(|m| m.owner_person_id.as_ref().is_none_or(|o| o == &person.id))
        .collect();
    Ok(CashData {
        entries: db.cash_entries(&person.id)?,
        last_currency: db.currency_setting(LAST_EXPENSE_CURRENCY)?,
        people,
        person,
        home,
        balances,
        value,
        cards,
    })
}

/// Currency the sheets start with: the one filtered for, else the first
/// cash currency other than the home currency, else the home currency.
fn suggested_currency(data: &CashData, filter: Option<Currency>) -> Currency {
    filter
        .or_else(|| {
            data.balances
                .iter()
                .map(|b| b.currency())
                .find(|c| *c != data.home)
        })
        .unwrap_or(data.home)
}

/// Total value and cash per currency; tapping a currency shows only its
/// movements (CASH-06), tapping it again shows all.
#[component]
fn BalanceCard(
    balances: Vec<Money>,
    value: CashValue,
    selected: Option<Currency>,
    on_select: EventHandler<Option<Currency>>,
) -> Element {
    let missing = value
        .missing
        .iter()
        .map(|c| c.code())
        .collect::<Vec<_>>()
        .join(", ");

    rsx! {
        section { class: "flex flex-col gap-4 rounded-2xl border border-jet-black-800 bg-jet-black-900 px-4 py-4",
            div { class: "flex flex-col gap-1",
                span { class: "text-sm text-floral-white-400", {t!("cash.value").to_string()} }
                MoneyText { amount: value.total, class: "text-3xl font-semibold text-floral-white-50" }
                if !value.missing.is_empty() {
                    span { class: "text-sm text-pale-oak-300",
                        {t!("cash.value_missing", currencies = missing).to_string()}
                    }
                }
            }
            if balances.is_empty() {
                p { class: "text-base text-floral-white-300", {t!("cash.none_yet").to_string()} }
            } else {
                div {
                    class: "-mx-2 flex flex-col",
                    role: "radiogroup",
                    aria_label: t!("cash.filter").to_string(),
                    for balance in balances {
                        button {
                            key: "{balance.currency().code()}",
                            class: "flex min-h-12 items-center gap-3 rounded-xl px-2 text-left active:bg-jet-black-800 transition-colors ease-apple",
                            class: if selected == Some(balance.currency()) { "bg-cerulean-900" },
                            r#type: "button",
                            role: "radio",
                            aria_checked: if selected == Some(balance.currency()) { "true" } else { "false" },
                            onclick: move |_| {
                                let currency = balance.currency();
                                on_select.call((selected != Some(currency)).then_some(currency));
                            },
                            span { class: "w-12 text-base font-semibold text-cerulean-200 tabular-nums", "{balance.currency().code()}" }
                            span { class: "min-w-0 flex-1 truncate text-sm text-floral-white-400", "{balance.currency().name()}" }
                            MoneyText {
                                amount: balance,
                                class: if balance.is_negative() { "text-lg font-semibold text-watermelon-300" } else { "text-lg font-semibold text-floral-white-50" },
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One of the three actions under the balance.
#[component]
fn ActionButton(label: String, onclick: EventHandler<()>, children: Element) -> Element {
    rsx! {
        button {
            class: "flex min-h-20 flex-col items-center justify-center gap-1.5 rounded-2xl border border-jet-black-800 bg-jet-black-900 px-2 text-sm font-medium text-floral-white-100 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            span { class: "flex h-10 w-10 items-center justify-center rounded-full bg-cerulean-800 text-cerulean-200",
                {children}
            }
            "{label}"
        }
    }
}

/// Name of a movement in the list.
fn entry_title(entry: &CashEntry) -> String {
    match (entry.kind, &entry.title) {
        (CashMovementKind::Expense, Some(title)) => title.clone(),
        (kind, _) => kind_name(kind),
    }
}

fn kind_name(kind: CashMovementKind) -> String {
    match kind {
        CashMovementKind::Withdrawal => t!("cash.kind_withdrawal"),
        CashMovementKind::Expense => t!("cash.kind_expense"),
        CashMovementKind::Exchange => t!("cash.kind_exchange"),
        CashMovementKind::Deposit => t!("cash.kind_deposit"),
        CashMovementKind::Correction => t!("cash.kind_correction"),
    }
    .to_string()
}

/// "03.10.2026 · Visa · 186,20 € belastet".
fn entry_detail(entry: &CashEntry, format: NumberFormat) -> String {
    let mut parts = vec![display_date(local_date(&entry.occurred_at))];
    parts.extend(entry.method.clone());
    match (entry.kind, entry.counterpart) {
        (CashMovementKind::Withdrawal, Some(charged)) => parts.push(
            t!(
                "cash.charged_detail",
                amount = format_money(charged, format)
            )
            .to_string(),
        ),
        (CashMovementKind::Exchange, Some(other)) => {
            parts.push(t!("cash.exchange_detail", amount = format_money(other, format)).to_string())
        }
        _ => {}
    }
    parts.join(" · ")
}

/// One movement; tapping opens its expense or its details.
#[component]
fn EntryRow(entry: CashEntry, onclick: EventHandler<()>) -> Element {
    let format = NumberFormat::current();
    let title = entry_title(&entry);
    let detail = entry_detail(&entry, format);

    rsx! {
        button {
            class: "flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            KindIcon { kind: entry.kind }
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "truncate text-base text-floral-white-50", "{title}" }
                span { class: "truncate text-sm text-floral-white-400", "{detail}" }
            }
            MoneyText { amount: entry.amount, signed: true, class: "shrink-0 text-base font-semibold" }
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
        }
    }
}

#[component]
fn KindIcon(kind: CashMovementKind) -> Element {
    rsx! {
        span { class: "flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-jet-black-800 text-cerulean-300",
            match kind {
                CashMovementKind::Withdrawal => rsx! { Icon { icon: LdLandmark, class: "h-5 w-5" } },
                CashMovementKind::Expense => rsx! { Icon { icon: LdReceipt, class: "h-5 w-5" } },
                CashMovementKind::Exchange => rsx! { Icon { icon: LdArrowLeftRight, class: "h-5 w-5" } },
                CashMovementKind::Deposit => rsx! { Icon { icon: LdPiggyBank, class: "h-5 w-5" } },
                CashMovementKind::Correction => rsx! { Icon { icon: LdClipboardCheck, class: "h-5 w-5" } },
            }
        }
    }
}

/// Labeled amount field with its currency, as a row of a sheet.
#[component]
fn AmountRow(
    id: String,
    label: String,
    value: String,
    currency: Currency,
    oninput: EventHandler<String>,
    /// Lets the currency be changed; `None` fixes it.
    #[props(default)]
    on_currency: Option<EventHandler<()>>,
    #[props(default)] hint: Option<String>,
) -> Element {
    rsx! {
        div { class: "flex flex-col gap-1",
            div { class: "flex min-h-12 items-center gap-2",
                span { class: "min-w-0 flex-1 text-base text-floral-white-200", "{label}" }
                CompactAmountInput { id, label: label.clone(), value, currency, oninput }
                if let Some(on_currency) = on_currency {
                    CurrencyButton {
                        currency,
                        label: t!("cash.currency").to_string(),
                        onclick: move |_| on_currency.call(()),
                    }
                }
            }
            if let Some(hint) = hint {
                p { class: "text-sm text-floral-white-400", "{hint}" }
            }
        }
    }
}

/// Error text of a failed booking.
fn save_error_text(error: &StorageError) -> String {
    match error {
        StorageError::Cash(CashError::NonPositiveAmount) => t!("cash.enter_amount").to_string(),
        StorageError::Cash(CashError::SameCurrency) => t!("cash.same_currency").to_string(),
        other => format!("{} {other}", t!("cash.save_error")),
    }
}

/// Toast with an undo that deletes the movement just booked.
fn toast_with_undo(
    db: Db,
    revision: DataRevision,
    mut toaster: Toaster,
    message: String,
    id: CashMovementId,
) {
    let undo = move || {
        let (mut revision, mut toaster) = (revision, toaster);
        match db.delete_cash_movement(&id) {
            Ok(()) => revision.bump(),
            Err(e) => toaster.show(format!("{} {e}", t!("cash.delete_error")), None),
        }
    };
    toaster.show(
        message,
        Some(ToastAction {
            label: t!("common.undo").to_string(),
            run: Rc::new(undo),
        }),
    );
}

/// What a withdrawal sheet shows instead of its form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WithdrawChoosing {
    Currency,
    Card,
}

/// Books a withdrawal (CASH-03): amount and currency, the charged card,
/// what it was charged (the actual rate) and a fee, which becomes an
/// expense (user decision in AP-22). Cards carry no currency yet (PAY-04),
/// so charge and fee are in the home currency.
#[component]
fn WithdrawSheet(
    person: PersonId,
    home: Currency,
    cards: Vec<PaymentMethod>,
    currency: Currency,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let format = NumberFormat::current();

    let mut currency = use_signal(|| currency);
    let mut amount = use_signal(String::new);
    let mut card = use_signal({
        let cards = cards.clone();
        move || match cards.as_slice() {
            [only] => Some(only.id.clone()),
            _ => None,
        }
    });
    let mut charged = use_signal(String::new);
    let mut fee = use_signal(String::new);
    let mut date = use_signal(|| local_now().0);
    let mut time = use_signal(|| local_now().1);
    let mut choosing = use_signal(|| None::<WithdrawChoosing>);
    let mut error = use_signal(|| None::<String>);
    let mut saving = use_signal(|| false);

    let cash = currency();
    let parsed = parse_amount(&amount(), cash, format).filter(|m| m.amount_minor() > 0);
    // The estimate only helps to spot a typo in the charged amount.
    let estimate = parsed.filter(|_| cash != home).and_then(|money| {
        db.rate_near(cash, home, &date())
            .ok()
            .flatten()
            .and_then(|near| fx::convert(money, &near.quote.rate).ok())
    });
    let charged_hint = match estimate {
        Some(estimate) => t!("cash.charged_hint", amount = format_money(estimate, format)),
        None => t!("cash.charged_hint_no_rate"),
    }
    .to_string();
    let chosen_card = card().and_then(|id| cards.iter().find(|m| m.id == id).cloned());

    let save = {
        let db = db.clone();
        let person = person.clone();
        move |_| {
            if saving() {
                return;
            }
            let Some(money) =
                parse_amount(&amount(), currency(), format).filter(|m| m.amount_minor() > 0)
            else {
                error.set(Some(t!("cash.enter_amount").to_string()));
                return;
            };
            let Some(at) = occurred_at(&date(), &time()) else {
                error.set(Some(t!("expense.date_time_invalid").to_string()));
                return;
            };
            let charged_money = (currency() != home)
                .then(|| parse_amount(&charged(), home, format))
                .flatten()
                .filter(|m| m.amount_minor() > 0);
            let fee_money = parse_amount(&fee(), home, format).filter(|m| m.amount_minor() > 0);
            let new = NewWithdrawal {
                person: person.clone(),
                amount: money,
                card: card(),
                charged: charged_money,
                occurred_at: at,
            };
            let fee_title = t!("cash.fee_title").to_string();
            let worker_db = db.clone();
            let toast_db = db.clone();
            saving.set(true);
            error.set(None);
            spawn(async move {
                // The fee's rate may have to be fetched; keep the network
                // off the UI thread.
                let outcome = tokio::task::spawn_blocking(move || {
                    record_withdrawal(
                        &worker_db,
                        &Frankfurter,
                        &CurrencyApi,
                        new,
                        fee_money,
                        &fee_title,
                    )
                })
                .await;
                match outcome {
                    Ok(Ok(id)) => {
                        let mut revision = revision;
                        revision.bump();
                        on_close.call(());
                        toast_with_undo(
                            toast_db,
                            revision,
                            toaster,
                            t!("cash.withdrawal_saved").to_string(),
                            id,
                        );
                    }
                    Ok(Err(SaveExpenseError::NoRate { from, to })) => {
                        saving.set(false);
                        error.set(Some(
                            t!("expense.no_rate", from = from.code(), to = to.code()).to_string(),
                        ));
                    }
                    Ok(Err(SaveExpenseError::Storage(e))) => {
                        saving.set(false);
                        error.set(Some(save_error_text(&e)));
                    }
                    Err(e) => {
                        saving.set(false);
                        error.set(Some(format!("{} {e}", t!("cash.save_error"))));
                    }
                }
            });
        }
    };

    let body = match choosing() {
        Some(WithdrawChoosing::Currency) => rsx! {
            div { class: "flex max-h-[70vh] flex-col overflow-hidden px-3 pt-2",
                CurrencyPicker {
                    selected: cash,
                    on_select: move |picked| {
                        currency.set(picked);
                        choosing.set(None);
                    },
                }
            }
        },
        Some(WithdrawChoosing::Card) => rsx! {
            div { class: "flex max-h-[70vh] flex-col gap-1 overflow-y-auto overscroll-contain px-3 pt-2", role: "listbox",
                MethodChoice {
                    label: t!("cash.no_card").to_string(),
                    selected: card().is_none(),
                    onclick: move |_| {
                        card.set(None);
                        choosing.set(None);
                    },
                    span { class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-jet-black-800 text-floral-white-400",
                        Icon { icon: LdX, class: "h-5 w-5" }
                    }
                }
                for entry in cards.iter().cloned() {
                    MethodChoice {
                        key: "{entry.id.as_str()}",
                        label: entry.name.clone(),
                        selected: card().as_ref() == Some(&entry.id),
                        onclick: move |_| {
                            card.set(Some(entry.id.clone()));
                            choosing.set(None);
                        },
                        PaymentMethodIcon { icon: entry.icon.clone(), color: entry.color.clone() }
                    }
                }
            }
        },
        None => rsx! {
            div { class: "flex max-h-[75vh] flex-col gap-4 overflow-y-auto overscroll-contain px-5 pt-3",
                AmountRow {
                    id: "withdraw-amount",
                    label: t!("cash.amount").to_string(),
                    value: amount(),
                    currency: cash,
                    oninput: move |text| {
                        amount.set(text);
                        error.set(None);
                    },
                    on_currency: move |_| choosing.set(Some(WithdrawChoosing::Currency)),
                }
                div { class: "flex min-h-12 items-center gap-3",
                    span { class: "flex-1 text-base text-floral-white-200", {t!("cash.card").to_string()} }
                    ChoiceButton {
                        onclick: move |_| choosing.set(Some(WithdrawChoosing::Card)),
                        if let Some(entry) = &chosen_card {
                            PaymentIconGlyph { icon: entry.icon.clone(), class: "h-4 w-4".to_string() }
                            span { "{entry.name}" }
                        } else {
                            span { class: "text-floral-white-400", {t!("cash.choose_card").to_string()} }
                        }
                    }
                }
                if cash != home {
                    AmountRow {
                        id: "withdraw-charged",
                        label: t!("cash.charged").to_string(),
                        value: charged(),
                        currency: home,
                        oninput: move |text| charged.set(text),
                        hint: charged_hint,
                    }
                }
                AmountRow {
                    id: "withdraw-fee",
                    label: t!("cash.fee").to_string(),
                    value: fee(),
                    currency: home,
                    oninput: move |text| fee.set(text),
                    hint: t!("cash.fee_hint").to_string(),
                }
                DateTimeField {
                    id: "withdraw-time",
                    label: t!("cash.date").to_string(),
                    date: date(),
                    time: time(),
                    date_label: t!("expense.date").to_string(),
                    time_label: t!("expense.time").to_string(),
                    on_date: move |value| date.set(value),
                    on_time: move |value| time.set(value),
                }
                ErrorBanner { error: error() }
                Button { class: "w-full", disabled: saving(), onclick: save,
                    Icon { icon: LdLandmark, class: "h-5 w-5" }
                    if saving() {
                        {t!("cash.saving").to_string()}
                    } else {
                        {t!("cash.save_withdrawal").to_string()}
                    }
                }
            }
        },
    };

    let title = match choosing() {
        Some(WithdrawChoosing::Currency) => t!("cash.currency"),
        Some(WithdrawChoosing::Card) => t!("cash.card"),
        None => t!("cash.withdraw"),
    }
    .to_string();
    rsx! {
        BottomSheet {
            title,
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

/// Which currency an exchange sheet is picking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExchangeSide {
    Given,
    Received,
}

/// Books an exchange (CASH-04) with the rate the two amounts imply.
#[component]
fn ExchangeSheet(
    person: PersonId,
    given: Currency,
    received: Currency,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let mut revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let format = NumberFormat::current();

    let mut given_currency = use_signal(|| given);
    let mut received_currency = use_signal(|| received);
    let mut given_amount = use_signal(String::new);
    let mut received_amount = use_signal(String::new);
    let mut date = use_signal(|| local_now().0);
    let mut time = use_signal(|| local_now().1);
    let mut choosing = use_signal(|| None::<ExchangeSide>);
    let mut error = use_signal(|| None::<String>);

    let given_money = parse_amount(&given_amount(), given_currency(), format);
    let received_money = parse_amount(&received_amount(), received_currency(), format);
    let rate = match (given_money, received_money) {
        (Some(from), Some(to)) => fx::implied_rate(from, to).ok(),
        _ => None,
    };

    let save = move |_| {
        if given_currency() == received_currency() {
            error.set(Some(t!("cash.same_currency").to_string()));
            return;
        }
        let (Some(from), Some(to)) = (
            parse_amount(&given_amount(), given_currency(), format),
            parse_amount(&received_amount(), received_currency(), format),
        ) else {
            error.set(Some(t!("cash.enter_amount").to_string()));
            return;
        };
        let Some(at) = occurred_at(&date(), &time()) else {
            error.set(Some(t!("expense.date_time_invalid").to_string()));
            return;
        };
        let new = NewExchange {
            person: person.clone(),
            given: from,
            received: to,
            occurred_at: at,
        };
        match db.record_exchange(new) {
            Ok(id) => {
                revision.bump();
                on_close.call(());
                toast_with_undo(
                    db.clone(),
                    revision,
                    toaster,
                    t!("cash.exchange_saved").to_string(),
                    id,
                );
            }
            Err(e) => error.set(Some(save_error_text(&e))),
        }
    };

    if let Some(side) = choosing() {
        let selected = match side {
            ExchangeSide::Given => given_currency(),
            ExchangeSide::Received => received_currency(),
        };
        return rsx! {
            BottomSheet { title: t!("cash.currency").to_string(), on_close: move |_| choosing.set(None),
                div { class: "flex max-h-[70vh] flex-col overflow-hidden px-3 pt-2",
                    CurrencyPicker {
                        selected,
                        on_select: move |picked| {
                            match side {
                                ExchangeSide::Given => given_currency.set(picked),
                                ExchangeSide::Received => received_currency.set(picked),
                            }
                            error.set(None);
                            choosing.set(None);
                        },
                    }
                }
            }
        };
    }

    rsx! {
        BottomSheet { title: t!("cash.exchange").to_string(), on_close: move |_| on_close.call(()),
            div { class: "flex max-h-[75vh] flex-col gap-4 overflow-y-auto overscroll-contain px-5 pt-3",
                AmountRow {
                    id: "exchange-given",
                    label: t!("cash.given").to_string(),
                    value: given_amount(),
                    currency: given_currency(),
                    oninput: move |text| {
                        given_amount.set(text);
                        error.set(None);
                    },
                    on_currency: move |_| choosing.set(Some(ExchangeSide::Given)),
                }
                AmountRow {
                    id: "exchange-received",
                    label: t!("cash.received").to_string(),
                    value: received_amount(),
                    currency: received_currency(),
                    oninput: move |text| {
                        received_amount.set(text);
                        error.set(None);
                    },
                    on_currency: move |_| choosing.set(Some(ExchangeSide::Received)),
                }
                if let Some(rate) = rate {
                    p { class: "text-sm text-floral-white-300 tabular-nums",
                        {t!(
                            "cash.rate",
                            from = rate.base().code(),
                            value = format_rate(rate.value(), format),
                            to = rate.quote().code()
                        ).to_string()}
                    }
                }
                DateTimeField {
                    id: "exchange-time",
                    label: t!("cash.date").to_string(),
                    date: date(),
                    time: time(),
                    date_label: t!("expense.date").to_string(),
                    time_label: t!("expense.time").to_string(),
                    on_date: move |value| date.set(value),
                    on_time: move |value| time.set(value),
                }
                ErrorBanner { error: error() }
                Button { class: "w-full", onclick: save,
                    Icon { icon: LdArrowLeftRight, class: "h-5 w-5" }
                    {t!("cash.save_exchange").to_string()}
                }
            }
        }
    }
}

/// Cash count (CASH-05): what is really in the wallet; the difference to
/// what the app expects is booked as a correction.
#[component]
fn CountSheet(person: PersonId, currency: Currency, on_close: EventHandler<()>) -> Element {
    let db = use_context::<Db>();
    let mut revision = use_context::<DataRevision>();
    let mut toaster = use_context::<Toaster>();
    let format = NumberFormat::current();

    let mut currency = use_signal(|| currency);
    let mut counted = use_signal(String::new);
    let mut date = use_signal(|| local_now().0);
    let mut time = use_signal(|| local_now().1);
    let mut picking = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    // What the count is compared with: the cash at the chosen time.
    let expected = occurred_at(&date(), &time())
        .and_then(|at| db.cash_balance_at(&person, currency(), &at).ok())
        .unwrap_or_else(|| Money::zero(currency()));

    let save = move |_| {
        // An empty field means "nothing left", like a counted 0.
        let text = counted();
        let money = if text.is_empty() {
            Some(Money::zero(currency()))
        } else {
            parse_amount(&text, currency(), format)
        };
        let Some(money) = money else {
            error.set(Some(t!("cash.enter_amount").to_string()));
            return;
        };
        let Some(at) = occurred_at(&date(), &time()) else {
            error.set(Some(t!("expense.date_time_invalid").to_string()));
            return;
        };
        match db.record_cash_count(&person, money, &at) {
            Ok(Some((id, correction))) => {
                revision.bump();
                on_close.call(());
                let message = t!(
                    "cash.count_saved",
                    amount = format_money(correction, format)
                );
                toast_with_undo(db.clone(), revision, toaster, message.to_string(), id);
            }
            Ok(None) => {
                on_close.call(());
                toaster.show(t!("cash.count_matches").to_string(), None);
            }
            Err(e) => error.set(Some(save_error_text(&e))),
        }
    };

    if picking() {
        return rsx! {
            BottomSheet { title: t!("cash.currency").to_string(), on_close: move |_| picking.set(false),
                div { class: "flex max-h-[70vh] flex-col overflow-hidden px-3 pt-2",
                    CurrencyPicker {
                        selected: currency(),
                        on_select: move |picked| {
                            currency.set(picked);
                            picking.set(false);
                        },
                    }
                }
            }
        };
    }

    rsx! {
        BottomSheet { title: t!("cash.count").to_string(), on_close: move |_| on_close.call(()),
            div { class: "flex max-h-[75vh] flex-col gap-4 overflow-y-auto overscroll-contain px-5 pt-3",
                AmountRow {
                    id: "count-amount",
                    label: t!("cash.counted").to_string(),
                    value: counted(),
                    currency: currency(),
                    oninput: move |text| {
                        counted.set(text);
                        error.set(None);
                    },
                    on_currency: move |_| picking.set(true),
                    hint: format!(
                        "{} {}",
                        t!("cash.expected", amount = format_money(expected, format)),
                        t!("cash.count_hint")
                    ),
                }
                DateTimeField {
                    id: "count-time",
                    label: t!("cash.date").to_string(),
                    date: date(),
                    time: time(),
                    date_label: t!("expense.date").to_string(),
                    time_label: t!("expense.time").to_string(),
                    on_date: move |value| date.set(value),
                    on_time: move |value| time.set(value),
                }
                ErrorBanner { error: error() }
                Button { class: "w-full", onclick: save,
                    Icon { icon: LdClipboardCheck, class: "h-5 w-5" }
                    {t!("cash.save_count").to_string()}
                }
            }
        }
    }
}

/// Details of a stored movement, with the way to its fee expense and to
/// deleting it (undo in the toast).
#[component]
fn EntrySheet(entry: CashEntry, on_close: EventHandler<()>) -> Element {
    let db = use_context::<Db>();
    let mut revision = use_context::<DataRevision>();
    let mut toaster = use_context::<Toaster>();
    let nav = use_navigator();
    let format = NumberFormat::current();
    let title = entry_title(&entry);
    let detail = entry_detail(&entry, format);
    let hint = match (entry.kind, &entry.expense_id) {
        (CashMovementKind::Exchange, _) => Some(t!("cash.delete_hint_exchange").to_string()),
        (CashMovementKind::Withdrawal, Some(_)) => Some(t!("cash.delete_hint_fee").to_string()),
        _ => None,
    };

    let delete = {
        let id = entry.movement_id.clone();
        move |_| {
            let Some(id) = id.clone() else { return };
            match db.delete_cash_movement(&id) {
                Ok(()) => {
                    revision.bump();
                    on_close.call(());
                    let db = db.clone();
                    let undo = move || {
                        let (mut revision, mut toaster) = (revision, toaster);
                        match db.restore_cash_movement(&id) {
                            Ok(()) => revision.bump(),
                            Err(e) => {
                                toaster.show(format!("{} {e}", t!("cash.restore_error")), None)
                            }
                        }
                    };
                    toaster.show(
                        t!("cash.deleted").to_string(),
                        Some(ToastAction {
                            label: t!("common.undo").to_string(),
                            run: Rc::new(undo),
                        }),
                    );
                }
                Err(e) => toaster.show(format!("{} {e}", t!("cash.delete_error")), None),
            }
        }
    };

    rsx! {
        BottomSheet { title: t!("cash.details").to_string(), on_close: move |_| on_close.call(()),
            div { class: "flex flex-col gap-4 px-5 pt-3",
                div { class: "flex items-center gap-3",
                    KindIcon { kind: entry.kind }
                    span { class: "flex min-w-0 flex-1 flex-col",
                        span { class: "truncate text-base text-floral-white-50", "{title}" }
                        span { class: "text-sm text-floral-white-400", "{detail}" }
                    }
                    MoneyText { amount: entry.amount, signed: true, class: "shrink-0 text-lg font-semibold" }
                }
                if let Some(fee) = entry.fee {
                    p { class: "text-sm text-floral-white-300",
                        "{t!(\"cash.fee\")}: {format_money(fee, format)}"
                    }
                }
                if let Some(expense) = entry.expense_id.clone() {
                    Button {
                        variant: ButtonVariant::Secondary,
                        class: "w-full",
                        onclick: move |_| {
                            on_close.call(());
                            nav.push(Route::ExpenseDetail { id: expense.as_str().to_string() });
                        },
                        Icon { icon: LdReceipt, class: "h-5 w-5" }
                        {t!("cash.open_expense").to_string()}
                    }
                }
                if let Some(hint) = hint {
                    p { class: "text-sm text-floral-white-400", "{hint}" }
                }
                Button { variant: ButtonVariant::Danger, class: "w-full", onclick: delete,
                    Icon { icon: LdTrash2, class: "h-5 w-5" }
                    {t!("common.delete").to_string()}
                }
            }
        }
    }
}

/// Opens a list to choose from; shows the current choice.
#[component]
fn ChoiceButton(onclick: EventHandler<()>, children: Element) -> Element {
    rsx! {
        button {
            class: "flex min-h-11 items-center gap-2 rounded-full bg-jet-black-800 px-3 text-sm text-floral-white-200 active:bg-jet-black-700 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            {children}
            Icon { icon: LdChevronRight, class: "h-4 w-4 text-floral-white-500" }
        }
    }
}
