//! Row classification (OCR-11..13) and the total check (OCR-14).
//!
//! A receipt is read in three zones:
//! 1. **Items** up to the first sum row: every row ending in a price is an
//!    item; quantity rows (`2 X 0,49`) attach to the item above or below.
//! 2. **Sums** from the first sum row to the first payment row. The last
//!    `Summe`/`Gesamt` is the total, earlier sum rows are subtotals. Priced
//!    rows in between (`MwSt.-Senkung -0,88`) become items only if they
//!    explain the step from one sum to the next.
//! 3. **After** the first payment row: payment, change, VAT table and
//!    footer; nothing there is an item.

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::rows::group_rows;
use super::tokens::{
    is_times, is_trailing_mark, is_unit_word, keyword_form, line_total, parse_count, parse_price,
    parse_quantity, quantity_after_times, quantity_before_times, quantity_fits,
};
use super::{
    ItemKind, ParsedItem, ParsedReceipt, ReceiptError, ReceiptRow, RecognizedText, RowKind,
    TotalCheck,
};
use crate::domain::{Currency, Money};

/// Parses OCR output of one receipt whose amounts are in `currency`
/// (OCR-10..14). Deterministic and independent of the OCR engine.
pub fn parse_receipt(
    fragments: &[RecognizedText],
    currency: Currency,
) -> Result<ParsedReceipt, ReceiptError> {
    let rows = group_rows(fragments);
    let tokens: Vec<Vec<&str>> = rows
        .iter()
        .map(|row| row.text.split_whitespace().collect())
        .collect();
    let cx = Context::new(currency, &tokens);

    let mut reader = Reader::new(currency, rows.len());
    for (index, row_tokens) in tokens.iter().enumerate() {
        reader.read(index, analyze(row_tokens, &cx))?;
    }
    reader.close_items();
    reader.close_sums()?;

    let rows = rows
        .into_iter()
        .zip(&reader.kinds)
        .map(|(row, kind)| ReceiptRow {
            text: row.text,
            bbox: row.bbox,
            kind: *kind,
        })
        .collect();
    let money = |minor: Option<i64>| minor.map(|m| Money::new(m, currency));
    let mut receipt = ParsedReceipt {
        currency,
        rows,
        items: reader.items,
        subtotal: money(reader.subtotal),
        total: money(reader.total),
        tendered: money(reader.tendered),
        change: money(reader.change),
        check: TotalCheck::NoTotal,
    };
    receipt.check = match receipt.total {
        None => TotalCheck::NoTotal,
        Some(total) => {
            let items_sum = receipt.items_sum()?;
            let difference = total
                .checked_sub(items_sum)
                .map_err(|_| ReceiptError::Overflow)?;
            if difference.is_zero() {
                TotalCheck::Matches
            } else {
                TotalCheck::Differs {
                    items_sum,
                    difference,
                }
            }
        }
    };
    Ok(receipt)
}

struct Context {
    currency: Currency,
    /// False when the till prints a minus after every amount (IKEA).
    trailing_minus_negates: bool,
}

impl Context {
    fn new(currency: Currency, rows: &[Vec<&str>]) -> Self {
        let prices: Vec<_> = rows
            .iter()
            .flatten()
            .filter_map(|token| parse_price(token, currency))
            .collect();
        let with_minus = prices.iter().filter(|p| p.trailing_minus).count();
        Self {
            currency,
            trailing_minus_negates: prices.len() < 2 || with_minus * 2 <= prices.len(),
        }
    }

    fn price(&self, token: &str) -> Option<i64> {
        parse_price(token, self.currency).map(|price| {
            if price.trailing_minus && self.trailing_minus_negates {
                -price.minor
            } else {
                price.minor
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Keyword {
    Total,
    Subtotal,
    Payment,
    Change,
}

/// Leading words, compared in [`keyword_form`].
const KEYWORDS: &[(&[&str], Keyword)] = &[
    (&["zwischensumme"], Keyword::Subtotal),
    (&["zwsumme"], Keyword::Subtotal),
    (&["zw", "summe"], Keyword::Subtotal),
    (&["subtotal"], Keyword::Subtotal),
    (&["sub", "total"], Keyword::Subtotal),
    (&["summe"], Keyword::Total),
    (&["gesamt"], Keyword::Total),
    (&["gesamtsumme"], Keyword::Total),
    (&["gesamtbetrag"], Keyword::Total),
    (&["endsumme"], Keyword::Total),
    (&["endbetrag"], Keyword::Total),
    (&["zahlbetrag"], Keyword::Total),
    (&["zu", "zahlen"], Keyword::Total),
    (&["betrag"], Keyword::Total),
    (&["total"], Keyword::Total),
    (&["bar"], Keyword::Payment),
    (&["barzahlung"], Keyword::Payment),
    (&["bargeld"], Keyword::Payment),
    (&["gegeben"], Keyword::Payment),
    (&["karte"], Keyword::Payment),
    (&["kartenzahlung"], Keyword::Payment),
    (&["ec"], Keyword::Payment),
    (&["eckarte"], Keyword::Payment),
    (&["girocard"], Keyword::Payment),
    (&["kreditkarte"], Keyword::Payment),
    (&["visa"], Keyword::Payment),
    (&["mastercard"], Keyword::Payment),
    (&["maestro"], Keyword::Payment),
    (&["paypal"], Keyword::Payment),
    (&["cash"], Keyword::Payment),
    (&["rückgeld"], Keyword::Change),
    (&["ruckgeld"], Keyword::Change),
    (&["wechselgeld"], Keyword::Change),
    (&["zurück"], Keyword::Change),
    (&["change"], Keyword::Change),
];

/// First words of VAT rows without a percentage (`enth. MwSt 1,59`).
const TAX_WORDS: &[&str] = &[
    "mwst",
    "ust",
    "mehrwertsteuer",
    "steuer",
    "enth",
    "enthaltene",
    "inkl",
    "vat",
    "tax",
];

fn keyword(tokens: &[&str]) -> Option<Keyword> {
    let words: Vec<String> = tokens
        .iter()
        .map(|token| keyword_form(token))
        .filter(|word| !word.is_empty())
        .take(2)
        .collect();
    KEYWORDS
        .iter()
        .find(|(phrase, _)| {
            phrase.len() <= words.len() && phrase.iter().zip(&words).all(|(p, w)| p == w)
        })
        .map(|(_, keyword)| *keyword)
}

/// What one row says, before the zone decides what it means.
enum Line {
    /// No price at the end of the row.
    Plain,
    Keyword {
        keyword: Keyword,
        prices: Vec<i64>,
    },
    Tax,
    Item(Draft),
    /// Quantity and unit price without text, e.g. `2 X 0,49`.
    Quantity(Draft),
}

#[derive(Debug, Clone)]
struct Draft {
    text: String,
    /// Only when printed; items without it count 1.
    quantity: Option<Decimal>,
    unit: Option<i64>,
    total: i64,
}

fn analyze(tokens: &[&str], cx: &Context) -> Line {
    let prices: Vec<i64> = tokens.iter().filter_map(|token| cx.price(token)).collect();
    if let Some(keyword) = keyword(tokens) {
        return Line::Keyword { keyword, prices };
    }
    let Some(&first_price) = prices.first() else {
        return Line::Plain;
    };
    let has_percent = tokens.iter().any(|token| token.contains('%'));
    let starts_with_tax_word = tokens
        .first()
        .is_some_and(|token| TAX_WORDS.contains(&keyword_form(token).as_str()));
    // `Rabatt 20% -1,00` is a discount; `A 7 % 0,53 7,51 8,04` a VAT row.
    if (has_percent && (prices.len() >= 2 || first_price >= 0)) || starts_with_tax_word {
        return Line::Tax;
    }
    item_or_quantity(tokens, cx)
}

/// How a printed quantity relates to the prices after it.
#[derive(Debug, Clone, Copy)]
enum Marker {
    /// `2 x 1,99`: the next price is the unit price.
    PerUnit(Decimal),
    /// `x2 3,98`, `2x Cola 3,98`: the price is the line total.
    Count(Decimal),
}

fn item_or_quantity(tokens: &[&str], cx: &Context) -> Line {
    let mut end = tokens.len();
    while end > 0 && is_trailing_mark(tokens[end - 1], cx.currency) {
        end -= 1;
    }
    let mut prices = Vec::with_capacity(2);
    while end > 0 && prices.len() < 2 {
        let Some(price) = cx.price(tokens[end - 1]) else {
            break;
        };
        prices.insert(0, price);
        end -= 1;
    }
    if prices.is_empty() {
        return Line::Plain;
    }

    let mut head: Vec<&str> = tokens[..end].to_vec();
    let marker = take_quantity_suffix(&mut head).or_else(|| take_quantity_prefix(&mut head));
    let resolved = match (marker, prices.as_slice()) {
        (Some(Marker::PerUnit(q)), &[unit]) => {
            line_total(q, unit).map(|total| (Some(q), Some(unit), total))
        }
        (Some(Marker::PerUnit(q) | Marker::Count(q)), &[unit, total]) => {
            Some((Some(q), Some(unit), total))
        }
        (Some(Marker::Count(q)), &[total]) => Some((Some(q), exact_unit(total, q), total)),
        (None, &[total]) => Some((None, None, total)),
        (None, &[first, total]) => Some(two_prices(&mut head, tokens[end], first, total)),
        _ => None,
    };
    let Some((quantity, unit, total)) = resolved else {
        return Line::Plain;
    };

    let draft = Draft {
        text: head.join(" "),
        quantity,
        unit,
        total,
    };
    if draft.text.is_empty() && marker.is_some() {
        Line::Quantity(draft)
    } else {
        Line::Item(draft)
    }
}

/// `… 2 x`, `… 0,452 kg x`, `… 2x` before the unit price; `… x2` before the
/// line total.
fn take_quantity_suffix(head: &mut Vec<&str>) -> Option<Marker> {
    let last = *head.last()?;
    if is_times(last) && head.len() >= 2 {
        let mut at = head.len() - 2;
        if at >= 1 && is_unit_word(head[at]) {
            at -= 1;
        }
        let quantity = parse_quantity(head[at])?;
        head.truncate(at);
        return Some(Marker::PerUnit(quantity));
    }
    if let Some(quantity) = quantity_before_times(last) {
        head.pop();
        return Some(Marker::PerUnit(quantity));
    }
    if let Some(quantity) = quantity_after_times(last) {
        head.pop();
        return Some(Marker::Count(quantity));
    }
    None
}

/// `2 x Cola …` or `2x Cola …`: a count in front of the text.
fn take_quantity_prefix(head: &mut Vec<&str>) -> Option<Marker> {
    if head.len() >= 3
        && is_times(head[1])
        && let Some(count) = parse_count(head[0])
    {
        head.drain(..2);
        return Some(Marker::Count(count));
    }
    if head.len() >= 2
        && let Some(count) = quantity_before_times(head[0]).filter(|q| q.fract().is_zero())
    {
        head.remove(0);
        return Some(Marker::Count(count));
    }
    None
}

/// Two prices without a quantity sign: `1 T-RINDERSTEAK 19.90 19.90` is
/// count, unit price, total. If the prices do not fit together, the first
/// one stays part of the text.
fn two_prices<'a>(
    head: &mut Vec<&'a str>,
    first_token: &'a str,
    unit: i64,
    total: i64,
) -> (Option<Decimal>, Option<i64>, i64) {
    if head.len() >= 2
        && let Some(count) = parse_count(head[0])
        && quantity_fits(count, unit, total)
    {
        head.remove(0);
        return (Some(count), Some(unit), total);
    }
    if unit != 0 && total % unit == 0 && total / unit > 0 {
        return (Some(Decimal::from(total / unit)), Some(unit), total);
    }
    head.push(first_token);
    (None, None, total)
}

/// Unit price of a whole-number count, if it divides the total evenly.
fn exact_unit(total: i64, quantity: Decimal) -> Option<i64> {
    let count = quantity.fract().is_zero().then(|| quantity.to_i64())??;
    (count != 0 && total % count == 0).then(|| total / count)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    Items,
    Sums,
    After,
}

enum SumEntry {
    Sum {
        row: usize,
        keyword: Keyword,
        amount: i64,
    },
    Adjustment {
        row: usize,
        draft: Draft,
    },
}

struct Reader {
    currency: Currency,
    zone: Zone,
    kinds: Vec<RowKind>,
    items: Vec<ParsedItem>,
    /// The newest item has no printed quantity yet and may take the next
    /// quantity row (Lidl prints it below the item).
    last_item_open: bool,
    /// A quantity row that did not fit the item above; it may belong to
    /// the next item (other tills print it above).
    pending_quantity: Option<(usize, Draft)>,
    sums: Vec<SumEntry>,
    subtotal: Option<i64>,
    total: Option<i64>,
    tendered: Option<i64>,
    change: Option<i64>,
}

impl Reader {
    fn new(currency: Currency, row_count: usize) -> Self {
        Self {
            currency,
            zone: Zone::Items,
            kinds: vec![RowKind::Other; row_count],
            items: Vec::new(),
            last_item_open: false,
            pending_quantity: None,
            sums: Vec::new(),
            subtotal: None,
            total: None,
            tendered: None,
            change: None,
        }
    }

    fn read(&mut self, row: usize, line: Line) -> Result<(), ReceiptError> {
        match (self.zone, line) {
            (
                _,
                Line::Keyword {
                    keyword: keyword @ (Keyword::Payment | Keyword::Change),
                    prices,
                },
            ) if !prices.is_empty() => {
                self.close_items();
                self.close_sums()?;
                self.zone = Zone::After;
                let amount = prices[0].abs();
                if keyword == Keyword::Payment {
                    self.kinds[row] = RowKind::Payment;
                    self.tendered.get_or_insert(amount);
                } else {
                    self.kinds[row] = RowKind::Change;
                    self.change.get_or_insert(amount);
                }
            }
            (
                Zone::Items | Zone::Sums,
                Line::Keyword {
                    keyword: keyword @ (Keyword::Total | Keyword::Subtotal),
                    prices,
                },
            ) if prices.len() == 1 => {
                self.close_items();
                self.zone = Zone::Sums;
                self.sums.push(SumEntry::Sum {
                    row,
                    keyword,
                    amount: prices[0],
                });
            }
            (_, Line::Tax) => self.kinds[row] = RowKind::Tax,
            (Zone::Items, Line::Item(draft)) => self.add_item(row, draft),
            (Zone::Items, Line::Quantity(draft)) => self.add_quantity(row, draft),
            (Zone::Sums, Line::Item(draft)) => {
                self.sums.push(SumEntry::Adjustment { row, draft });
            }
            _ => {}
        }
        Ok(())
    }

    fn push_item(&mut self, row: usize, draft: Draft) {
        let quantity = draft.quantity.unwrap_or(Decimal::ONE);
        let unit = draft
            .unit
            .or((quantity == Decimal::ONE).then_some(draft.total));
        self.kinds[row] = RowKind::Item;
        self.items.push(ParsedItem {
            text: draft.text,
            quantity,
            unit_price: unit.map(|u| Money::new(u, self.currency)),
            total_price: Money::new(draft.total, self.currency),
            kind: if draft.total < 0 {
                ItemKind::Discount
            } else {
                ItemKind::Article
            },
            rows: vec![row],
        });
    }

    fn add_item(&mut self, row: usize, draft: Draft) {
        let open = draft.quantity.is_none();
        self.push_item(row, draft);
        self.last_item_open = open;
        if let Some((quantity_row, quantity)) = self.pending_quantity.take() {
            if open && self.apply_quantity(quantity_row, &quantity) {
                if let Some(item) = self.items.last_mut() {
                    item.rows.insert(0, quantity_row);
                }
            } else {
                self.kinds[quantity_row] = RowKind::Other;
            }
        }
    }

    fn add_quantity(&mut self, row: usize, draft: Draft) {
        if self.last_item_open && self.apply_quantity(row, &draft) {
            if let Some(item) = self.items.last_mut() {
                item.rows.push(row);
            }
            return;
        }
        // An earlier unmatched quantity row stays `Other`.
        self.pending_quantity = Some((row, draft));
    }

    /// Gives the newest item the quantity of `draft` if it explains the
    /// item's total.
    fn apply_quantity(&mut self, row: usize, draft: &Draft) -> bool {
        let (Some(quantity), Some(item)) = (draft.quantity, self.items.last_mut()) else {
            return false;
        };
        let total = item.total_price.amount_minor();
        let fits = match draft.unit {
            Some(unit) => quantity_fits(quantity, unit, total),
            None => draft.total == total,
        };
        if !fits {
            return false;
        }
        item.quantity = quantity;
        item.unit_price = draft
            .unit
            .or_else(|| exact_unit(total, quantity))
            .map(|unit| Money::new(unit, self.currency));
        self.kinds[row] = RowKind::Quantity;
        self.last_item_open = false;
        true
    }

    fn close_items(&mut self) {
        self.last_item_open = false;
        // `kinds` already says `Other` for an unmatched quantity row.
        self.pending_quantity = None;
    }

    /// Picks the total and decides which rows between sums are items.
    fn close_sums(&mut self) -> Result<(), ReceiptError> {
        let mut sums: Vec<(usize, Keyword, i64)> = Vec::new();
        let mut between: Vec<(usize, Draft)> = Vec::new();
        for entry in std::mem::take(&mut self.sums) {
            match entry {
                SumEntry::Adjustment { row, draft } => between.push((row, draft)),
                SumEntry::Sum {
                    row,
                    keyword,
                    amount,
                } => {
                    if let Some(&(_, _, previous)) = sums.last() {
                        let adjustments = between
                            .iter()
                            .try_fold(0_i64, |sum, (_, draft)| sum.checked_add(draft.total))
                            .ok_or(ReceiptError::Overflow)?;
                        let step = amount.checked_sub(previous).ok_or(ReceiptError::Overflow)?;
                        if !between.is_empty() && adjustments == step {
                            for (adjustment_row, draft) in between.drain(..) {
                                self.push_item(adjustment_row, draft);
                            }
                        }
                    }
                    between.clear();
                    self.kinds[row] = RowKind::Subtotal;
                    sums.push((row, keyword, amount));
                }
            }
        }

        let total_at = sums
            .iter()
            .rposition(|(_, keyword, _)| *keyword == Keyword::Total)
            .or(sums.len().checked_sub(1));
        if let Some(at) = total_at {
            let (row, _, amount) = sums[at];
            self.kinds[row] = RowKind::Total;
            self.total = Some(amount);
            if at > 0 {
                self.subtotal = Some(sums[0].2);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt::BoundingBox;

    /// One fragment per printed row, as if the engine had read whole rows.
    fn receipt(rows: &[&str]) -> ParsedReceipt {
        receipt_in(rows, "EUR")
    }

    fn receipt_in(rows: &[&str], currency: &str) -> ParsedReceipt {
        let fragments: Vec<RecognizedText> = rows
            .iter()
            .zip(0..)
            .map(|(text, i)| RecognizedText {
                text: (*text).into(),
                bbox: BoundingBox {
                    left: 0,
                    top: i * 50,
                    right: 500,
                    bottom: i * 50 + 40,
                },
            })
            .collect();
        parse_receipt(&fragments, Currency::from_code(currency).unwrap()).unwrap()
    }

    fn dec(text: &str) -> Decimal {
        text.parse().unwrap()
    }

    /// (text, quantity, unit price minor, total minor)
    fn items(receipt: &ParsedReceipt) -> Vec<(&str, Decimal, Option<i64>, i64)> {
        receipt
            .items
            .iter()
            .map(|item| {
                (
                    item.text.as_str(),
                    item.quantity,
                    item.unit_price.map(|m| m.amount_minor()),
                    item.total_price.amount_minor(),
                )
            })
            .collect()
    }

    fn kinds(receipt: &ParsedReceipt) -> Vec<RowKind> {
        receipt.rows.iter().map(|row| row.kind).collect()
    }

    #[test]
    fn simple_receipt_with_total_and_change() {
        let r = receipt(&[
            "BÄCKEREI MUSTER",
            "Brötchen 0,40 A",
            "Brot 3,20 A",
            "SUMME EUR 3,60",
            "Gegeben 5,00",
            "Rückgeld 1,40",
        ]);
        assert_eq!(
            items(&r),
            [
                ("Brötchen", Decimal::ONE, Some(40), 40),
                ("Brot", Decimal::ONE, Some(320), 320)
            ]
        );
        assert_eq!(r.total.map(|m| m.amount_minor()), Some(360));
        assert_eq!(r.tendered.map(|m| m.amount_minor()), Some(500));
        assert_eq!(r.change.map(|m| m.amount_minor()), Some(140));
        assert_eq!(r.check, TotalCheck::Matches);
        use RowKind::*;
        assert_eq!(kinds(&r), [Other, Item, Item, Total, Payment, Change]);
    }

    #[test]
    fn quantity_row_below_item() {
        let r = receipt(&["Joghurt 0,98 A", "2 X 0,49", "Summe 0,98"]);
        assert_eq!(items(&r), [("Joghurt", dec("2"), Some(49), 98)]);
        assert_eq!(r.items[0].rows, [0, 1]);
        assert_eq!(r.rows[1].kind, RowKind::Quantity);
    }

    #[test]
    fn quantity_row_above_item() {
        let r = receipt(&[
            "Butter 1,99 B",
            "2 Stk x 0,89",
            "Milch 1,78 B",
            "Summe 3,77",
        ]);
        assert_eq!(
            items(&r),
            [
                ("Butter", Decimal::ONE, Some(199), 199),
                ("Milch", dec("2"), Some(89), 178)
            ]
        );
        assert_eq!(r.items[1].rows, [1, 2]);
        assert_eq!(r.check, TotalCheck::Matches);
    }

    #[test]
    fn weighed_goods() {
        let r = receipt(&["Bananen 1,80 B", "0,452 kg x 3,99 EUR/kg", "Summe 1,80"]);
        assert_eq!(items(&r), [("Bananen", dec("0.452"), Some(399), 180)]);
    }

    #[test]
    fn unmatched_quantity_row_is_ignored() {
        let r = receipt(&["Brot 3,20", "2 x 0,49", "Summe 3,20"]);
        assert_eq!(items(&r), [("Brot", Decimal::ONE, Some(320), 320)]);
        assert_eq!(r.rows[1].kind, RowKind::Other);
    }

    #[test]
    fn inline_quantities() {
        let r = receipt(&[
            "Cola 2 x 1,50 3,00",
            "Wasser 3 x 0,80",
            "Bier x2 7,00",
            "2x Brezel 1,60",
            "4 Kaffee 2,50 10,00",
            "Summe 24,00",
        ]);
        assert_eq!(
            items(&r),
            [
                ("Cola", dec("2"), Some(150), 300),
                ("Wasser", dec("3"), Some(80), 240),
                ("Bier", dec("2"), Some(350), 700),
                ("Brezel", dec("2"), Some(80), 160),
                ("Kaffee", dec("4"), Some(250), 1000),
            ]
        );
        assert_eq!(r.check, TotalCheck::Matches);
    }

    #[test]
    fn pack_size_in_the_name_is_no_quantity() {
        let r = receipt(&["Ersatzfilter 2 Stk 9,99 D", "Summe 9,99"]);
        assert_eq!(
            items(&r),
            [("Ersatzfilter 2 Stk", Decimal::ONE, Some(999), 999)]
        );
    }

    #[test]
    fn two_unrelated_prices_keep_the_first_in_the_text() {
        let r = receipt(&["Gutschein 5,00 3,99", "Summe 3,99"]);
        assert_eq!(
            items(&r),
            [("Gutschein 5,00", Decimal::ONE, Some(399), 399)]
        );
    }

    #[test]
    fn discount_between_subtotal_and_total() {
        let r = receipt(&[
            "Futter 14,99",
            "SUMME 14,99",
            "Aktion -2,00",
            "Summe 12,99",
            "Bar 20,00",
        ]);
        assert_eq!(r.subtotal.map(|m| m.amount_minor()), Some(1499));
        assert_eq!(r.total.map(|m| m.amount_minor()), Some(1299));
        assert_eq!(r.items[1].kind, ItemKind::Discount);
        assert_eq!(r.items[1].total_price.amount_minor(), -200);
        assert_eq!(r.check, TotalCheck::Matches);
        use RowKind::*;
        assert_eq!(kinds(&r), [Item, Subtotal, Item, Total, Payment]);
    }

    #[test]
    fn rows_between_sums_that_do_not_add_up_are_no_items() {
        let r = receipt(&["Futter 14,99", "Summe 14,99", "Punkte 1,00", "Summe 14,99"]);
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.rows[2].kind, RowKind::Other);
        assert_eq!(r.check, TotalCheck::Matches);
    }

    #[test]
    fn negative_item_and_trailing_minus() {
        let r = receipt(&[
            "Wasser 6 x 0,49 2,94",
            "Pfand 1,50",
            "Leergut 0,75-",
            "Summe 3,69",
        ]);
        assert_eq!(r.items[2].total_price.amount_minor(), -75);
        assert_eq!(r.items[2].kind, ItemKind::Discount);
        assert_eq!(r.check, TotalCheck::Matches);
    }

    #[test]
    fn minus_after_every_amount_is_decoration() {
        let r = receipt(&[
            "TAFEL 9,99-A",
            "LAMPE 3,99-A",
            "Zwischensumme 13,98-",
            "Gesamt 13,98-",
        ]);
        assert_eq!(r.items_sum().unwrap().amount_minor(), 1398);
        assert_eq!(r.subtotal.map(|m| m.amount_minor()), Some(1398));
        assert_eq!(r.total.map(|m| m.amount_minor()), Some(1398));
        assert_eq!(r.check, TotalCheck::Matches);
    }

    #[test]
    fn vat_rows_are_not_items() {
        let r = receipt(&[
            "Steak 19,90",
            "MWST Netto Steuer Brutto",
            "7% 18,60 1,30 19,90",
            "enth. MwSt 1,30",
            "Summe 19,90",
            "Karte 19,90",
            "A 7 % 1,30 18,60 19,90",
        ]);
        assert_eq!(r.items.len(), 1);
        use RowKind::*;
        assert_eq!(kinds(&r), [Item, Other, Tax, Tax, Total, Payment, Tax]);
    }

    #[test]
    fn percentage_discount_is_an_item() {
        let r = receipt(&["Hose 20,00", "Rabatt 10% -2,00", "Summe 18,00"]);
        assert_eq!(r.items[1].kind, ItemKind::Discount);
        assert_eq!(r.check, TotalCheck::Matches);
    }

    #[test]
    fn nothing_after_payment_is_an_item() {
        let r = receipt(&["Gurke 0,77 A", "zu zahlen 0,77", "Karte 0,77", "EUR 0,77"]);
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.rows[3].kind, RowKind::Other);
    }

    #[test]
    fn deviation_from_total_is_reported() {
        let r = receipt(&["Brot 3,20", "Milch 1,19", "Summe 5,39"]);
        assert_eq!(
            r.check,
            TotalCheck::Differs {
                items_sum: Money::new(439, r.currency),
                difference: Money::new(100, r.currency),
            }
        );
    }

    #[test]
    fn missing_total() {
        let r = receipt(&["Brot 3,20", "Milch 1,19"]);
        assert_eq!(r.total, None);
        assert_eq!(r.check, TotalCheck::NoTotal);
        assert_eq!(r.items.len(), 2);
    }

    #[test]
    fn sum_row_with_several_amounts_is_no_total() {
        let r = receipt(&["Brot 3,20", "Summe 0,21 2,99 3,20"]);
        assert_eq!(r.total, None);
        assert_eq!(r.rows[1].kind, RowKind::Other);
    }

    #[test]
    fn empty_input() {
        let r = receipt(&[]);
        assert!(r.rows.is_empty() && r.items.is_empty());
        assert_eq!(r.check, TotalCheck::NoTotal);
    }

    #[test]
    fn yen_amounts_use_the_currency_exponent() {
        // Japanese sum words (合計, OCR-17) follow with AP-20.
        let r = receipt_in(&["中華そば ¥748", "特選セットA 1,120円"], "JPY");
        assert_eq!(
            items(&r),
            [
                ("中華そば", Decimal::ONE, Some(748), 748),
                ("特選セットA", Decimal::ONE, Some(1120), 1120)
            ]
        );
    }
}
