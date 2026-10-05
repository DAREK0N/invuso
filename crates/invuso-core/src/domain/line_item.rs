use std::collections::BTreeMap;

use rust_decimal::Decimal;
use thiserror::Error;

use super::PersonId;
use crate::split::{ItemLine, SplitError, allocate};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LineItemError {
    #[error("unknown line item kind `{0}`")]
    UnknownKind(String),
    #[error("the quantity must be greater than zero")]
    NonPositiveQuantity,
    #[error("the amount has the wrong sign for its kind")]
    WrongSign,
    #[error("a person's share of a line item must not be negative")]
    NegativeWeight,
    #[error("a line item cannot be split any further")]
    CannotSplit,
    #[error(transparent)]
    Split(#[from] SplitError),
}

/// What a receipt line is (idee.md 4.1 `LineItem.kind`) and how it counts
/// when splitting (idee.md 8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LineItemKind {
    #[default]
    Article,
    /// Discount, voucher or returned deposit: subtracts.
    Discount,
    /// Deposit paid on top of an article.
    Deposit,
    /// Tax added on top of the prices.
    Tax,
    Tip,
    ServiceCharge,
    /// A printed subtotal or anything else that must not count.
    Ignored,
}

impl LineItemKind {
    pub const ALL: [Self; 7] = [
        Self::Article,
        Self::Discount,
        Self::Deposit,
        Self::Tax,
        Self::Tip,
        Self::ServiceCharge,
        Self::Ignored,
    ];

    /// Stable code stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            Self::Article => "article",
            Self::Discount => "discount",
            Self::Deposit => "deposit",
            Self::Tax => "tax",
            Self::Tip => "tip",
            Self::ServiceCharge => "service_charge",
            Self::Ignored => "ignored",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, LineItemError> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.code() == code)
            .ok_or_else(|| LineItemError::UnknownKind(code.to_string()))
    }

    /// Lines that belong to someone (idee.md 8.2 steps 1–3). Tax, tip and
    /// service charge are shared proportionally instead (step 4).
    pub fn is_assignable(self) -> bool {
        matches!(self, Self::Article | Self::Discount | Self::Deposit)
    }

    /// Whether the line adds up to the receipt's total at all.
    pub fn counts(self) -> bool {
        self != Self::Ignored
    }

    /// Discounts subtract, everything else adds; amounts are entered
    /// without a sign (user decision in AP-19).
    pub fn signed(self, magnitude: i64) -> i64 {
        if self == Self::Discount {
            -magnitude.abs()
        } else {
            magnitude.abs()
        }
    }
}

/// One position of an expense, read from its receipt or entered by hand
/// (idee.md 4.1 `LineItem`). Amounts are minor units of the expense's
/// currency and carry the sign of their kind.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LineItem {
    /// As printed or first typed.
    pub original_text: String,
    pub translated_text: Option<String>,
    /// The user's correction; wins over everything else.
    pub user_text: Option<String>,
    /// Count or weight, e.g. `0.452` kg.
    pub quantity: Decimal,
    pub unit_price_minor: Option<i64>,
    pub total_minor: i64,
    pub kind: LineItemKind,
    /// Who carries the line, with weights (`6 × Bier` → 2 : 1 : 3). Empty
    /// = everyone taking part in the expense (idee.md 8.2).
    pub assigned_to: BTreeMap<PersonId, Decimal>,
    /// Mean OCR confidence, 0–1, for lines read from a receipt.
    pub ocr_confidence: Option<f32>,
    pub edited_by_user: bool,
}

impl LineItem {
    /// The text to show: correction, else translation, else original.
    pub fn text(&self) -> &str {
        self.user_text
            .as_deref()
            .or(self.translated_text.as_deref())
            .unwrap_or(&self.original_text)
    }

    /// Checks quantity, sign and weights.
    pub fn validate(&self) -> Result<(), LineItemError> {
        if self.quantity <= Decimal::ZERO {
            return Err(LineItemError::NonPositiveQuantity);
        }
        let negative = self.total_minor < 0;
        if negative != (self.kind == LineItemKind::Discount) && self.total_minor != 0 {
            return Err(LineItemError::WrongSign);
        }
        if self
            .assigned_to
            .values()
            .any(|w| w.is_sign_negative() && !w.is_zero())
        {
            return Err(LineItemError::NegativeWeight);
        }
        Ok(())
    }

    /// Splits the line in two (OCR-31): with a whole quantity of at least
    /// 2, one unit comes off (`3 × Bier` → `2 × Bier` + `1 × Bier`), so it
    /// can belong to someone else; otherwise the line is halved. The rest
    /// unit goes to the first part (idee.md 8.4). Both keep the assignment.
    pub fn split(&self) -> Result<(LineItem, LineItem), LineItemError> {
        let whole = self.quantity.fract().is_zero() && self.quantity >= Decimal::TWO;
        let (rest_quantity, part_quantity) = if whole {
            (self.quantity - Decimal::ONE, Decimal::ONE)
        } else {
            let half = self.quantity / Decimal::TWO;
            (half, half)
        };
        if part_quantity.is_zero() || self.total_minor.abs() < 2 {
            return Err(LineItemError::CannotSplit);
        }
        let weights = BTreeMap::from([(0, rest_quantity), (1, part_quantity)]);
        let parts = allocate(self.total_minor, &weights)?;
        let piece = |quantity: Decimal, total: i64| LineItem {
            quantity,
            total_minor: total,
            edited_by_user: true,
            ..self.clone()
        };
        Ok((
            piece(rest_quantity, parts[&0]),
            piece(part_quantity, parts[&1]),
        ))
    }

    /// Joins this line with `next` (OCR-31), e.g. an article and its
    /// deposit. The same article at the same price adds up the quantity;
    /// anything else becomes one line of quantity 1 with both texts. The
    /// first line's assignment wins.
    pub fn merge(&self, next: &LineItem) -> Result<LineItem, LineItemError> {
        let total = self
            .total_minor
            .checked_add(next.total_minor)
            .ok_or(SplitError::Overflow)?;
        let same = self.text() == next.text()
            && self.kind == next.kind
            && self.unit_price_minor.is_some()
            && self.unit_price_minor == next.unit_price_minor;
        let kind = if total < 0 {
            LineItemKind::Discount
        } else if self.kind == LineItemKind::Discount {
            next.kind
        } else {
            self.kind
        };
        let joined = |a: &str, b: &str| format!("{a} + {b}");
        Ok(if same {
            LineItem {
                quantity: self.quantity + next.quantity,
                total_minor: total,
                edited_by_user: true,
                ..self.clone()
            }
        } else {
            LineItem {
                original_text: joined(&self.original_text, &next.original_text),
                translated_text: match (&self.translated_text, &next.translated_text) {
                    (Some(a), Some(b)) => Some(joined(a, b)),
                    _ => None,
                },
                user_text: (self.user_text.is_some() || next.user_text.is_some())
                    .then(|| joined(self.text(), next.text())),
                quantity: Decimal::ONE,
                unit_price_minor: None,
                total_minor: kind.signed(total),
                kind,
                assigned_to: self.assigned_to.clone(),
                ocr_confidence: match (self.ocr_confidence, next.ocr_confidence) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                },
                edited_by_user: true,
            }
        })
    }
}

/// The lines that are split by assignment (idee.md 8.2 steps 1–3), as
/// [`split_by_items`](crate::split::split_by_items) takes them. Tax, tip,
/// service charge and the rest up to the total are left out: they are
/// shared proportionally (step 4).
pub fn item_lines(items: &[LineItem]) -> Vec<ItemLine> {
    items
        .iter()
        .filter(|item| item.kind.is_assignable())
        .map(|item| ItemLine {
            amount_minor: item.total_minor,
            assigned_to: item.assigned_to.clone(),
        })
        .collect()
}

/// Sum of all lines that count towards the total (OCR-32).
pub fn line_items_sum(items: &[LineItem]) -> Result<i64, LineItemError> {
    items
        .iter()
        .filter(|item| item.kind.counts())
        .try_fold(0_i64, |sum, item| sum.checked_add(item.total_minor))
        .ok_or(LineItemError::Split(SplitError::Overflow))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn d(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn item(text: &str, quantity: &str, unit: Option<i64>, total: i64) -> LineItem {
        LineItem {
            original_text: text.into(),
            quantity: d(quantity),
            unit_price_minor: unit,
            total_minor: total,
            ..LineItem::default()
        }
    }

    #[test]
    fn codes_round_trip() {
        for kind in LineItemKind::ALL {
            assert_eq!(LineItemKind::from_code(kind.code()), Ok(kind));
        }
        assert!(LineItemKind::from_code("subtotal").is_err());
    }

    #[test]
    fn sign_follows_the_kind() {
        assert_eq!(LineItemKind::Discount.signed(50), -50);
        assert_eq!(LineItemKind::Discount.signed(-50), -50);
        assert_eq!(LineItemKind::Deposit.signed(-25), 25);

        let mut discount = item("Rabatt", "1", None, -50);
        discount.kind = LineItemKind::Discount;
        assert_eq!(discount.validate(), Ok(()));
        discount.total_minor = 50;
        assert_eq!(discount.validate(), Err(LineItemError::WrongSign));
        assert_eq!(
            item("Bier", "1", None, -50).validate(),
            Err(LineItemError::WrongSign)
        );
        assert_eq!(
            item("Bier", "0", None, 50).validate(),
            Err(LineItemError::NonPositiveQuantity)
        );
        let mut negative = item("Bier", "1", None, 50);
        negative.assigned_to.insert(PersonId::from("a"), d("-1"));
        assert_eq!(negative.validate(), Err(LineItemError::NegativeWeight));
    }

    #[test]
    fn text_prefers_the_correction() {
        let mut line = item("ビール", "1", None, 500);
        assert_eq!(line.text(), "ビール");
        line.translated_text = Some("Bier".into());
        assert_eq!(line.text(), "Bier");
        line.user_text = Some("Asahi".into());
        assert_eq!(line.text(), "Asahi");
    }

    #[test]
    fn split_takes_off_one_unit() {
        let beer = item("Bier", "3", Some(450), 1350);
        let (rest, one) = beer.split().unwrap();
        assert_eq!((rest.quantity, rest.total_minor), (d("2"), 900));
        assert_eq!((one.quantity, one.total_minor), (d("1"), 450));
        assert_eq!(one.unit_price_minor, Some(450));
        assert!(one.edited_by_user);
    }

    #[test]
    fn split_halves_single_units_and_weights() {
        // Rest cent to the first half (idee.md 8.4).
        let pizza = item("Pizza", "1", Some(999), 999);
        let (a, b) = pizza.split().unwrap();
        assert_eq!((a.total_minor, b.total_minor), (500, 499));
        assert_eq!((a.quantity, b.quantity), (d("0.5"), d("0.5")));

        let cheese = item("Käse", "0.452", None, 180);
        let (a, b) = cheese.split().unwrap();
        assert_eq!(a.total_minor + b.total_minor, 180);

        let mut discount = item("Rabatt", "1", None, -51);
        discount.kind = LineItemKind::Discount;
        let (a, b) = discount.split().unwrap();
        assert_eq!((a.total_minor, b.total_minor), (-26, -25));

        assert_eq!(
            item("Kaugummi", "1", None, 1).split(),
            Err(LineItemError::CannotSplit)
        );
    }

    #[test]
    fn merge_same_article_adds_quantity() {
        let a = item("Bier", "2", Some(450), 900);
        let b = item("Bier", "1", Some(450), 450);
        let merged = a.merge(&b).unwrap();
        assert_eq!((merged.quantity, merged.total_minor), (d("3"), 1350));
        assert_eq!(merged.original_text, "Bier");
    }

    #[test]
    fn merge_different_lines_joins_the_texts() {
        let mut water = item("Wasser", "1", Some(79), 79);
        water
            .assigned_to
            .insert(PersonId::from("ben"), Decimal::ONE);
        let deposit = LineItem {
            kind: LineItemKind::Deposit,
            ..item("Pfand", "1", Some(25), 25)
        };
        let merged = water.merge(&deposit).unwrap();
        assert_eq!(merged.original_text, "Wasser + Pfand");
        assert_eq!(merged.user_text, None);
        assert_eq!((merged.quantity, merged.unit_price_minor), (d("1"), None));
        assert_eq!(merged.total_minor, 104);
        assert_eq!(merged.kind, LineItemKind::Article);
        assert_eq!(merged.assigned_to, water.assigned_to);

        // A discount larger than the article stays a discount.
        let discount = LineItem {
            kind: LineItemKind::Discount,
            ..item("Gutschein", "1", None, -100)
        };
        let merged = water.merge(&discount).unwrap();
        assert_eq!(
            (merged.kind, merged.total_minor),
            (LineItemKind::Discount, -21)
        );
        merged.validate().unwrap();
    }

    #[test]
    fn only_assignable_lines_are_split_by_item() {
        let tax = LineItem {
            kind: LineItemKind::Tax,
            ..item("MwSt", "1", None, 190)
        };
        let subtotal = LineItem {
            kind: LineItemKind::Ignored,
            ..item("Zwischensumme", "1", None, 1000)
        };
        let items = [item("Brot", "1", None, 1000), tax, subtotal];
        let lines = item_lines(&items);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].amount_minor, 1000);
        assert_eq!(line_items_sum(&items), Ok(1190));
        assert_eq!(line_items_sum(&[]), Ok(0));
    }
}
