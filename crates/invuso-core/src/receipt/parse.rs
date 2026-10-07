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
//!
//! Japanese receipts (OCR-17) add: sum words anywhere in the row (`小計`,
//! `合計`, `お釣り`), a table header (`品名 … 金額`) that ends the shop's
//! header, amounts without minor units, counts glued to names and tax
//! added on top (`外税`).

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::details::{is_dated, read_details};
use super::rows::group_rows;
use super::tokens::{
    count_glued_to_name, count_in_brackets, is_currency_mark, is_piece_word, is_tax_class_word,
    is_times, is_trailing_mark, is_unit_word, keyword_form, line_total, parse_count, parse_price,
    parse_quantity, quantity_after_times, quantity_before_times, quantity_fits, split_glued_amount,
    strip_tax_class,
};
use super::vat::read_vat;
use super::{
    ItemKind, ParsedItem, ParsedReceipt, ReceiptDetails, ReceiptError, ReceiptRow, RecognizedText,
    RowKind, TotalCheck,
};
use crate::domain::{Currency, Money};

/// Parses OCR output of one receipt whose amounts are in `currency`
/// (OCR-10..14). Deterministic and independent of the OCR engine.
pub fn parse_receipt(
    fragments: &[RecognizedText],
    currency: Currency,
) -> Result<ParsedReceipt, ReceiptError> {
    let rows = group_rows(fragments);
    let mut tokens: Vec<Vec<&str>> = rows.iter().map(|row| split_row(&row.text)).collect();
    let cx = Context::new(currency, &tokens);
    let moved = attach_lone_amounts(&mut tokens, &cx);

    let mut reader = Reader::new(currency, rows.len());
    // Rows of text only right above, which may name prices printed below.
    let mut text_above: Vec<usize> = Vec::new();
    for (index, row_tokens) in tokens.iter().enumerate() {
        let mut line = analyze(row_tokens, &cx);
        let mut wrapped = None;
        if let Line::Item(draft) = &mut line
            && !draft
                .text
                .split_whitespace()
                .filter(|token| !is_currency_mark(token, currency))
                .any(|token| token.chars().any(char::is_alphabetic))
            && let Some(&above) = text_above.last()
        {
            take_name_from_above(draft, &rows[above].text, &cx);
            wrapped = Some(above);
        }
        // `1 x 469,99 469,99` below a name and its article and serial
        // numbers: a quantity row with its own total and nothing priced
        // above it is the item of that name.
        if let Line::Quantity(draft) = &line
            && row_tokens
                .iter()
                .filter(|token| cx.price(token).is_some())
                .count()
                >= 2
            && let Some(above) = name_row(&text_above, &tokens)
        {
            let mut draft = draft.clone();
            draft.text = rows[above].text.clone();
            line = Line::Item(draft);
            wrapped = Some(above);
        }
        if matches!(line, Line::Plain) && rows[index].text.chars().any(char::is_alphabetic) {
            text_above.push(index);
        } else {
            text_above.clear();
        }
        let items_before = reader.items.len();
        reader.read(index, line)?;
        if let Some(above) = wrapped
            && reader.items.len() > items_before
            && let Some(item) = reader.items.last_mut()
        {
            item.rows.insert(0, above);
            reader.kinds[above] = RowKind::Item;
        }
    }
    reader.close_items();
    reader.close_sums()?;
    for (from, to) in moved {
        reader.kinds[from] = reader.kinds[to];
    }

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
        details: ReceiptDetails::default(),
        vat: Vec::new(),
    };
    receipt.details = read_details(&receipt.rows);
    receipt.vat = read_vat(&receipt.rows, currency);
    // Tax is less than the total, and tax added on top is an item already.
    let added: Vec<i64> = receipt
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Tax)
        .map(|item| item.total_price.amount_minor())
        .collect();
    let added_sum: i64 = added.iter().sum();
    let total_minor = receipt.total.map(|m| m.amount_minor());
    receipt.vat.retain(|line| {
        let amount = line.amount.amount_minor();
        total_minor.is_none_or(|total| amount < total)
            && !added.contains(&amount)
            && amount != added_sum
    });
    drop_items_above_the_date(&mut receipt);
    attach_to_articles(&mut receipt);
    if let Some(total) = reader.total {
        repair_yen_marks(&mut receipt.items, total, currency);
        repair_returns(&mut receipt.items, total);
        drop_counts_read_as_yen(&mut receipt.items, total, currency);
        recover_misread_prices(&mut receipt, total, &cx);
    }
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

/// A deposit or discount printed right below an article (or below its
/// deposit) belongs to it. Returned bottles stand on their own, and so does
/// whatever is printed among the sums (a discount on the whole receipt).
fn attach_to_articles(receipt: &mut ParsedReceipt) {
    let first_sum = receipt
        .rows
        .iter()
        .position(|row| matches!(row.kind, RowKind::Subtotal | RowKind::Total))
        .unwrap_or(receipt.rows.len());
    for at in 1..receipt.items.len() {
        let (before, rest) = receipt.items.split_at_mut(at);
        let (previous, item) = (&before[at - 1], &mut rest[0]);
        let below = match (previous.rows.last(), item.rows.first()) {
            (Some(&last), Some(&first)) => first > last && first - last <= 2 && first < first_sum,
            _ => false,
        };
        let follows_article = previous.kind == ItemKind::Article || previous.attached;
        let adds_to_it = matches!(item.kind, ItemKind::Deposit | ItemKind::Discount)
            && !names_any(&item.text, RETURNED_BOTTLES);
        item.attached = below && follows_article && adds_to_it;
    }
}

/// Words of returned bottles: a refund of its own, not part of an article.
const RETURNED_BOTTLES: &[&str] = &[
    "leergut",
    "pfandrück",
    "pfandruck",
    "pfandbon",
    "pfandgutschrift",
];

/// Japanese receipts print shop data, then the date, then the items. When
/// items follow the first dated row, nothing above it is an item: numbers
/// up there are phone, slip and register numbers.
fn drop_items_above_the_date(receipt: &mut ParsedReceipt) {
    let japanese = super::language::detect_language(receipt.rows.iter().map(|r| r.text.as_str()))
        == Some("ja");
    let Some(dated) = receipt
        .rows
        .iter()
        .position(|row| is_dated(&row.text, japanese))
    else {
        return;
    };
    let first_row = |item: &ParsedItem| item.rows.first().copied().unwrap_or(0);
    if !receipt.items.iter().any(|item| first_row(item) > dated) {
        return;
    }
    let rows = &mut receipt.rows;
    receipt.items.retain(|item| {
        let keep = first_row(item) > dated;
        if !keep {
            for &row in &item.rows {
                rows[row].kind = RowKind::Other;
            }
        }
        keep
    });
}

/// Set parts and side dishes print their count (`1`) where other rows
/// print a price; in yen that reads as 1 yen. If the items exceed the
/// printed total by exactly the sum of such one-digit amounts, they go.
fn drop_counts_read_as_yen(items: &mut Vec<ParsedItem>, total: i64, currency: Currency) {
    if currency.exponent() != 0 {
        return;
    }
    let is_count = |item: &ParsedItem| {
        item.kind == ItemKind::Article
            && item.quantity == Decimal::ONE
            && (1..=9).contains(&item.total_price.amount_minor())
    };
    let sum: i64 = items.iter().map(|i| i.total_price.amount_minor()).sum();
    let counts: i64 = items
        .iter()
        .filter(|i| is_count(i))
        .map(|i| i.total_price.amount_minor())
        .sum();
    if counts > 0 && sum != total && sum - counts == total {
        items.retain(|item| !is_count(item));
    }
}

/// A price whose tax class was read as one more digit (`0,99 B` →
/// `0,990`) is no amount, so its item is lost. If exactly one such row
/// above the sums makes the items match the printed total, it becomes the
/// item it was.
fn recover_misread_prices(receipt: &mut ParsedReceipt, total: i64, cx: &Context) {
    let exponent = cx.currency.exponent() as usize;
    if exponent == 0 {
        return;
    }
    let sum: i64 = receipt
        .items
        .iter()
        .map(|item| item.total_price.amount_minor())
        .sum();
    let missing = total - sum;
    if missing <= 0 {
        return;
    }
    let first_sum = receipt
        .rows
        .iter()
        .position(|row| matches!(row.kind, RowKind::Subtotal | RowKind::Total))
        .unwrap_or(receipt.rows.len());
    let candidates: Vec<(usize, String)> = receipt.rows[..first_sum]
        .iter()
        .enumerate()
        .filter(|(_, row)| row.kind == RowKind::Other)
        .filter_map(|(index, row)| {
            let mut tokens: Vec<&str> = row.text.split_whitespace().collect();
            // Tax classes after the price (`0.,69 B`).
            while tokens.len() > 2
                && tokens
                    .last()
                    .is_some_and(|token| is_trailing_mark(token, cx.currency))
            {
                tokens.pop();
            }
            let (last, name) = tokens.split_last()?;
            let amount = misread_price(last, cx)?;
            let name = name.join(" ");
            (amount == missing && name.chars().any(char::is_alphabetic)).then_some((index, name))
        })
        .collect();
    let [(row, name)] = candidates.as_slice() else {
        return;
    };
    let at = receipt
        .items
        .iter()
        .position(|item| item.rows.first().is_some_and(|first| first > row))
        .unwrap_or(receipt.items.len());
    let price = Money::new(missing, cx.currency);
    receipt.items.insert(
        at,
        ParsedItem {
            text: name.clone(),
            quantity: Decimal::ONE,
            unit_price: Some(price),
            total_price: price,
            kind: ItemKind::Article,
            rows: vec![*row],
            attached: false,
        },
    );
    receipt.rows[*row].kind = RowKind::Item;
}

/// The amount a token that is no price was most likely meant to be: one
/// digit too many at the end (`0,990`, the class read as `0`) or a doubled
/// separator (`0.,69`).
fn misread_price(token: &str, cx: &Context) -> Option<i64> {
    if cx.price(token).is_some() {
        return None;
    }
    // Only an ASCII digit is dropped, so the cut is on a character boundary.
    let without_digit = token
        .strip_suffix(|c: char| c.is_ascii_digit())
        .and_then(|trimmed| cx.price(trimmed));
    let single_separator = ["..", ".,", ",.", ",,"]
        .iter()
        .find(|double| token.contains(*double))
        .and_then(|double| cx.price(&token.replacen(double, ",", 1)));
    without_digit.or(single_separator)
}

/// At most this many tax rows are tried as added tax (2^n sums).
const MAX_TAX_CANDIDATES: usize = 8;

/// At most this many items are tried in [`repair_yen_marks`] (2^n sums).
const MAX_MARK_CANDIDATES: usize = 10;

/// Older Japanese tills print a small `円` after each amount, which the
/// recognizer may read as a trailing `1` (`320円` → `3201`). If the items
/// miss the printed total and dropping that digit from some of them makes
/// it match exactly, they are corrected; the fewest changes win, and an
/// ambiguous choice changes nothing.
fn repair_yen_marks(items: &mut [ParsedItem], total: i64, currency: Currency) {
    if currency.exponent() != 0 {
        return;
    }
    let sum: Option<i64> = items.iter().try_fold(0_i64, |sum, item| {
        sum.checked_add(item.total_price.amount_minor())
    });
    let Some(sum) = sum.filter(|sum| *sum != total) else {
        return;
    };
    let candidates: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            let amount = item.total_price.amount_minor();
            item.quantity == Decimal::ONE && amount >= 11 && amount % 10 == 1
        })
        .map(|(index, _)| index)
        .take(MAX_MARK_CANDIDATES)
        .collect();
    // Dropping the digit lowers an amount by `amount - amount / 10`.
    let saving = |index: usize| {
        let amount = items[index].total_price.amount_minor();
        amount - amount / 10
    };
    let mut best: Option<(u32, u32, usize)> = None; // (changes, mask, matches)
    for mask in 1_u32..(1 << candidates.len()) {
        let saved: i64 = candidates
            .iter()
            .enumerate()
            .filter(|(bit, _)| mask & (1 << bit) != 0)
            .map(|(_, &index)| saving(index))
            .sum();
        if sum - saved != total {
            continue;
        }
        let changes = mask.count_ones();
        best = match best {
            Some((fewest, _, _)) if changes > fewest => best,
            Some((fewest, kept, count)) if changes == fewest => Some((fewest, kept, count + 1)),
            _ => Some((changes, mask, 1)),
        };
    }
    let Some((_, mask, 1)) = best else {
        return;
    };
    for (bit, &index) in candidates.iter().enumerate() {
        if mask & (1 << bit) != 0 {
            let item = &mut items[index];
            let amount = Money::new(item.total_price.amount_minor() / 10, currency);
            item.total_price = amount;
            item.unit_price = Some(amount);
        }
    }
}

/// The words of a row. A tax class in front of the yen sign is dropped
/// (`外2¥150`); a name or label glued to the last amount (`620計`,
/// `ロールパン200※`) becomes a word of its own.
fn split_row(text: &str) -> Vec<&str> {
    let mut tokens: Vec<&str> = text
        .split_whitespace()
        .map(|token| strip_tax_class(token).unwrap_or(token))
        .collect();
    if let Some(last) = tokens.pop() {
        match split_glued_amount(last) {
            Some((name, amount, label)) => tokens.extend(
                [name, amount, label]
                    .into_iter()
                    .filter(|part| !part.is_empty()),
            ),
            None => tokens.push(last),
        }
    }
    tokens
}

/// Japanese tills print `合計` and its amount in different font sizes, so
/// they may land in rows of their own. A sum word without an amount takes
/// a lone amount from the row right above or below. Returns the rows that
/// gave their amount away and the row that took it.
fn attach_lone_amounts(tokens: &mut [Vec<&str>], cx: &Context) -> Vec<(usize, usize)> {
    let mut moved: Vec<(usize, usize)> = Vec::new();
    for at in 0..tokens.len() {
        let wants_amount = matches!(
            keyword(&tokens[at], cx),
            Some(Keyword::Total | Keyword::Subtotal)
        ) && !tokens[at].iter().any(|token| cx.price(token).is_some());
        if !wants_amount {
            continue;
        }
        for from in [at.checked_sub(1), Some(at + 1)].into_iter().flatten() {
            let taken = moved.iter().any(|&(f, t)| f == from || t == from);
            if from >= tokens.len() || taken {
                continue;
            }
            if let Some(amount) = lone_amount(&tokens[from], cx) {
                tokens[at].push(amount);
                tokens[from].clear();
                moved.push((from, at));
                break;
            }
        }
    }
    moved
}

/// The only amount of a row that holds nothing else but marks.
fn lone_amount<'a>(tokens: &[&'a str], cx: &Context) -> Option<&'a str> {
    let mut words = tokens
        .iter()
        .filter(|token| !is_trailing_mark(token, cx.currency));
    let amount = *words.next()?;
    (words.next().is_none() && cx.price(amount).is_some()).then_some(amount)
}

struct Context {
    currency: Currency,
    /// False when the till prints a minus after every amount (IKEA).
    trailing_minus_negates: bool,
    /// VAT class marks of this receipt (`A`, `B`): short capitals printed
    /// next to prices in several rows, removed from the end of names.
    tax_classes: Vec<String>,
}

impl Context {
    fn new(currency: Currency, rows: &[Vec<&str>]) -> Self {
        let prices: Vec<_> = rows
            .iter()
            .flatten()
            .filter_map(|token| parse_price(token, currency))
            .collect();
        let with_minus = prices.iter().filter(|p| p.trailing_minus).count();
        let is_price = |token: &str| parse_price(token, currency).is_some();
        let mut marks: BTreeMap<&str, usize> = BTreeMap::new();
        for row in rows {
            for pair in row.windows(2) {
                let mark = match pair {
                    [mark, price] if is_price(price) => mark,
                    [price, mark] if is_price(price) => mark,
                    _ => continue,
                };
                let is_class = (1..=2).contains(&mark.len())
                    && mark.chars().all(|c| c.is_ascii_uppercase())
                    && !is_times(mark)
                    && !is_unit_word(mark);
                if is_class {
                    *marks.entry(mark).or_default() += 1;
                }
            }
        }
        Self {
            currency,
            trailing_minus_negates: prices.len() < 2 || with_minus * 2 <= prices.len(),
            tax_classes: marks
                .into_iter()
                .filter(|(_, count)| *count >= 2)
                .map(|(mark, _)| mark.to_string())
                .collect(),
        }
    }

    fn is_tax_class(&self, token: &str) -> bool {
        self.tax_classes.iter().any(|class| class == token)
    }

    /// The name without the VAT class at its end (`Stabkerze A`).
    fn trim_tax_class(&self, words: &mut Vec<&str>) {
        while words.len() > 1 && words.last().is_some_and(|word| self.is_tax_class(word)) {
            words.pop();
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

/// A word with this many digits is a code (article, EAN, serial number),
/// not part of an article name.
const CODE_DIGITS: usize = 5;

/// First words of VAT rows without a percentage (`enth. MwSt 1,59`).
const TAX_WORDS: &[&str] = &[
    "netto",
    "brutto",
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

/// Parts of words naming a deposit paid on top (`Pfand`, `Einwegpfand`,
/// `PFAND 0,25`), compared in [`keyword_form`].
const DEPOSIT_WORDS: &[&str] = &["pfand", "deposit", "cauzione"];

/// Parts of words naming money given back: returned bottles, vouchers,
/// coupons and discounts (OCR-15). Checked before [`DEPOSIT_WORDS`], since
/// `Pfandrückgabe` and `Pfandbon` contain `pfand`.
const RETURN_WORDS: &[&str] = &[
    "leergut",
    "pfandrück",
    "pfandruck",
    "pfandbon",
    "pfandgutschrift",
    "gutschein",
    "gutschrift",
    "coupon",
    "voucher",
    "rabatt",
    "nachlass",
    "preisvorteil",
    "discount",
    "クーポン",
    "割引",
    "値引",
];

/// What an item is: negative amounts subtract; a positive amount is a
/// deposit if its first word says so (`Wasser inkl. Pfand` is an article).
fn item_kind(draft: &Draft) -> ItemKind {
    let first_word = draft.text.split_whitespace().next().unwrap_or_default();
    if draft.tax {
        ItemKind::Tax
    } else if draft.total < 0 {
        ItemKind::Discount
    } else if names_any(first_word, DEPOSIT_WORDS) && !names_any(&draft.text, RETURN_WORDS) {
        ItemKind::Deposit
    } else {
        ItemKind::Article
    }
}

/// Whether a word of `text` contains one of `parts`.
fn names_any(text: &str, parts: &[&str]) -> bool {
    text.split_whitespace()
        .map(keyword_form)
        .any(|word| parts.iter().any(|part| word.contains(part)))
}

/// Some tills print returned bottles and vouchers without a minus
/// (`Leergut 0,25`). If the items miss the printed total and subtracting
/// all such lines instead of adding them makes it match exactly, they
/// become discounts.
fn repair_returns(items: &mut [ParsedItem], total: i64) {
    let is_return = |item: &ParsedItem| {
        item.kind == ItemKind::Article
            && item.total_price.amount_minor() > 0
            && names_any(&item.text, RETURN_WORDS)
    };
    let sum = items.iter().try_fold(0_i64, |sum, item| {
        sum.checked_add(item.total_price.amount_minor())
    });
    let returned = items
        .iter()
        .filter(|item| is_return(item))
        .try_fold(0_i64, |sum, item| {
            sum.checked_add(item.total_price.amount_minor())
        });
    let (Some(sum), Some(returned)) = (sum, returned) else {
        return;
    };
    let flipped = returned
        .checked_mul(2)
        .and_then(|twice| sum.checked_sub(twice));
    if returned == 0 || sum == total || flipped != Some(total) {
        return;
    }
    for item in items.iter_mut().filter(|item| is_return(item)) {
        let currency = item.total_price.currency();
        item.total_price = Money::new(-item.total_price.amount_minor(), currency);
        item.unit_price = item
            .unit_price
            .map(|unit| Money::new(-unit.amount_minor(), currency));
        item.kind = ItemKind::Discount;
    }
}

/// Words saying a VAT amount is already in the prices.
const INCLUDED_TAX_WORDS: &[&str] = &["inkl", "enth", "incl", "included", "netto", "brutto"];

/// Japanese sum words, found anywhere in the row since tills print dates
/// or counts in front (`2022年03月01日 小計 712`) and space the letters
/// out (`小 計`). Checked in this order: `お預り合計` is a payment.
const JAPANESE_KEYWORDS: &[(&[&str], Keyword)] = &[
    (&["釣", "おつり"], Keyword::Change),
    (
        &[
            "預",
            "現金",
            "クレジット",
            "支払",
            "電子マネー",
            "コード",
            "交通系",
            "楽天ペイ",
            "払い",
        ],
        Keyword::Payment,
    ),
    (&["小計", "税抜計", "品計"], Keyword::Subtotal),
    (&["合計", "総計", "会計", "現計", "お買上"], Keyword::Total),
];

/// Words of Japanese tax rows: `内税` (included), `外税` (added),
/// `消費税`, `税額`; a row ending in `税` (`45円税`) too.
const JAPANESE_TAX_WORDS: &[&str] = &["内税", "外税", "消費税", "税額"];

fn keyword(tokens: &[&str], cx: &Context) -> Option<Keyword> {
    if is_count_row(tokens) {
        return None;
    }
    leading_keyword(tokens)
        .or_else(|| {
            // A speck in front of the word, read as a letter (`E Summe 34,09`).
            let (first, rest) = tokens.split_first()?;
            (keyword_form(first).chars().count() == 1)
                .then(|| leading_keyword(rest))
                .flatten()
        })
        .or_else(|| japanese_keyword(&label(tokens, cx)))
        .or_else(|| payment_brand(tokens))
}

/// Cashless payment brands printed with the amount paid (`nanaco支払
/// ¥1,294`, `QRコード(PayPay等) 8,800`).
const PAYMENT_BRANDS: &[&str] = &["paypay", "nanaco", "waon", "suica", "pasmo", "icoca"];

fn payment_brand(tokens: &[&str]) -> Option<Keyword> {
    tokens
        .iter()
        .any(|token| {
            let lower = token.to_lowercase();
            PAYMENT_BRANDS.iter().any(|brand| lower.contains(brand))
        })
        .then_some(Keyword::Payment)
}

fn japanese_keyword(label: &str) -> Option<Keyword> {
    if label.is_ascii() {
        return None;
    }
    JAPANESE_KEYWORDS
        .iter()
        .find(|(words, _)| words.iter().any(|word| label.contains(word)))
        .map(|(_, keyword)| *keyword)
        // `620 計`: a lone `計` is the total, inside a word it is not (`時計`).
        .or_else(|| (label == "計").then_some(Keyword::Total))
}

/// The letters of a row without its amounts, for Japanese words. Latin
/// letters in a Japanese row are recognition noise (`合 KR 言十`), and a
/// spaced-out `計` is often read as `言` and `十`.
fn label(tokens: &[&str], cx: &Context) -> String {
    let letters: String = tokens
        .iter()
        .filter(|token| cx.price(token).is_none())
        .map(|token| keyword_form(token))
        .collect();
    if letters.is_ascii() {
        return letters;
    }
    letters
        .chars()
        .filter(|c| !c.is_ascii())
        .collect::<String>()
        .replace("言計十", "計")
        .replace("言十", "計")
}

/// `2 点`, `合計点数 3点`: the number of articles, not an amount. A row
/// with a yen amount is a sum after all (`合計 1点 ¥330`).
fn is_count_row(tokens: &[&str]) -> bool {
    let counts = tokens.iter().any(|token| {
        token.contains("点数")
            || token
                .strip_suffix('点')
                .is_some_and(|count| count.chars().all(|c| c.is_ascii_digit()))
    });
    let yen = tokens.iter().any(|token| token.contains(['¥', '￥', '円']));
    counts && !yen
}

/// `(税合計 ¥17`, `消費税等(8%)合計 ¥229`: the tax in the total, not the
/// total. `合計(税込)`, `税込合計` and `税抜計` are sums, `外税計` is
/// tax added on top.
fn is_tax_sum(label: &str) -> bool {
    !label.is_ascii()
        && label.contains('税')
        && label.contains('計')
        && !["合計", "総計", "小計", "外"]
            .iter()
            .any(|word| label.starts_with(word))
        && !label.contains("税込")
        && !label.contains("税抜")
}

/// Words of header and footer rows whose numbers are no prices: phone,
/// registration, terminal, slip, table and staff numbers.
const HEADER_INFO_WORDS: &[&str] = &[
    "登録番号",
    "電話",
    "端末",
    "レシート",
    "伝票",
    "テーブル",
    "呼出",
    "店舗",
    "取引",
    "担当",
];

/// A row of the shop's or slip's data rather than an item: `TEL03-3694-8275
/// レジ118677`, `登録番号 T1234`, or an address (`愛知県名古屋市中区大須3-30-8`).
fn is_header_info(tokens: &[&str]) -> bool {
    let text: String = tokens.concat();
    let lower = text.to_lowercase();
    let address = ['都', '道', '府', '県'].iter().any(|c| text.contains(*c))
        && ['市', '区', '町', '村'].iter().any(|c| text.contains(*c))
        && text.chars().any(|c| c.is_ascii_digit());
    lower.starts_with("tel") || HEADER_INFO_WORDS.iter().any(|word| text.contains(word)) || address
}

/// `品名 単価 数量 金額`, `数 メニュー 金額`: the column header of the item
/// table. Rows above it are the shop's header (`人数= 2`, `伝票No 985`),
/// whose numbers would pass for yen amounts.
fn is_table_header(label: &str) -> bool {
    (label.contains("金額") || label.contains("金额"))
        && ["品", "メ", "数"].iter().any(|word| label.contains(word))
}

fn leading_keyword(tokens: &[&str]) -> Option<Keyword> {
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
    /// Column header of a Japanese item table.
    Header,
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
    /// Tax added on top (`外税`).
    tax: bool,
}

fn analyze(tokens: &[&str], cx: &Context) -> Line {
    let prices: Vec<i64> = tokens.iter().filter_map(|token| cx.price(token)).collect();
    let label = label(tokens, cx);
    if is_table_header(&label) {
        return Line::Header;
    }
    if is_tax_sum(&label) {
        return Line::Tax;
    }
    if let Some(keyword) = keyword(tokens, cx) {
        return Line::Keyword { keyword, prices };
    }
    let Some(&first_price) = prices.first() else {
        return Line::Plain;
    };
    // A bare number on a row of its own (`4197`) is a code in yen; a yen
    // amount below its name carries a mark (`¥280`).
    if let &[token] = tokens
        && cx.currency.exponent() == 0
        && token.chars().all(|c| c.is_ascii_digit())
    {
        return Line::Plain;
    }
    if is_count_row(tokens) || is_header_info(tokens) {
        return Line::Plain;
    }
    // `10%対象計 ¥210`, `税込対象額 ¥8,800`: the amount a rate applies to.
    let japanese_tax = !label.is_ascii()
        && (JAPANESE_TAX_WORDS.iter().any(|word| label.contains(word))
            || label.ends_with('税')
            || label.contains("対象"));
    if japanese_tax {
        // `外税 8% ¥18` adds tax; `外税8%対象額 ¥228` is the base it is
        // computed on.
        // So may `消費税等(8%) ¥8` and `外8% ¥111` between the subtotals
        // of tills that print prices without tax; whether they were added
        // is decided against the total (`add_taxes_if_they_explain`).
        let added = matches!(label.as_str(), "外税" | "外税額")
            || ((label.starts_with('外') || label.starts_with("消費税"))
                && !label.contains('内')
                && !label.contains("対象"));
        return match prices.as_slice() {
            &[amount] if amount > 0 && added => Line::Item(Draft {
                text: "外税".to_string(),
                quantity: None,
                unit: None,
                total: amount,
                tax: true,
            }),
            _ => Line::Tax,
        };
    }
    let has_percent = tokens.iter().any(|token| token.contains('%'));
    let starts_with_tax_word = tokens
        .first()
        .is_some_and(|token| TAX_WORDS.contains(&keyword_form(token).as_str()));
    // `Rabatt 20% -1,00` is a discount; `A 7 % 0,53 7,51 8,04` a VAT row.
    if (has_percent && (prices.len() >= 2 || first_price >= 0)) || starts_with_tax_word {
        // `TAX 6% $1.87` between subtotal and total may be added on top;
        // whether it was is decided against the total.
        let included = tokens.iter().any(|token| {
            INCLUDED_TAX_WORDS
                .iter()
                .any(|word| keyword_form(token).starts_with(word))
        });
        if starts_with_tax_word
            && !included
            && let &[amount] = prices.as_slice()
            && amount > 0
        {
            let text = tokens
                .iter()
                .filter(|token| cx.price(token).is_none())
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            return Line::Item(Draft {
                text,
                quantity: None,
                unit: None,
                total: amount,
                tax: true,
            });
        }
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
    // dm prints the VAT class as a digit after the price (`7,50 2`). With
    // minor units a lone digit is no amount, so after a price it is a mark.
    let tax_class_digit = |at: usize| {
        cx.currency.exponent() > 0
            && at >= 1
            && tokens[at].len() == 1
            && tokens[at].chars().all(|c| c.is_ascii_digit())
            && cx.price(tokens[at - 1]).is_some()
    };
    while end > 0 && (is_trailing_mark(tokens[end - 1], cx.currency) || tax_class_digit(end - 1)) {
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
    // `贅沢ハンバーグ 1,000 1 1,000`: unit price, count and total, as
    // Japanese tills print them; in yen the count reads as an amount too.
    if cx.currency.exponent() == 0
        && let &[count, total] = prices.as_slice()
        && parse_count(tokens[end]).is_some()
        && end > 0
        && let Some(unit) = cx.price(tokens[end - 1])
        && count > 0
        && unit.checked_mul(count) == Some(total)
    {
        return Line::Item(Draft {
            text: tokens[..end - 1].join(" "),
            quantity: Some(Decimal::from(count)),
            unit: Some(unit),
            total,
            tax: false,
        });
    }

    let mut head: Vec<&str> = tokens[..end].to_vec();
    if head.first().is_some_and(|word| is_tax_class_word(word)) {
        head.remove(0);
    }
    cx.trim_tax_class(&mut head);
    // `(セット) 400 (1) ¥400`: unit price and count in brackets.
    if let &[total] = prices.as_slice()
        && let [.., unit, count] = head[..]
        && head.len() >= 3
        && let (Some(count), Some(unit)) = (count_in_brackets(count), cx.price(unit))
        && quantity_fits(count, unit, total)
    {
        head.truncate(head.len() - 2);
        return Line::Item(Draft {
            text: head.join(" "),
            quantity: Some(count),
            unit: Some(unit),
            total,
            tax: false,
        });
    }
    if let &[total] = prices.as_slice()
        && let Some((quantity, unit)) = take_unit_price_times(&mut head, cx, total)
    {
        return Line::Item(Draft {
            text: head.join(" "),
            quantity: Some(quantity),
            unit: quantity_fits(quantity, unit, total).then_some(unit),
            total,
            tax: false,
        });
    }
    // `メンソレータムエクシ1,480 1,480`: the unit price glued to the name.
    if let &[total] = prices.as_slice()
        && let Some(last) = head.last()
        && let Some((name, amount, "")) = split_glued_amount(last)
        && !name.is_empty()
        && cx.price(amount) == Some(total)
    {
        let at = head.len() - 1;
        head[at] = name;
    }
    // `4 x Coupon 15% Bad Deo Dusche -1,33`: the count says how many
    // articles the discount was given on, not how often it was given.
    if let &[total] = prices.as_slice()
        && total < 0
    {
        return Line::Item(Draft {
            text: head.join(" "),
            quantity: None,
            unit: None,
            total,
            tax: false,
        });
    }
    let marker = take_quantity_suffix(&mut head)
        .or_else(|| take_quantity_prefix(&mut head))
        .or_else(|| take_piece_count(&mut head, &prices));
    // `2x 1,25 Odol Med3 2,50`: dm prints count and unit price in front.
    let leading_unit = match (marker, prices.as_slice(), head.first()) {
        (Some(Marker::Count(q)), &[total], Some(first)) if head.len() >= 2 => cx
            .price(first)
            .filter(|unit| *unit > 0 && quantity_fits(q, *unit, total)),
        _ => None,
    };
    if leading_unit.is_some() {
        head.remove(0);
    }
    let resolved = match (marker, prices.as_slice()) {
        (Some(Marker::Count(q)), &[total]) if leading_unit.is_some() => {
            Some((Some(q), leading_unit, total))
        }
        (Some(Marker::PerUnit(q)), &[unit]) => {
            line_total(q, unit).map(|total| (Some(q), Some(unit), total))
        }
        (Some(Marker::PerUnit(q) | Marker::Count(q)), &[unit, total]) => {
            Some((Some(q), Some(unit), total))
        }
        (Some(Marker::Count(q)), &[total]) => Some((Some(q), exact_unit(total, q), total)),
        (None, &[total]) => Some((None, None, total)),
        (None, &[first, total]) => Some(two_prices(&mut head, tokens[end], first, total, cx)),
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
        tax: false,
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

/// `Mül.Froop 0,39 € x 3 1,17`: unit price, multiplication sign and count
/// between the text and the line total, as Edeka prints them. A count lost
/// in recognition (`… 0,89 € x 1,78`) follows from the total if it divides
/// evenly. Returns count and unit price and removes them from `head`.
fn take_unit_price_times(head: &mut Vec<&str>, cx: &Context, total: i64) -> Option<(Decimal, i64)> {
    let count = head.last().and_then(|token| parse_count(token));
    // `x`, or `€x` with the currency glued on.
    let times = |token: &str| {
        is_times(token)
            || token.char_indices().nth(1).is_some_and(|(split, _)| {
                is_currency_mark(&token[..split], cx.currency) && is_times(&token[split..])
            })
    };
    // Index of the multiplication sign, then of the unit price before it.
    let mut at = head.len() - usize::from(count.is_some());
    if at == 0 || !times(head[at - 1]) {
        return None;
    }
    at -= 1;
    if at > 0 && is_currency_mark(head[at - 1], cx.currency) {
        at -= 1;
    }
    // Some text must stay in front of the unit price.
    if at < 2 {
        return None;
    }
    at -= 1;
    let unit = cx.price(head[at]).filter(|unit| *unit > 0)?;
    let quantity = match count {
        Some(count) => count,
        None => Decimal::from(exact_count(total, unit)?),
    };
    head.truncate(at);
    Some((quantity, unit))
}

/// `total / unit` when it is a whole number of at least 1.
fn exact_count(total: i64, unit: i64) -> Option<i64> {
    (unit > 0 && total % unit == 0 && total / unit > 0).then(|| total / unit)
}

/// Invoices print the name on its own row and below it `1,0 34,99 EUR
/// 34,99 EUR` (quantity, unit price, total). The prices row becomes the
/// item, named by the row above; a leading quantity and unit price that
/// explain the total are kept as such.
fn take_name_from_above(draft: &mut Draft, above: &str, cx: &Context) {
    let tokens: Vec<&str> = draft
        .text
        .split_whitespace()
        .filter(|token| !is_currency_mark(token, cx.currency))
        .collect();
    if draft.quantity.is_none()
        && let [quantity, unit] = tokens.as_slice()
        && let (Some(quantity), Some(unit)) = (parse_quantity(quantity), cx.price(unit))
        && quantity_fits(quantity, unit, draft.total)
    {
        draft.quantity = Some(quantity);
        draft.unit = Some(unit);
    }
    // TEDi prints the VAT class after the name (`Stabkerze A`), the price
    // with the article number below.
    let mut name: Vec<&str> = above.split_whitespace().collect();
    cx.trim_tax_class(&mut name);
    draft.text = name.join(" ");
}

/// Of the text rows right above some prices, the one naming the article:
/// the closest without a code (article number, EAN, serial number), else
/// the closest.
fn name_row(text_above: &[usize], tokens: &[Vec<&str>]) -> Option<usize> {
    let has_code = |row: usize| {
        tokens[row]
            .iter()
            .any(|token| token.chars().filter(char::is_ascii_digit).count() >= CODE_DIGITS)
    };
    text_above
        .iter()
        .rev()
        .find(|&&row| !has_code(row))
        .or_else(|| text_above.last())
        .copied()
}

/// `2 x Cola …`, `2x Cola …` or `1中華そば`: a count in front of the text.
fn take_quantity_prefix(head: &mut Vec<&str>) -> Option<Marker> {
    // A short code may stand in front of the count (`TP 1特セット餃子`).
    let short_code = |token: &str| {
        (1..=3).contains(&token.len()) && token.chars().all(|c| c.is_ascii_uppercase())
    };
    let at = usize::from(head.len() >= 2 && short_code(head[0]));
    if let Some((count, name)) = head.get(at).and_then(|token| count_glued_to_name(token)) {
        head[at] = name;
        return Some(Marker::Count(count));
    }
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

/// `Milch 2 St 1,98`: in German a piece count before the line total means
/// `2 x`. A pack size in the name (`Ersatzfilter 2 Stk 9,99`) reads the
/// same, so the count only holds if it divides the total evenly.
fn take_piece_count(head: &mut Vec<&str>, prices: &[i64]) -> Option<Marker> {
    let &[total] = prices else {
        return None;
    };
    let [.., count, unit] = head[..] else {
        return None;
    };
    // Some text must stay in front of the count; weights have their own rule.
    if head.len() < 3 || !is_piece_word(unit) {
        return None;
    }
    let count = parse_count(count)?.to_i64().filter(|count| *count >= 2)?;
    exact_count(total, count)?;
    head.truncate(head.len() - 2);
    Some(Marker::Count(Decimal::from(count)))
}

/// Two prices without a quantity sign: `1 T-RINDERSTEAK 19.90 19.90` is
/// count, unit price, total. If the prices do not fit together, the first
/// one stays part of the text.
fn two_prices<'a>(
    head: &mut Vec<&'a str>,
    first_token: &'a str,
    unit: i64,
    total: i64,
    cx: &Context,
) -> (Option<Decimal>, Option<i64>, i64) {
    if head.len() >= 2
        && let Some(count) = parse_count(head[0])
        && quantity_fits(count, unit, total)
    {
        head.remove(0);
        return (Some(count), Some(unit), total);
    }
    // `朝食 1 320`: in yen the first number may be the count. Of count and
    // unit price, the count is the smaller one (`2 600` vs. `300 600`).
    // Without a name in front, a number is more likely a department code.
    if cx.currency.exponent() == 0
        && !head.is_empty()
        && parse_count(first_token).is_some()
        && unit > 0
        && total % unit == 0
        && unit.checked_mul(unit).is_some_and(|square| square <= total)
    {
        return (Some(Decimal::from(unit)), Some(total / unit), total);
    }
    let department_code = cx.currency.exponent() == 0 && head.is_empty();
    if !department_code && unit != 0 && total % unit == 0 && total / unit > 0 {
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
    /// Added tax printed before the first sum; it only becomes an item if
    /// it explains the total.
    early_taxes: Vec<(usize, Draft)>,
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
            early_taxes: Vec::new(),
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
            (Zone::Items, Line::Header) => self.drop_items(),
            (Zone::Items, Line::Item(draft)) if draft.tax => {
                self.kinds[row] = RowKind::Tax;
                self.early_taxes.push((row, draft));
            }
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
        let kind = item_kind(&draft);
        self.items.push(ParsedItem {
            text: draft.text,
            quantity,
            unit_price: unit.map(|u| Money::new(u, self.currency)),
            total_price: Money::new(draft.total, self.currency),
            kind,
            rows: vec![row],
            attached: false,
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

    /// Forgets everything read as items so far: it was the shop's header.
    fn drop_items(&mut self) {
        for item in self.items.drain(..) {
            for row in item.rows {
                self.kinds[row] = RowKind::Other;
            }
        }
        self.close_items();
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
        // Added tax that did not explain the step between two sums.
        let mut taxes: Vec<(usize, Draft)> = std::mem::take(&mut self.early_taxes);
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
                    taxes.extend(between.drain(..).filter(|(_, draft)| draft.tax));
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
            self.add_taxes_if_they_explain(amount, row, taxes)?;
        }
        Ok(())
    }

    /// `外税 10% ¥33` and `外税 8% ¥24` printed between several subtotals
    /// (one per tax rate): they are items if, added to the items, they give
    /// the total.
    fn add_taxes_if_they_explain(
        &mut self,
        total: i64,
        total_row: usize,
        mut taxes: Vec<(usize, Draft)>,
    ) -> Result<(), ReceiptError> {
        taxes.retain(|(row, _)| *row < total_row);
        taxes.truncate(MAX_TAX_CANDIDATES);
        if taxes.is_empty() {
            return Ok(());
        }
        let items = self
            .items
            .iter()
            .map(|item| item.total_price.amount_minor())
            .try_fold(0_i64, i64::checked_add)
            .ok_or(ReceiptError::Overflow)?;
        let missing = total.checked_sub(items).ok_or(ReceiptError::Overflow)?;
        // Tills print the tax per rate and again as a sum (`外8% ¥111`,
        // `外税計 ¥111`): the fewest rows that explain the total win, and
        // only if every such choice adds the same amounts.
        let amounts = |mask: u32| -> Vec<i64> {
            let mut picked: Vec<i64> = (0..taxes.len())
                .filter(|bit| mask & (1 << bit) != 0)
                .map(|bit| taxes[bit].1.total)
                .collect();
            picked.sort_unstable();
            picked
        };
        let fitting: Vec<u32> = (1_u32..(1 << taxes.len()))
            .filter(|&mask| amounts(mask).iter().sum::<i64>() == missing)
            .collect();
        let Some(fewest) = fitting.iter().map(|mask| mask.count_ones()).min() else {
            return Ok(());
        };
        let choices: Vec<u32> = fitting
            .into_iter()
            .filter(|mask| mask.count_ones() == fewest)
            .collect();
        if choices
            .iter()
            .any(|&mask| amounts(mask) != amounts(choices[0]))
        {
            return Ok(());
        }
        for (bit, (row, draft)) in taxes.into_iter().enumerate() {
            if choices[0] & (1 << bit) != 0 {
                self.push_item(row, draft);
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
    fn name_above_article_and_serial_numbers() {
        // Smyths: name between article number and serial number, prices
        // in a row of their own below.
        let r = receipt(&[
            "Steuernummer: DE325778210",
            "EUR",
            "Art/EAN 249691",
            "Nintendo Switch 2 Konsol",
            "Seriennr.: HAE10473970120",
            "1 × 469,99 469,99 G",
            "Art/EAN 8044881",
            "SMYTHS TOYS TRAGETASCHE",
            "1 × 1,00 1,00 G",
            "SUMME [2] EUR 470,99",
        ]);
        assert_eq!(
            items(&r),
            [
                ("Nintendo Switch 2 Konsol", dec("1"), Some(46999), 46999),
                ("SMYTHS TOYS TRAGETASCHE", dec("1"), Some(100), 100)
            ]
        );
        assert_eq!(r.items[0].rows, [3, 5]);
        assert_eq!(r.check, TotalCheck::Matches);

        // A quantity row without its own total still waits for the item
        // below it.
        let r = receipt(&["BÄCKEREI", "2 x 0,49", "Joghurt 0,98", "Summe 0,98"]);
        assert_eq!(items(&r), [("Joghurt", dec("2"), Some(49), 98)]);
    }

    #[test]
    fn speck_before_the_total_word() {
        let r = receipt(&[
            "Filter 9,99",
            "SUMME EUR 9,99",
            "MwSt.-Senkung -0,88",
            "E Summe EUR 9,11",
        ]);
        assert_eq!(r.total.map(|m| m.amount_minor()), Some(911));
        assert_eq!(r.check, TotalCheck::Matches);
        // A one-letter word is not ignored in front of other text.
        let r = receipt(&["A Milch 1,00", "Summe 1,00"]);
        assert_eq!(items(&r), [("A Milch", Decimal::ONE, Some(100), 100)]);
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
    fn unit_price_before_the_count() {
        // Edeka: unit price, `€ x`, count, total; recognition sometimes
        // loses the count or glues `€x` together.
        let r = receipt(&[
            "Mül.Froop 0,39 € x 3 1,17 AW",
            "G&G Rahmspina 0,89 € x 1,78 A",
            "B10E H-Milch 1,15 €X 4 4,60 A",
            "Pfand 0,15*A",
            "Leergut -0,25*B",
            "Summe 7,45",
        ]);
        assert_eq!(
            items(&r),
            [
                ("Mül.Froop", dec("3"), Some(39), 117),
                ("G&G Rahmspina", dec("2"), Some(89), 178),
                ("B10E H-Milch", dec("4"), Some(115), 460),
                ("Pfand", Decimal::ONE, Some(15), 15),
                ("Leergut", Decimal::ONE, Some(-25), -25),
            ]
        );
        assert_eq!(r.items[3].kind, ItemKind::Deposit);
        assert_eq!(r.items[4].kind, ItemKind::Discount);
        assert_eq!(r.check, TotalCheck::Matches);
    }

    #[test]
    fn invoice_with_the_name_above_its_prices() {
        let r = receipt(&[
            "Rechnung",
            "Datum: 02.10.2026",
            "1 Nintendo Captain Toad",
            "1,0 34,99 EUR 34,99 EUR",
            "Netto: 29,40 EUR",
            "19,00% MWSt: 5,59 EUR",
            "Endbetrag: 34,99 EUR",
        ]);
        assert_eq!(
            items(&r),
            [("1 Nintendo Captain Toad", dec("1.0"), Some(3499), 3499)]
        );
        assert_eq!(r.items[0].rows, [2, 3]);
        assert_eq!(r.rows[2].kind, RowKind::Item);
        assert_eq!(r.rows[4].kind, RowKind::Tax);
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
    fn piece_count_before_the_total() {
        let r = receipt(&[
            "Milch 2 St 1,98",
            "Joghurt 3 Stk. 1,47 A",
            "Ersatzfilter 2 Stk 9,99",
            "Kartoffeln 2 kg 3,98",
            "Summe 17,42",
        ]);
        assert_eq!(
            items(&r),
            [
                ("Milch", dec("2"), Some(99), 198),
                ("Joghurt", dec("3"), Some(49), 147),
                ("Ersatzfilter 2 Stk", Decimal::ONE, Some(999), 999),
                ("Kartoffeln 2 kg", Decimal::ONE, Some(398), 398),
            ]
        );
        assert_eq!(r.check, TotalCheck::Matches);
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

    fn item_kinds(receipt: &ParsedReceipt) -> Vec<ItemKind> {
        receipt.items.iter().map(|item| item.kind).collect()
    }

    #[test]
    fn deposit_returns_and_vouchers() {
        let r = receipt(&[
            "Wasser 6 x 0,49 2,94",
            "Pfand 6 x 0,25 1,50 A",
            "EW-Pfand 0,25",
            "Wasser inkl. Pfand 0,99",
            "Leergut -3,00",
            "Pfandrückgabe -0,25",
            "Coupon -1,00",
            "Summe 1,43",
        ]);
        use ItemKind::*;
        assert_eq!(
            item_kinds(&r),
            [
                Article, Deposit, Deposit, Article, Discount, Discount, Discount
            ]
        );
        assert_eq!(r.items[1].quantity, dec("6"));
        assert_eq!(r.check, TotalCheck::Matches);
    }

    #[test]
    fn returns_printed_without_minus_subtract_when_the_total_says_so() {
        let r = receipt(&[
            "Cola 1,99",
            "Pfand 0,25",
            "Leergut 0,75",
            "Gutschein 1,00",
            "Summe 0,49",
        ]);
        use ItemKind::*;
        assert_eq!(item_kinds(&r), [Article, Deposit, Discount, Discount]);
        assert_eq!(
            items(&r)[2..],
            [
                ("Leergut", Decimal::ONE, Some(-75), -75),
                ("Gutschein", Decimal::ONE, Some(-100), -100)
            ]
        );
        assert_eq!(r.check, TotalCheck::Matches);
        // A bought voucher stays an article: subtracting it does not fit.
        let r = receipt(&["Gutschein 25,00", "Summe 25,00"]);
        assert_eq!(item_kinds(&r), [Article]);
        // Without a total nothing tells, so nothing changes.
        let r = receipt(&["Cola 1,99", "Leergut 0,25"]);
        assert_eq!(item_kinds(&r), [Article, Article]);
    }

    #[test]
    fn dm_layout_tax_digit_leading_unit_and_coupon_counts() {
        let r = receipt(&[
            "Papiertragetasche 0,20 1",
            "2x 3,75 always Ult. Binden 7,50 2",
            "3x 0,95 WC-Frisch 2,85",
            "Zwischensumme 10,55",
            "4 x Coupon 15% Bad -1,33",
            "SUMME EUR 9,22",
        ]);
        assert_eq!(
            items(&r),
            [
                ("Papiertragetasche", Decimal::ONE, Some(20), 20),
                ("always Ult. Binden", dec("2"), Some(375), 750),
                ("WC-Frisch", dec("3"), Some(95), 285),
                ("4 x Coupon 15% Bad", Decimal::ONE, Some(-133), -133),
            ]
        );
        assert_eq!(r.check, TotalCheck::Matches);
        // A lone digit stays an amount where there are no minor units.
        let r = yen(&["お茶 2", "合計 2"]);
        assert_eq!(r.items.len(), 1);
    }

    #[test]
    fn price_with_its_tax_class_read_as_a_digit_is_recovered_by_the_total() {
        let r = receipt(&[
            "Chips 1,19 B",
            "Leckermaeulchen 0,990",
            "Rabatt -0,20",
            "Summe 1,98",
        ]);
        assert_eq!(
            items(&r)[1],
            ("Leckermaeulchen", Decimal::ONE, Some(99), 99)
        );
        assert_eq!(r.check, TotalCheck::Matches);
        // A row ending in a multi-byte character is no candidate (and
        // does not break the cut).
        let r = receipt(&["Chips 1,19 B", "Gutschein 0,99 €", "Summe 1,98"]);
        assert_eq!(r.items.len(), 2);
        let r = receipt(&["Chips 1,19 B", "Hinweis 0,9円", "Summe 2,18"]);
        assert_eq!(r.items.len(), 1);
        // A doubled separator, the tax class after it.
        let r = receipt(&["Chips 1,19 B", "Spinat Feta 0.,69 B", "Summe 1,88"]);
        assert_eq!(items(&r)[1], ("Spinat Feta", Decimal::ONE, Some(69), 69));
        // Without the total saying so, nothing is guessed.
        let r = receipt(&["Chips 1,19 B", "Leckermaeulchen 0,990", "Summe 1,19"]);
        assert_eq!(r.items.len(), 1);
    }

    #[test]
    fn tax_added_on_top_of_a_subtotal() {
        let r = receipt_in(
            &[
                "SOCKS $5.99",
                "HOODIE $14.99",
                "SUBTOTAL $20.98",
                "TAX 6% $1.26",
                "TOTAL $22.24",
            ],
            "USD",
        );
        assert_eq!(r.items.len(), 3);
        assert_eq!(r.items[2].kind, ItemKind::Tax);
        assert_eq!(r.check, TotalCheck::Matches);
        // Included VAT stays out.
        let r = receipt(&["Brot 1,99", "SUMME 1,99", "ENTH. MwSt 7% 0,13"]);
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.check, TotalCheck::Matches);
    }

    #[test]
    fn deposits_and_discounts_belong_to_the_article_above() {
        let r = receipt(&[
            "Wasser 1,99",
            "Pfand 0,25",
            "Rabatt -0,20",
            "Leergut -0,25",
            "Brot 1,00",
            "Summe 2,79",
            "Coupon -0,10",
            "Summe 2,69",
        ]);
        let attached: Vec<(&str, bool)> = r
            .items
            .iter()
            .map(|item| (item.text.as_str(), item.attached))
            .collect();
        assert_eq!(
            attached,
            [
                ("Wasser", false),
                ("Pfand", true),
                ("Rabatt", true),
                ("Leergut", false),
                ("Brot", false),
                ("Coupon", false),
            ]
        );
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
        let r = receipt_in(&["中華そば ¥748", "特選セットA 1,120円"], "JPY");
        assert_eq!(
            items(&r),
            [
                ("中華そば", Decimal::ONE, Some(748), 748),
                ("特選セットA", Decimal::ONE, Some(1120), 1120)
            ]
        );
    }

    fn yen(rows: &[&str]) -> ParsedReceipt {
        receipt_in(rows, "JPY")
    }

    #[test]
    fn japanese_sums_payment_and_change() {
        let r = yen(&[
            "おにぎり ¥150",
            "お茶 ¥130",
            "小 計 ¥280",
            "(内消費税等 ¥20)",
            "合計 ¥280",
            "お預り合計 ¥500",
            "お釣り ¥220",
        ]);
        assert_eq!(r.items.len(), 2);
        assert_eq!(r.subtotal.map(|m| m.amount_minor()), Some(280));
        assert_eq!(r.total.map(|m| m.amount_minor()), Some(280));
        assert_eq!(r.tendered.map(|m| m.amount_minor()), Some(500));
        assert_eq!(r.change.map(|m| m.amount_minor()), Some(220));
        // `¥20)` with its bracket is no amount; the row is informational.
        use RowKind::*;
        assert_eq!(
            kinds(&r),
            [Item, Item, Subtotal, Other, Total, Payment, Change]
        );
    }

    #[test]
    fn japanese_quantity_layouts() {
        let r = yen(&[
            "品名 単価 数量 金額",
            "コーラ 150 2 300",
            "朝食 1 320",
            "2牛丼 800",
            "ビール 500 (2コ) ¥1,000",
            "合計 ¥2,420",
        ]);
        assert_eq!(
            items(&r),
            [
                ("コーラ", dec("2"), Some(150), 300),
                ("朝食", dec("1"), Some(320), 320),
                ("牛丼", dec("2"), Some(400), 800),
                ("ビール", dec("2"), Some(500), 1000),
            ]
        );
        assert_eq!(r.check, TotalCheck::Matches);
        // The bigger number of two is the total; `300 600` is 2 × 300.
        let r = yen(&["お茶 300 600", "合計 600"]);
        assert_eq!(items(&r), [("お茶", dec("2"), Some(300), 600)]);
    }

    #[test]
    fn rows_above_the_table_header_are_no_items() {
        let r = yen(&[
            "伝票No 985",
            "人数= 2",
            "数 メニュー 金額",
            "1中華そば 748",
            "合計 748",
        ]);
        assert_eq!(items(&r), [("中華そば", dec("1"), Some(748), 748)]);
        assert_eq!(r.rows[1].kind, RowKind::Other);
        // Without a header nothing is dropped.
        let r = yen(&["人数= 2", "合計 2"]);
        assert_eq!(r.items.len(), 1);
    }

    #[test]
    fn article_count_rows_are_no_amounts() {
        let r = yen(&["パン ¥150", "合計点数 1点", "点 数 1個", "合計 1点 ¥150"]);
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.total.map(|m| m.amount_minor()), Some(150));
        use RowKind::*;
        assert_eq!(kinds(&r), [Item, Other, Other, Total]);
    }

    #[test]
    fn added_tax_becomes_an_item_when_it_explains_the_total() {
        let r = yen(&[
            "弁当 ¥500",
            "小計 ¥500",
            "外税8%対象額 ¥500",
            "外税 8% ¥40",
            "合計 ¥540",
        ]);
        assert_eq!(r.items.len(), 2);
        assert_eq!(r.items[1].kind, ItemKind::Tax);
        assert_eq!(r.items[1].total_price.amount_minor(), 40);
        assert_eq!(r.rows[2].kind, RowKind::Tax);
        assert_eq!(r.check, TotalCheck::Matches);
        // Tax that does not explain the total stays out.
        let r = yen(&["弁当 ¥500", "小計 ¥500", "外税 ¥40", "合計 ¥500"]);
        assert_eq!(r.items.len(), 1);
        // `外税 0` adds nothing and stays a tax row.
        let r = yen(&["弁当 500", "外税 0", "合計 500"]);
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.rows[1].kind, RowKind::Tax);
    }

    #[test]
    fn tax_in_the_total_is_no_total() {
        let r = yen(&[
            "天然水 ¥116",
            "合計/ 1点 ¥116",
            "(税合計 ¥10",
            "お預り ¥200",
        ]);
        assert_eq!(r.total.map(|m| m.amount_minor()), Some(116));
        assert_eq!(r.rows[2].kind, RowKind::Tax);
        let r = yen(&["弁当 ¥500", "税込合計 ¥540"]);
        assert_eq!(r.total.map(|m| m.amount_minor()), Some(540));
    }

    #[test]
    fn sum_word_takes_a_lone_amount_from_the_next_row() {
        let r = yen(&["牛丼 712", "712", "合計"]);
        assert_eq!(r.total.map(|m| m.amount_minor()), Some(712));
        assert_eq!(r.rows[1].kind, RowKind::Total);
        assert_eq!(r.rows[2].kind, RowKind::Total);
        let r = receipt(&["Brot 3,20", "SUMME", "EUR 3,20"]);
        assert_eq!(r.total.map(|m| m.amount_minor()), Some(320));
        assert_eq!(r.check, TotalCheck::Matches);
    }

    #[test]
    fn yen_sign_read_as_one_is_repaired_against_the_total() {
        let r = yen(&["2 3201", "2 300", "620 計"]);
        assert_eq!(
            items(&r),
            [
                ("2", dec("1"), Some(320), 320),
                ("2", dec("1"), Some(300), 300)
            ]
        );
        assert_eq!(r.check, TotalCheck::Matches);
        // Two ways to reach the total: nothing is changed.
        let r = yen(&["A 3201", "B 3201", "3500 計"]);
        assert_eq!(r.items_sum().unwrap().amount_minor(), 6402);
        // Not for currencies with minor units.
        let r = receipt(&["Brot 32,01", "Summe 3,20"]);
        assert_eq!(r.items[0].total_price.amount_minor(), 3201);
    }
}
