//! Receipt parser: recognized text → line items (idee.md 5.6, OCR-10..14).
//!
//! Engine-independent: any OCR engine delivers [`RecognizedText`] boxes,
//! [`parse_receipt`] rebuilds the printed rows and turns them into
//! [`ParsedItem`]s plus the printed total, so the review screen can show
//! whether both agree. The rules follow German till receipts plus the
//! Japanese specifics of OCR-17 (`小計`, `合計`, `お釣り`, `内税`/`外税`,
//! counts glued to names, amounts without minor units).

mod language;
mod parse;
mod rows;
mod tokens;

pub use language::detect_language;
pub use parse::parse_receipt;

/// The printed rows as plain text, top to bottom, rebuilt the same way the
/// parser sees them; for archiving the raw OCR text (idee.md 4.1
/// `ocr_raw_text`).
pub fn text_rows(fragments: &[RecognizedText]) -> Vec<String> {
    rows::group_rows(fragments)
        .into_iter()
        .map(|row| row.text)
        .collect()
}

use rust_decimal::Decimal;
use thiserror::Error;

use crate::domain::{Currency, Money};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReceiptError {
    #[error("amount out of range")]
    Overflow,
}

/// Axis-aligned box in image pixels; `right`/`bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BoundingBox {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl BoundingBox {
    /// Smallest box containing both.
    pub fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    fn height(&self) -> i64 {
        i64::from(self.bottom) - i64::from(self.top)
    }

    /// Twice the vertical centre, so it stays an integer.
    fn center_y2(&self) -> i64 {
        i64::from(self.top) + i64::from(self.bottom)
    }
}

/// One piece of text as an OCR engine reports it: a box and its content.
/// A box may hold a whole printed row or only part of it (name, price).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecognizedText {
    pub text: String,
    pub bbox: BoundingBox,
}

/// What a reconstructed row was recognized as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Article, discount or deposit: becomes a [`ParsedItem`].
    Item,
    /// Quantity and unit price of the item above or below (`2 X 0,49`).
    Quantity,
    Subtotal,
    Total,
    /// VAT rows; prices on German receipts include VAT, so they are
    /// informational only.
    Tax,
    /// Amount handed over or charged (`Bar`, `Karte`, `Gegeben`).
    Payment,
    /// Change given back (`Rückgeld`).
    Change,
    /// Anything else: header, address, footer, unreadable rows.
    Other,
}

/// A printed row rebuilt from the OCR boxes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptRow {
    pub text: String,
    pub bbox: BoundingBox,
    pub kind: RowKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Article,
    /// Negative amount, e.g. a discount or a returned deposit.
    Discount,
    /// Tax added on top of the prices (`外税`), shared like the rest of
    /// the bill (idee.md 8.2 step 4).
    Tax,
}

/// One position of the receipt (later a `LineItem`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedItem {
    /// Text as printed, without quantity and prices.
    pub text: String,
    /// Count or weight; 1 unless the receipt states otherwise.
    pub quantity: Decimal,
    /// Known when printed or when the quantity is 1.
    pub unit_price: Option<Money>,
    pub total_price: Money,
    pub kind: ItemKind,
    /// Indices into [`ParsedReceipt::rows`] this item was read from.
    pub rows: Vec<usize>,
}

/// Result of the plausibility check (OCR-14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TotalCheck {
    /// The items add up to the printed total.
    Matches,
    /// They do not; `difference = total − items_sum`.
    Differs { items_sum: Money, difference: Money },
    /// No total was found on the receipt.
    NoTotal,
}

/// Everything read from one receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedReceipt {
    pub currency: Currency,
    pub rows: Vec<ReceiptRow>,
    pub items: Vec<ParsedItem>,
    pub subtotal: Option<Money>,
    pub total: Option<Money>,
    /// First amount handed over or charged.
    pub tendered: Option<Money>,
    pub change: Option<Money>,
    pub check: TotalCheck,
}

impl ParsedReceipt {
    /// Sum of all items, discounts included.
    pub fn items_sum(&self) -> Result<Money, ReceiptError> {
        self.items
            .iter()
            .try_fold(Money::zero(self.currency), |sum, item| {
                sum.checked_add(item.total_price)
            })
            .map_err(|_| ReceiptError::Overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_rows_join_boxes_of_one_printed_row() {
        let text = |text: &str, left, top| RecognizedText {
            text: text.to_string(),
            bbox: BoundingBox {
                left,
                top,
                right: left + 80,
                bottom: top + 20,
            },
        };
        let fragments = [
            text("SUMME", 10, 60),
            text("0,99", 300, 31),
            text("Brot", 10, 30),
            text("  ", 10, 90),
        ];
        assert_eq!(text_rows(&fragments), ["Brot 0,99", "SUMME"]);
        assert!(text_rows(&[]).is_empty());
    }
}
