//! The line items of an expense (idee.md 7.2 step 5, OCR-30..35, SPL-02):
//! a list that looks like the printed receipt, and a sheet to correct one
//! line, split or merge it and say who had how much of it.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdArrowDown, LdArrowUp, LdCornerDownRight, LdDownload, LdLanguages, LdMerge, LdMinus,
        LdPlus, LdSplit, LdTrash2, LdTriangleAlert,
    },
};
use invuso_core::Decimal;
use invuso_core::domain::{
    Currency, GroupMember, LineItem, LineItemError, LineItemKind, Money, Person, PersonId,
    effective_assignments, line_items_sum,
};
use invuso_core::receipt::{ItemKind, ParsedReceipt, VatLine};
use invuso_core::split::allocate;

use crate::Route;
use crate::components::{
    Avatar, AvatarEntry, AvatarSize, AvatarStack, BottomSheet, Chip, CompactAmountInput,
    CompactNumberInput, SwitchRow, TextField,
};
use crate::format::{
    NumberFormat, amount_text, fit_amount_text, format_money, format_number, format_plain,
    number_text, parse_amount, parse_number,
};
use crate::preferences::kind_label;
use crate::services::ocr::{ItemMark, PhotoQuad, bounds_of, is_unsure};

/// Decimals a quantity can be typed with (weights in kg: `0,452`).
const QUANTITY_DECIMALS: u32 = 3;

static NEXT_KEY: AtomicU64 = AtomicU64::new(1);

/// A line while the form is open, with a key that stays the same when
/// lines are moved, split or merged.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ItemDraft {
    pub key: u64,
    pub item: LineItem,
    /// Where the line was printed in the receipt photo (OCR-37); empty
    /// for lines typed by hand or saved before.
    pub marks: Vec<PhotoQuad>,
}

impl ItemDraft {
    pub(super) fn new(item: LineItem) -> Self {
        Self {
            key: NEXT_KEY.fetch_add(1, Ordering::Relaxed),
            item,
            marks: Vec::new(),
        }
    }

    /// Marked as possibly misread until the user changes it (OCR-18).
    pub(super) fn unsure(&self) -> bool {
        unsure(&self.item)
    }
}

fn unsure(item: &LineItem) -> bool {
    is_unsure(item.ocr_confidence) && !item.edited_by_user
}

/// The lines the parser read (OCR-10..13), ready to correct, with how sure
/// the engine was of each and where it was printed (`marks`, one per item
/// of `receipt`; OCR-18, OCR-37).
pub(super) fn drafts_from_parsed(receipt: &ParsedReceipt, marks: &[ItemMark]) -> Vec<ItemDraft> {
    receipt
        .items
        .iter()
        .enumerate()
        .map(|(index, parsed)| {
            let mark = marks.get(index).cloned().unwrap_or_default();
            let kind = match parsed.kind {
                ItemKind::Article => LineItemKind::Article,
                ItemKind::Discount => LineItemKind::Discount,
                ItemKind::Deposit => LineItemKind::Deposit,
                ItemKind::Tax => LineItemKind::Tax,
            };
            ItemDraft {
                marks: mark.quads,
                ..ItemDraft::new(LineItem {
                    original_text: parsed.text.clone(),
                    quantity: parsed.quantity,
                    unit_price_minor: parsed.unit_price.map(|m| m.amount_minor()),
                    total_minor: kind.signed(parsed.total_price.amount_minor()),
                    kind,
                    attached: parsed.attached,
                    ocr_confidence: mark.confidence,
                    ..LineItem::default()
                })
            }
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
            // Both halves were printed in the same place.
            let part = ItemDraft {
                marks: drafts[index].marks.clone(),
                ..ItemDraft::new(part)
            };
            drafts.insert(index + 1, part);
            Ok(false)
        }
        ItemAction::MergeNext => {
            if let Some(next) = drafts.get(index + 1) {
                let merged = drafts[index].item.merge(&next.item)?;
                let marks = next.marks.clone();
                drafts[index].item = merged;
                drafts[index].marks.extend(marks);
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

/// A line about the translation of the lines above the list (TRL-01).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TranslationNote {
    pub text: String,
    /// Drawn as a hint (no engine, failure) rather than as information.
    pub warning: bool,
    /// Offers the way to the translation packs (SET-07).
    pub packs_link: bool,
}

/// The lines in the look of the printed receipt (user wish in AP-19):
/// correction or translation large, the printed text small underneath,
/// swapped by a switch (OCR-35); `quantity × unit price`, the total and
/// who had it (OCR-33).
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
    #[props(default)] translation: Option<TranslationNote>,
    /// VAT the receipt says is contained in its prices (AP-38).
    #[props(default)]
    vat: Vec<VatLine>,
) -> Element {
    let format = NumberFormat::current();
    let mut show_original = use_signal(|| false);
    let lines = line_items(&items);
    // Who carries each line, attached ones as their article.
    let carried = effective_assignments(&lines);
    // The switch only makes sense once some line reads differently.
    let has_other_text = lines
        .iter()
        .any(|line| !line.original_text.is_empty() && line.text() != line.original_text);
    let original_first = show_original() && has_other_text;
    let unsure_count = items.iter().filter(|d| d.unsure()).count();
    let sum = line_items_sum(&lines).ok();
    let difference = match (total, sum) {
        (Some(total), Some(sum)) => Some(total.amount_minor() - sum),
        _ => None,
    };

    rsx! {
        section { class: "flex flex-col gap-2",
            div { class: "flex min-h-11 items-center justify-between gap-3 px-1",
                h2 { class: "text-sm font-medium text-floral-white-300", {t!("items.title").to_string()} }
                if has_other_text {
                    button {
                        class: "flex min-h-11 items-center gap-2 rounded-full px-3 text-sm font-medium text-cerulean-300 active:bg-jet-black-800 transition-colors",
                        r#type: "button",
                        aria_pressed: if original_first { "true" } else { "false" },
                        onclick: move |_| show_original.toggle(),
                        Icon { icon: LdLanguages, class: "h-4 w-4" }
                        if original_first {
                            {t!("items.show_translation").to_string()}
                        } else {
                            {t!("items.show_original").to_string()}
                        }
                    }
                }
            }
            if let Some(note) = translation {
                p {
                    class: "px-1 text-sm",
                    class: if note.warning { "text-pale-oak-200" } else { "text-floral-white-400" },
                    role: "status",
                    aria_live: "polite",
                    "{note.text}"
                }
                if note.packs_link {
                    Link {
                        class: "flex min-h-11 items-center gap-2 self-start rounded-full px-3 text-sm font-medium text-cerulean-300 active:bg-jet-black-800 transition-colors",
                        to: Route::SettingsLanguages {},
                        Icon { icon: LdDownload, class: "h-4 w-4" }
                        {t!("items.open_packs").to_string()}
                    }
                }
            }
            if unsure_count > 0 {
                div { class: "flex items-start gap-3 px-1 text-sm text-pale-oak-200", role: "status",
                    Icon { icon: LdTriangleAlert, class: "mt-0.5 h-4 w-4 shrink-0 text-pale-oak-300" }
                    span {
                        if unsure_count == 1 {
                            {t!("items.unsure_one").to_string()}
                        } else {
                            {t!("items.unsure_other", count = unsure_count).to_string()}
                        }
                    }
                }
            }
            div { class: "flex flex-col drop-shadow-lg",
                div { class: "receipt-edge-top h-2", aria_hidden: "true" }
                div { class: "flex flex-col bg-floral-white-50 px-3 pb-3 font-mono text-jet-black-950",
                    if items.is_empty() {
                        p { class: "px-1 py-6 text-center text-sm text-jet-black-600", {t!("items.empty").to_string()} }
                    }
                    for (index, draft) in items.iter().cloned().enumerate() {
                        ItemRow {
                            key: "{draft.key}",
                            attached: draft.item.attached && index > 0,
                            unsure: draft.unsure(),
                            carried_by: carried.get(index).cloned().unwrap_or_default(),
                            item: draft.item.clone(),
                            currency,
                            people: people.clone(),
                            assignable,
                            original_first,
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
                        for line in vat.iter() {
                            div { class: "flex items-baseline justify-between gap-3 text-xs tabular-nums text-jet-black-600",
                                span {
                                    match line.rate {
                                        Some(rate) => t!("items.vat_rate", rate = format_number(rate, format)).to_string(),
                                        None => t!("items.vat").to_string(),
                                    }
                                }
                                span { {format_plain(line.amount, format)} }
                            }
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
    /// Belongs to the line above (a deposit, a discount).
    attached: bool,
    /// Possibly misread (OCR-18).
    unsure: bool,
    /// Who carries the line; for an attached line, who carries its article.
    carried_by: BTreeMap<PersonId, Decimal>,
    currency: Currency,
    people: Vec<Person>,
    assignable: bool,
    /// The printed text large and the translation small (OCR-35).
    original_first: bool,
    onclick: Option<EventHandler<()>>,
) -> Element {
    let format = NumberFormat::current();
    let text = item.text().to_string();
    // The other text when the line reads differently from the print.
    let other = (item.original_text != text && !item.original_text.is_empty())
        .then(|| item.original_text.clone());
    let (main, original) = match other {
        Some(printed) if original_first => (printed, Some(text)),
        other => (text, other),
    };
    let shown_text = if main.trim().is_empty() {
        t!("items.no_text").to_string()
    } else {
        main
    };
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
    let holders: Vec<AvatarEntry> = carried_by
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
            class: if unsure { "bg-pale-oak-100" },
            class: if onclick.is_some() { "active:bg-floral-white-200" },
            r#type: "button",
            disabled: onclick.is_none(),
            onclick: move |_| {
                if let Some(onclick) = onclick {
                    onclick.call(());
                }
            },
            if attached {
                Icon { icon: LdCornerDownRight, class: "mt-1 h-4 w-4 shrink-0 text-jet-black-500" }
            }
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
                span { class: "flex items-center gap-1",
                    if unsure {
                        Icon { icon: LdTriangleAlert, class: "h-4 w-4 shrink-0 text-pale-oak-700" }
                        span { class: "sr-only", {t!("items.unsure").to_string()} }
                    }
                    span {
                        class: "text-base tabular-nums",
                        class: if ignored { "text-jet-black-500 line-through" },
                        {format_plain(Money::new(item.total_minor, currency), format)}
                    }
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
    /// The receipt photo, to show where the line was printed (OCR-37).
    #[props(default)]
    photo: Option<ReceiptPhoto>,
    /// Opens the photo in full screen on this line.
    #[props(default)]
    on_show_photo: Option<EventHandler<()>>,
) -> Element {
    let format = NumberFormat::current();
    let item = draft.item.clone();
    let unsure = draft.unsure();
    let snippet = photo.and_then(|photo| {
        let region = snippet_region(&draft.marks, photo.size)?;
        Some((photo, region))
    });
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

    // Correcting what was read; this also clears the unsure mark (OCR-18).
    let edit = {
        let item = item.clone();
        move |change: &dyn Fn(&mut LineItem)| {
            let mut changed = item.clone();
            change(&mut changed);
            changed.edited_by_user = true;
            on_change.call(changed);
        }
    };
    // Saying who had the line or what it belongs to corrects nothing that
    // was read, so an unsure line stays marked.
    let assign = {
        let item = item.clone();
        move |change: &dyn Fn(&mut LineItem)| {
            let mut changed = item.clone();
            change(&mut changed);
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
        let assign = assign.clone();
        move |(person, weight): (PersonId, Decimal)| {
            assign(&|line| {
                if weight.is_zero() {
                    line.assigned_to.remove(&person);
                } else {
                    line.assigned_to.insert(person.clone(), weight);
                }
            });
        }
    };
    let everyone = {
        let assign = assign.clone();
        move |_| assign(&|line| line.assigned_to.clear())
    };
    let set_attached = move |attached: bool| assign(&|line| line.attached = attached);
    let attached = item.attached && has_previous;

    rsx! {
        BottomSheet { title: t!("items.edit_title").to_string(), on_close,
            div { class: "flex max-h-[75vh] flex-col gap-4 overflow-y-auto overscroll-contain px-4 pt-2",
                if let Some((photo, region)) = snippet {
                    PhotoSnippet {
                        photo,
                        region,
                        marks: draft.marks.clone(),
                        unsure,
                        onclick: on_show_photo,
                    }
                }
                if unsure {
                    div { class: "flex items-start gap-3 rounded-2xl bg-pale-oak-900 px-4 py-3 text-sm text-pale-oak-200",
                        role: "status",
                        Icon { icon: LdTriangleAlert, class: "mt-0.5 h-5 w-5 shrink-0" }
                        span { {t!("items.unsure_hint").to_string()} }
                    }
                }
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
                if has_previous {
                    div { class: "overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-950",
                        SwitchRow {
                            label: t!("items.attached").to_string(),
                            hint: t!("items.attached_hint").to_string(),
                            checked: attached,
                            onchange: set_attached,
                        }
                    }
                }
                if assignable && item.kind.is_assignable() && !attached {
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

/// The photo the lines were read from (OCR-37).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ReceiptPhoto {
    pub src: String,
    /// Size of the image the boxes refer to.
    pub size: (u32, u32),
}

/// Part of the photo shown around a line: `x, y, width, height` in pixels
/// of an image of `size`. A little of the rows above and below stays
/// visible, and the part is at least [`SNIPPET_ASPECT`] times as wide as
/// high where the photo allows, so a short word is not blown up.
fn snippet_region(marks: &[PhotoQuad], size: (u32, u32)) -> Option<(f32, f32, f32, f32)> {
    let (left, top, right, bottom) = bounds_of(marks)?;
    let (width, height) = (size.0 as f32, size.1 as f32);
    let row = marks
        .iter()
        .map(|q| {
            let (_, t, _, b) = q.bounds();
            b - t
        })
        .fold(f32::MAX, f32::min);
    let (pad_x, pad_y) = (width * 0.03, row * 1.2);
    let (mut left, mut right) = ((left - pad_x).max(0.0), (right + pad_x).min(width));
    let (top, bottom) = ((top - pad_y).max(0.0), (bottom + pad_y).min(height));
    let wanted = (bottom - top) * SNIPPET_ASPECT;
    if right - left < wanted {
        let middle = (left + right) / 2.0;
        let half = (wanted / 2.0).min(width / 2.0);
        let start = (middle - half).clamp(0.0, width - 2.0 * half);
        (left, right) = (start, start + 2.0 * half);
    }
    (right > left && bottom > top).then_some((left, top, right - left, bottom - top))
}

/// Narrowest the photo part of a line gets, as width per height.
const SNIPPET_ASPECT: f32 = 3.0;

/// The part of the photo around one line with the line outlined; tapping
/// it enlarges the photo there.
#[component]
fn PhotoSnippet(
    photo: ReceiptPhoto,
    region: (f32, f32, f32, f32),
    marks: Vec<PhotoQuad>,
    unsure: bool,
    onclick: Option<EventHandler<()>>,
) -> Element {
    let (x, y, w, h) = region;
    let (width, height) = (photo.size.0 as f32, photo.size.1 as f32);
    // The whole photo, scaled and moved so the region fills the frame.
    let placement = format!(
        "left: {}%; top: {}%; width: {}%; height: {}%;",
        -x / w * 100.0,
        -y / h * 100.0,
        width / w * 100.0,
        height / h * 100.0
    );
    let outline = if unsure {
        "fill-pale-oak-400/20 stroke-pale-oak-600"
    } else {
        "fill-cerulean-400/15 stroke-cerulean-400"
    };
    rsx! {
        button {
            class: "relative block w-full shrink-0 overflow-hidden rounded-xl border border-jet-black-800 bg-jet-black-950 active:opacity-80 transition-opacity ease-apple",
            style: "aspect-ratio: {w} / {h};",
            r#type: "button",
            aria_label: t!("items.photo").to_string(),
            disabled: onclick.is_none(),
            onclick: move |_| {
                if let Some(onclick) = onclick {
                    onclick.call(());
                }
            },
            div { class: "absolute", style: "{placement}",
                img {
                    class: "absolute inset-0 h-full w-full max-w-none",
                    src: "{photo.src}",
                    alt: "",
                    draggable: "false",
                }
                svg {
                    class: "absolute inset-0 h-full w-full",
                    view_box: "0 0 {photo.size.0} {photo.size.1}",
                    preserve_aspect_ratio: "none",
                    for (index, quad) in marks.iter().enumerate() {
                        polygon {
                            key: "{index}",
                            class: outline,
                            stroke_width: "2",
                            vector_effect: "non-scaling-stroke",
                            points: quad.svg_points(),
                        }
                    }
                }
            }
        }
    }
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
    fn read_lines_carry_confidence_and_place() {
        use invuso_core::receipt::{BoundingBox, parse_receipt};

        use crate::services::ocr::item_marks;
        use crate::storage::{OcrFragment, ReceiptText};

        let fragment = |text: &str, left, top, confidence| OcrFragment {
            text: text.to_string(),
            bbox: BoundingBox {
                left,
                top,
                right: left + 80,
                bottom: top + 20,
            },
            confidence,
        };
        let text = ReceiptText::new(
            "test",
            vec![
                fragment("Milch", 10, 100, 0.99),
                fragment("1,99", 300, 100, 0.62),
                fragment("Brot", 10, 140, 0.97),
                fragment("2,49", 300, 140, 0.96),
                fragment("SUMME 4,48", 10, 190, 0.99),
            ],
            0.0,
        )
        .with_image_size(400, 300);
        let parsed = parse_receipt(&text.recognized(), cur("EUR")).unwrap();
        let mut list = drafts_from_parsed(&parsed, &item_marks(&text, &parsed));
        assert_eq!(texts(&list), ["Milch", "Brot"]);
        // Stored with the line (`line_item.ocr_confidence`).
        let lines = line_items(&list);
        assert_eq!(lines[0].ocr_confidence, Some(0.62));
        assert_eq!(lines[1].ocr_confidence, Some(0.96));
        assert!(list[0].unsure());
        assert!(!list[1].unsure());
        assert_eq!(list[0].marks.len(), 2);
        assert_eq!(
            snippet_region(&list[0].marks, (400, 300)).map(|(_, _, w, h)| w >= h * 3.0),
            Some(true)
        );

        // Halves keep the place; a merged line has both.
        let milk = list[0].key;
        let mut halves = list.clone();
        apply(&mut halves, milk, ItemAction::Split).unwrap();
        assert_eq!(halves[0].marks, halves[1].marks);
        apply(&mut list, milk, ItemAction::MergeNext).unwrap();
        assert_eq!(list[0].marks.len(), 4);
        assert_eq!(list[0].item.ocr_confidence, Some(0.62));

        // A corrected line is no longer marked; one typed by hand never is.
        let mut corrected = list[0].clone();
        assert!(!corrected.unsure(), "merging is a correction");
        corrected.item.edited_by_user = false;
        assert!(corrected.unsure());
        assert!(!draft("Wasser", "1", 99).unsure());
        assert_eq!(snippet_region(&[], (400, 300)), None);
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
