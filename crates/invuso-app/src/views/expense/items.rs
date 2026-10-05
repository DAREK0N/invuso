//! The line items of an expense (idee.md 7.2 step 5, OCR-30..35, SPL-02):
//! a list that looks like the printed receipt, and a sheet to correct one
//! line, split or merge it and say who had how much of it.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdArrowDown, LdArrowUp, LdMerge, LdMinus, LdPlus, LdSplit, LdTrash2, LdTriangleAlert,
    },
};
use invuso_core::Decimal;
use invuso_core::domain::{
    Currency, GroupMember, LineItem, LineItemError, LineItemKind, Money, Person, PersonId,
    line_items_sum,
};
use invuso_core::receipt::{ItemKind, ParsedReceipt};
use invuso_core::split::allocate;

use crate::components::{
    Avatar, AvatarEntry, AvatarSize, AvatarStack, BottomSheet, Chip, CompactAmountInput,
    CompactNumberInput, TextField,
};
use crate::format::{
    NumberFormat, amount_text, fit_amount_text, format_money, format_number, format_plain,
    number_text, parse_amount, parse_number,
};

/// Decimals a quantity can be typed with (weights in kg: `0,452`).
const QUANTITY_DECIMALS: u32 = 3;

static NEXT_KEY: AtomicU64 = AtomicU64::new(1);

/// A line while the form is open, with a key that stays the same when
/// lines are moved, split or merged.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ItemDraft {
    pub key: u64,
    pub item: LineItem,
}

impl ItemDraft {
    pub(super) fn new(item: LineItem) -> Self {
        Self {
            key: NEXT_KEY.fetch_add(1, Ordering::Relaxed),
            item,
        }
    }
}

/// The lines the parser read (OCR-10..13), ready to correct.
pub(super) fn drafts_from_parsed(receipt: &ParsedReceipt) -> Vec<ItemDraft> {
    receipt
        .items
        .iter()
        .map(|parsed| {
            let kind = match parsed.kind {
                ItemKind::Article => LineItemKind::Article,
                ItemKind::Discount => LineItemKind::Discount,
            };
            ItemDraft::new(LineItem {
                original_text: parsed.text.clone(),
                quantity: parsed.quantity,
                unit_price_minor: parsed.unit_price.map(|m| m.amount_minor()),
                total_minor: kind.signed(parsed.total_price.amount_minor()),
                kind,
                ..LineItem::default()
            })
        })
        .collect()
}

pub(super) fn drafts_from_saved(items: &[LineItem]) -> Vec<ItemDraft> {
    items.iter().cloned().map(ItemDraft::new).collect()
}

pub(super) fn line_items(drafts: &[ItemDraft]) -> Vec<LineItem> {
    drafts.iter().map(|d| d.item.clone()).collect()
}

/// What can be done to one line from its sheet (OCR-31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ItemAction {
    Split,
    MergeNext,
    MoveUp,
    MoveDown,
    Delete,
}

/// Applies `action` to the line with `key`. Returns whether its sheet can
/// stay open: moving keeps the line as it is, the other actions replace
/// or remove it.
pub(super) fn apply(
    drafts: &mut Vec<ItemDraft>,
    key: u64,
    action: ItemAction,
) -> Result<bool, LineItemError> {
    let Some(index) = drafts.iter().position(|d| d.key == key) else {
        return Ok(false);
    };
    match action {
        ItemAction::Split => {
            let (rest, part) = drafts[index].item.split()?;
            drafts[index].item = rest;
            drafts.insert(index + 1, ItemDraft::new(part));
            Ok(false)
        }
        ItemAction::MergeNext => {
            if let Some(next) = drafts.get(index + 1) {
                let merged = drafts[index].item.merge(&next.item)?;
                drafts[index].item = merged;
                drafts.remove(index + 1);
            }
            Ok(false)
        }
        ItemAction::MoveUp => {
            if index > 0 {
                drafts.swap(index, index - 1);
            }
            Ok(true)
        }
        ItemAction::MoveDown => {
            if index + 1 < drafts.len() {
                drafts.swap(index, index + 1);
            }
            Ok(true)
        }
        ItemAction::Delete => {
            drafts.remove(index);
            Ok(false)
        }
    }
}

/// Shortens the amounts to what `to` allows after switching the currency,
/// like the other amount fields of the form (`12,50 €` → `12 ¥`).
pub(super) fn fit_currency(
    drafts: &mut [ItemDraft],
    from: Currency,
    to: Currency,
    format: NumberFormat,
) {
    let fit = |minor: i64| {
        let text = amount_text(Money::new(minor.abs(), from), format);
        let magnitude = parse_amount(&fit_amount_text(&text, to, format), to, format)
            .map_or(0, |m| m.amount_minor());
        if minor < 0 { -magnitude } else { magnitude }
    };
    for draft in drafts {
        draft.item.total_minor = fit(draft.item.total_minor);
        draft.item.unit_price_minor = draft.item.unit_price_minor.map(fit);
    }
}

/// The lines in the look of the printed receipt (user wish in AP-19):
/// correction or translation large, the printed text small underneath
/// (OCR-35), `quantity × unit price`, the total and who had it (OCR-33).
/// Below the lines their sum against the expense's total (OCR-14, OCR-32).
/// Without `on_open` the list only shows the lines (expense detail).
#[component]
pub(super) fn ReceiptItems(
    items: Vec<ItemDraft>,
    currency: Currency,
    /// The expense's total as typed; `None` while empty.
    total: Option<Money>,
    /// Everyone a line can be assigned to, for names and colors.
    people: Vec<Person>,
    /// Whether assignments matter (group expense split by items).
    assignable: bool,
    #[props(default)] on_open: Option<EventHandler<u64>>,
    #[props(default)] on_add: Option<EventHandler<()>>,
) -> Element {
    let format = NumberFormat::current();
    let lines = line_items(&items);
    let sum = line_items_sum(&lines).ok();
    let difference = match (total, sum) {
        (Some(total), Some(sum)) => Some(total.amount_minor() - sum),
        _ => None,
    };

    rsx! {
        section { class: "flex flex-col gap-2",
            h2 { class: "px-1 text-sm font-medium text-floral-white-300", {t!("items.title").to_string()} }
            div { class: "flex flex-col drop-shadow-lg",
                div { class: "receipt-edge-top h-2", aria_hidden: "true" }
                div { class: "flex flex-col bg-floral-white-50 px-3 pb-3 font-mono text-jet-black-950",
                    if items.is_empty() {
                        p { class: "px-1 py-6 text-center text-sm text-jet-black-600", {t!("items.empty").to_string()} }
                    }
                    for draft in items.iter().cloned() {
                        ItemRow {
                            key: "{draft.key}",
                            item: draft.item.clone(),
                            currency,
                            people: people.clone(),
                            assignable,
                            onclick: on_open.map(|open| EventHandler::new(move |_: ()| open.call(draft.key))),
                        }
                    }
                    div { class: "mt-2 flex flex-col gap-1 border-t-2 border-dashed border-jet-black-300 px-1 pt-3 text-sm",
                        SumLine {
                            label: t!("items.sum").to_string(),
                            value: sum.map(|s| format_plain(Money::new(s, currency), format)).unwrap_or_default(),
                        }
                        SumLine {
                            label: t!("items.total").to_string(),
                            value: total.map(|t| format_plain(t, format)).unwrap_or_else(|| "–".to_string()),
                            strong: true,
                        }
                    }
                }
                div { class: "receipt-edge-bottom h-2", aria_hidden: "true" }
            }
            match difference {
                Some(0) | None => rsx! {},
                Some(difference) => rsx! {
                    div {
                        class: "flex items-start gap-3 rounded-2xl bg-pale-oak-900 px-4 py-3 text-sm text-pale-oak-200",
                        role: "status",
                        aria_live: "polite",
                        Icon { icon: LdTriangleAlert, class: "mt-0.5 h-5 w-5 shrink-0" }
                        span {
                            {t!("items.difference", amount = format_money(Money::new(difference, currency), format)).to_string()}
                        }
                    }
                },
            }
            if let Some(on_add) = on_add {
                button {
                    class: "flex min-h-11 items-center gap-2 self-start rounded-full px-3 text-sm font-medium text-cerulean-300 active:bg-jet-black-800 transition-colors",
                    r#type: "button",
                    onclick: move |_| on_add.call(()),
                    Icon { icon: LdPlus, class: "h-4 w-4" }
                    {t!("items.add").to_string()}
                }
            }
        }
    }
}

#[component]
fn SumLine(label: String, value: String, #[props(default)] strong: bool) -> Element {
    rsx! {
        div {
            class: "flex items-baseline justify-between gap-3 tabular-nums",
            class: if strong { "text-base font-bold" },
            span { "{label}" }
            span { "{value}" }
        }
    }
}

/// One line of the receipt; tapping opens its sheet, if there is one.
#[component]
fn ItemRow(
    item: LineItem,
    currency: Currency,
    people: Vec<Person>,
    assignable: bool,
    onclick: Option<EventHandler<()>>,
) -> Element {
    let format = NumberFormat::current();
    let text = item.text().to_string();
    let shown_text = if text.trim().is_empty() {
        t!("items.no_text").to_string()
    } else {
        text.clone()
    };
    // The printed text when the line shows something else (OCR-35).
    let original = (item.original_text != text && !item.original_text.is_empty())
        .then(|| item.original_text.clone());
    let quantity_line = (item.quantity != Decimal::ONE || item.unit_price_minor.is_some())
        .then(|| {
            let unit = item
                .unit_price_minor
                .map(|u| format_plain(Money::new(u, currency), format));
            match unit {
                Some(unit) if item.quantity != Decimal::ONE => {
                    format!("{} × {unit}", format_number(item.quantity, format))
                }
                Some(_) => String::new(),
                None => format!("{} ×", format_number(item.quantity, format)),
            }
        })
        .filter(|line| !line.is_empty());
    let ignored = !item.kind.counts();
    let kind_label = (item.kind != LineItemKind::Article).then(|| kind_label(item.kind));
    let holders: Vec<AvatarEntry> = item
        .assigned_to
        .iter()
        .filter(|(_, weight)| !weight.is_zero())
        .filter_map(|(id, _)| people.iter().find(|p| &p.id == id))
        .map(|p| AvatarEntry {
            name: p.name.clone(),
            color: p.color.clone(),
        })
        .collect();
    let show_holders = assignable && item.kind.is_assignable();

    rsx! {
        button {
            class: "flex min-h-14 w-full items-start gap-3 border-b border-dashed border-jet-black-200 px-1 py-2 text-left transition-colors ease-apple",
            class: if onclick.is_some() { "active:bg-floral-white-200" },
            r#type: "button",
            disabled: onclick.is_none(),
            onclick: move |_| {
                if let Some(onclick) = onclick {
                    onclick.call(());
                }
            },
            span { class: "flex min-w-0 flex-1 flex-col",
                span {
                    class: "text-base break-words",
                    class: if ignored { "text-jet-black-500 line-through" },
                    "{shown_text}"
                }
                if let Some(original) = original {
                    span { class: "text-xs break-words text-jet-black-600", "{original}" }
                }
                if quantity_line.is_some() || kind_label.is_some() {
                    span { class: "flex flex-wrap gap-x-3 text-xs text-jet-black-600",
                        if let Some(line) = quantity_line {
                            span { class: "tabular-nums", "{line}" }
                        }
                        if let Some(label) = kind_label {
                            span { class: "uppercase", "{label}" }
                        }
                    }
                }
            }
            span { class: "flex shrink-0 flex-col items-end gap-1",
                span {
                    class: "text-base tabular-nums",
                    class: if ignored { "text-jet-black-500 line-through" },
                    {format_plain(Money::new(item.total_minor, currency), format)}
                }
                if show_holders {
                    if holders.is_empty() {
                        span { class: "text-xs text-jet-black-600", {t!("items.everyone").to_string()} }
                    } else {
                        AvatarStack { people: holders, max: 3 }
                    }
                }
            }
        }
    }
}

/// Edits one line: text, quantity, unit price, total, kind (OCR-30), who
/// had how much of it (OCR-33, weights as in OCR-40) and the actions of
/// OCR-31. Every change goes to `on_change` right away.
#[component]
pub(super) fn ItemSheet(
    draft: ItemDraft,
    currency: Currency,
    members: Vec<GroupMember>,
    /// Whether assignments matter (group expense split by items).
    assignable: bool,
    has_previous: bool,
    has_next: bool,
    error: Option<String>,
    on_change: EventHandler<LineItem>,
    on_action: EventHandler<ItemAction>,
    on_close: EventHandler<()>,
) -> Element {
    let format = NumberFormat::current();
    let item = draft.item.clone();
    let magnitude = move |minor: i64| amount_text(Money::new(minor.abs(), currency), format);
    let mut text = use_signal(|| item.text().to_string());
    let mut quantity = use_signal(|| number_text(item.quantity, format));
    let mut unit = use_signal(|| item.unit_price_minor.map(magnitude).unwrap_or_default());
    let mut total = use_signal(|| magnitude(item.total_minor));

    let id = |field: &str| format!("item-{}-{field}", draft.key);
    let shares = if item.assigned_to.is_empty() {
        BTreeMap::new()
    } else {
        allocate(item.total_minor, &item.assigned_to).unwrap_or_default()
    };
    let weight_sum: Decimal = item.assigned_to.values().sum();
    let printed = item.translated_text.as_ref().unwrap_or(&item.original_text);
    let show_original = !item.original_text.is_empty() && *printed != text();

    let edit = {
        let item = item.clone();
        move |change: &dyn Fn(&mut LineItem)| {
            let mut changed = item.clone();
            change(&mut changed);
            changed.edited_by_user = true;
            on_change.call(changed);
        }
    };

    let on_text = {
        let edit = edit.clone();
        let item = item.clone();
        move |value: String| {
            text.set(value.clone());
            let printed = item
                .translated_text
                .clone()
                .unwrap_or_else(|| item.original_text.clone());
            edit(&|line| {
                line.user_text = (value.trim() != printed.trim()).then(|| value.trim().to_string());
            });
        }
    };

    // Quantity × unit price gives the total when both are known.
    let on_quantity = {
        let edit = edit.clone();
        let item = item.clone();
        move |value: String| {
            quantity.set(value.clone());
            let Some(q) = parse_number(&value, format).filter(|q| *q > Decimal::ZERO) else {
                return;
            };
            let new_total = item.unit_price_minor.and_then(|u| times(q, u, currency));
            if let Some(t) = new_total {
                total.set(magnitude(t));
            }
            edit(&|line| {
                line.quantity = q;
                if let Some(t) = new_total {
                    line.total_minor = t;
                }
            });
        }
    };

    let on_unit = {
        let edit = edit.clone();
        let item = item.clone();
        move |value: String| {
            unit.set(value.clone());
            let price =
                parse_amount(&value, currency, format).map(|m| item.kind.signed(m.amount_minor()));
            let new_total = price.and_then(|u| times(item.quantity, u, currency));
            if let Some(t) = new_total {
                total.set(magnitude(t));
            }
            edit(&|line| {
                line.unit_price_minor = price;
                if let Some(t) = new_total {
                    line.total_minor = t;
                }
            });
        }
    };

    // A total that no longer fits quantity × unit price drops the price.
    let on_total = {
        let edit = edit.clone();
        let item = item.clone();
        move |value: String| {
            total.set(value.clone());
            let minor = parse_amount(&value, currency, format)
                .map_or(0, |m| item.kind.signed(m.amount_minor()));
            let keeps_unit = item
                .unit_price_minor
                .and_then(|u| times(item.quantity, u, currency))
                == Some(minor);
            if !keeps_unit {
                unit.set(String::new());
            }
            edit(&|line| {
                line.total_minor = minor;
                if !keeps_unit {
                    line.unit_price_minor = None;
                }
            });
        }
    };

    let set_kind = {
        let edit = edit.clone();
        move |kind: LineItemKind| {
            edit(&|line| {
                line.kind = kind;
                line.total_minor = kind.signed(line.total_minor);
                line.unit_price_minor = line.unit_price_minor.map(|u| kind.signed(u));
            });
        }
    };

    let set_weight = {
        let edit = edit.clone();
        move |(person, weight): (PersonId, Decimal)| {
            edit(&|line| {
                if weight.is_zero() {
                    line.assigned_to.remove(&person);
                } else {
                    line.assigned_to.insert(person.clone(), weight);
                }
            });
        }
    };
    let everyone = {
        let edit = edit.clone();
        move |_| edit(&|line| line.assigned_to.clear())
    };

    rsx! {
        BottomSheet { title: t!("items.edit_title").to_string(), on_close,
            div { class: "flex max-h-[75vh] flex-col gap-4 overflow-y-auto overscroll-contain px-4 pt-2",
                div { class: "flex flex-col gap-1",
                    TextField {
                        id: id("text"),
                        label: t!("items.text").to_string(),
                        value: text(),
                        oninput: on_text,
                    }
                    if show_original {
                        p { class: "px-1 text-sm break-words text-floral-white-400",
                            {t!("items.original", text = printed).to_string()}
                        }
                    }
                }
                div { class: "flex flex-col gap-2",
                    FieldRow { label: t!("items.quantity").to_string(),
                        CompactNumberInput {
                            id: id("quantity"),
                            label: t!("items.quantity").to_string(),
                            value: quantity(),
                            decimals: QUANTITY_DECIMALS,
                            unit: "×",
                            oninput: on_quantity,
                        }
                    }
                    FieldRow { label: t!("items.unit_price").to_string(),
                        CompactAmountInput {
                            id: id("unit"),
                            label: t!("items.unit_price").to_string(),
                            value: unit(),
                            currency,
                            oninput: on_unit,
                        }
                    }
                    FieldRow { label: t!("items.line_total").to_string(),
                        CompactAmountInput {
                            id: id("total"),
                            label: t!("items.line_total").to_string(),
                            value: total(),
                            currency,
                            oninput: on_total,
                        }
                    }
                }
                div { class: "flex flex-col gap-2",
                    span { class: "text-sm font-medium text-floral-white-300", {t!("items.kind").to_string()} }
                    div { class: "flex flex-wrap gap-2", role: "radiogroup",
                        for kind in LineItemKind::ALL {
                            Chip {
                                key: "{kind.code()}",
                                label: kind_label(kind),
                                selected: item.kind == kind,
                                onclick: {
                                    let set_kind = set_kind.clone();
                                    move |_| set_kind(kind)
                                },
                            }
                        }
                    }
                    p { class: "px-1 text-xs text-floral-white-400", {kind_hint(item.kind)} }
                }
                if assignable && item.kind.is_assignable() {
                    div { class: "flex flex-col gap-2",
                        div { class: "flex items-center justify-between gap-2",
                            span { class: "text-sm font-medium text-floral-white-300", {t!("items.who").to_string()} }
                            Chip {
                                label: t!("items.everyone").to_string(),
                                selected: item.assigned_to.is_empty(),
                                onclick: everyone,
                            }
                        }
                        div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-950",
                            for member in members.iter().cloned() {
                                WeightRow {
                                    key: "{member.person.id.as_str()}",
                                    weight: item.assigned_to.get(&member.person.id).copied().unwrap_or_default(),
                                    share: shares.get(&member.person.id).map(|s| Money::new(*s, currency)),
                                    on_change: {
                                        let set_weight = set_weight.clone();
                                        let person = member.person.id.clone();
                                        move |weight| set_weight((person.clone(), weight))
                                    },
                                    person: member.person,
                                }
                            }
                        }
                        p { class: "px-1 text-xs text-floral-white-400",
                            if item.assigned_to.is_empty() {
                                {t!("items.everyone_hint").to_string()}
                            } else {
                                {t!("items.weights_sum", sum = format_number(weight_sum, format), quantity = format_number(item.quantity, format)).to_string()}
                            }
                        }
                    }
                }
                if let Some(error) = error {
                    p { class: "px-1 text-sm text-watermelon-300", role: "alert", "{error}" }
                }
                div { class: "grid grid-cols-2 gap-2 pb-2",
                    ActionButton {
                        label: t!("items.split").to_string(),
                        onclick: move |_| on_action.call(ItemAction::Split),
                        Icon { icon: LdSplit, class: "h-5 w-5" }
                    }
                    ActionButton {
                        label: t!("items.merge_next").to_string(),
                        disabled: !has_next,
                        onclick: move |_| on_action.call(ItemAction::MergeNext),
                        Icon { icon: LdMerge, class: "h-5 w-5" }
                    }
                    ActionButton {
                        label: t!("items.move_up").to_string(),
                        disabled: !has_previous,
                        onclick: move |_| on_action.call(ItemAction::MoveUp),
                        Icon { icon: LdArrowUp, class: "h-5 w-5" }
                    }
                    ActionButton {
                        label: t!("items.move_down").to_string(),
                        disabled: !has_next,
                        onclick: move |_| on_action.call(ItemAction::MoveDown),
                        Icon { icon: LdArrowDown, class: "h-5 w-5" }
                    }
                    ActionButton {
                        label: t!("items.delete").to_string(),
                        danger: true,
                        onclick: move |_| on_action.call(ItemAction::Delete),
                        Icon { icon: LdTrash2, class: "h-5 w-5" }
                    }
                }
            }
        }
    }
}

/// `quantity × unit` rounded to the currency (idee.md 8.4).
fn times(quantity: Decimal, unit_minor: i64, currency: Currency) -> Option<i64> {
    let unit = Money::new(unit_minor, currency).to_decimal();
    Money::from_decimal(quantity.checked_mul(unit)?, currency)
        .ok()
        .map(|m| m.amount_minor())
}

#[component]
fn FieldRow(label: String, children: Element) -> Element {
    rsx! {
        div { class: "flex min-h-12 items-center justify-between gap-3",
            span { class: "text-base text-floral-white-200", "{label}" }
            {children}
        }
    }
}

/// A person's part of one line: tap minus or plus to change how many of
/// it they had; 0 = not theirs.
#[component]
fn WeightRow(
    person: Person,
    weight: Decimal,
    share: Option<Money>,
    on_change: EventHandler<Decimal>,
) -> Element {
    let format = NumberFormat::current();
    let name = person.name.clone();
    let active = !weight.is_zero();
    rsx! {
        div { class: "flex min-h-14 items-center gap-3 border-b border-jet-black-800 px-3 py-1 last:border-b-0",
            Avatar { name: name.clone(), color: person.color.clone(), size: AvatarSize::Sm }
            span { class: "flex min-w-0 flex-1 flex-col",
                span {
                    class: "truncate text-base",
                    class: if active { "text-floral-white-50" } else { "text-floral-white-300" },
                    "{name}"
                }
                if let Some(share) = share.filter(|_| active) {
                    span { class: "text-sm tabular-nums text-floral-white-400", {format_money(share, format)} }
                }
            }
            button {
                class: "flex h-11 w-11 items-center justify-center rounded-full bg-jet-black-800 text-floral-white-200 active:bg-jet-black-700 disabled:opacity-40 transition-colors",
                r#type: "button",
                disabled: !active,
                aria_label: t!("items.less", name = name).to_string(),
                onclick: move |_| on_change.call((weight - Decimal::ONE).max(Decimal::ZERO)),
                Icon { icon: LdMinus, class: "h-5 w-5" }
            }
            span {
                class: "w-8 text-center text-base font-semibold tabular-nums",
                class: if active { "text-floral-white-50" } else { "text-floral-white-500" },
                aria_live: "polite",
                {format_number(weight, format)}
            }
            button {
                class: "flex h-11 w-11 items-center justify-center rounded-full bg-jet-black-800 text-floral-white-200 active:bg-jet-black-700 transition-colors",
                r#type: "button",
                aria_label: t!("items.more", name = name).to_string(),
                onclick: move |_| on_change.call(weight.floor() + Decimal::ONE),
                Icon { icon: LdPlus, class: "h-5 w-5" }
            }
        }
    }
}

#[component]
fn ActionButton(
    label: String,
    onclick: EventHandler<()>,
    #[props(default)] disabled: bool,
    #[props(default)] danger: bool,
    children: Element,
) -> Element {
    let colors = if danger {
        "text-watermelon-300"
    } else {
        "text-floral-white-100"
    };
    rsx! {
        button {
            class: "flex min-h-12 items-center justify-center gap-2 rounded-2xl bg-jet-black-800 px-3 text-sm font-medium active:bg-jet-black-700 disabled:opacity-40 transition-colors ease-apple {colors}",
            r#type: "button",
            disabled,
            onclick: move |_| onclick.call(()),
            {children}
            "{label}"
        }
    }
}

pub(super) fn kind_label(kind: LineItemKind) -> String {
    match kind {
        LineItemKind::Article => t!("items.kind_article"),
        LineItemKind::Discount => t!("items.kind_discount"),
        LineItemKind::Deposit => t!("items.kind_deposit"),
        LineItemKind::Tax => t!("items.kind_tax"),
        LineItemKind::Tip => t!("items.kind_tip"),
        LineItemKind::ServiceCharge => t!("items.kind_service"),
        LineItemKind::Ignored => t!("items.kind_ignored"),
    }
    .to_string()
}

fn kind_hint(kind: LineItemKind) -> String {
    if kind == LineItemKind::Discount {
        t!("items.hint_discount")
    } else if kind.is_assignable() {
        t!("items.hint_assignable")
    } else if kind.counts() {
        t!("items.hint_proportional")
    } else {
        t!("items.hint_ignored")
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    const DE: NumberFormat = NumberFormat {
        decimal: ',',
        group: '.',
        symbol_before: false,
    };

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn d(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn draft(text: &str, quantity: &str, total: i64) -> ItemDraft {
        ItemDraft::new(LineItem {
            original_text: text.into(),
            quantity: d(quantity),
            total_minor: total,
            ..LineItem::default()
        })
    }

    fn texts(drafts: &[ItemDraft]) -> Vec<String> {
        drafts.iter().map(|d| d.item.text().to_string()).collect()
    }

    #[test]
    fn actions_change_the_list() {
        let mut list = vec![
            draft("Bier", "3", 1_350),
            draft("Brot", "1", 199),
            draft("Pfand", "1", 25),
        ];
        let (beer, bread) = (list[0].key, list[1].key);

        assert!(!apply(&mut list, beer, ItemAction::Split).unwrap());
        assert_eq!(texts(&list), ["Bier", "Bier", "Brot", "Pfand"]);
        assert_eq!(
            (list[0].item.total_minor, list[1].item.total_minor),
            (900, 450)
        );
        assert_eq!(list[0].key, beer);

        assert!(apply(&mut list, bread, ItemAction::MoveUp).unwrap());
        assert_eq!(texts(&list), ["Bier", "Brot", "Bier", "Pfand"]);
        assert!(apply(&mut list, bread, ItemAction::MoveDown).unwrap());
        assert!(apply(&mut list, bread, ItemAction::MoveDown).unwrap());
        assert_eq!(texts(&list), ["Bier", "Bier", "Pfand", "Brot"]);
        // The last line cannot go further down.
        assert!(apply(&mut list, bread, ItemAction::MoveDown).unwrap());
        assert_eq!(list[3].key, bread);

        let deposit = list[2].key;
        let second_beer = list[1].key;
        apply(&mut list, second_beer, ItemAction::MergeNext).unwrap();
        assert_eq!(texts(&list), ["Bier", "Bier + Pfand", "Brot"]);
        assert_eq!(list[1].item.total_minor, 475);
        assert!(list.iter().all(|d| d.key != deposit));

        apply(&mut list, bread, ItemAction::Delete).unwrap();
        // Merging the last line does nothing.
        let last = list[1].key;
        apply(&mut list, last, ItemAction::MergeNext).unwrap();
        assert_eq!(texts(&list), ["Bier", "Bier + Pfand"]);
        assert_eq!(line_items_sum(&line_items(&list)), Ok(1_375));
    }

    #[test]
    fn switching_currency_fits_the_amounts() {
        let mut list = vec![draft("Ramen", "1", 1_250)];
        list[0].item.unit_price_minor = Some(1_250);
        let mut discount = draft("Coupon", "1", -199);
        discount.item.kind = LineItemKind::Discount;
        list.push(discount);
        fit_currency(&mut list, cur("EUR"), cur("JPY"), DE);
        assert_eq!(list[0].item.total_minor, 12);
        assert_eq!(list[0].item.unit_price_minor, Some(12));
        assert_eq!(list[1].item.total_minor, -1);
    }

    #[test]
    fn quantity_times_unit_price_rounds_half_even() {
        let eur = cur("EUR");
        assert_eq!(times(d("2"), 199, eur), Some(398));
        // 0.452 kg × 3.99 €/kg = 1.80348 €.
        assert_eq!(times(d("0.452"), 399, eur), Some(180));
        assert_eq!(times(d("0.5"), 5, eur), Some(2));
    }
}
