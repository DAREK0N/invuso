//! The receipt archive (RCP-09): every receipt across groups with what was
//! read from it, searchable by merchant and recognized text.

use invuso_core::domain::Currency;
use invuso_core::receipt::{detect_currency, parse_receipt, text_rows};

use crate::storage::{ArchivedReceipt, Db, StorageError};

/// A receipt as the archive shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub receipt: ArchivedReceipt,
    /// The shop as printed in the header (OCR-16); `None` until analyzed or
    /// if none was found.
    pub merchant: Option<String>,
}

impl ArchiveEntry {
    /// Waits for the user: no expense was made from it yet (RCP-07).
    pub fn is_open(&self) -> bool {
        self.receipt.expense.is_none()
    }

    /// Whether `query` (any case) is in the merchant, the expense's title
    /// or the recognized text; a blank query matches everything.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        [
            self.merchant.as_deref(),
            self.receipt.expense.as_ref().map(|e| e.title.as_str()),
            self.receipt.raw_text.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|text| text.to_lowercase().contains(&query))
    }
}

/// Every receipt, newest first. `fallback` is the currency to read a
/// receipt in that names none; the merchant does not depend on it.
pub fn receipt_archive(db: &Db, fallback: Currency) -> Result<Vec<ArchiveEntry>, StorageError> {
    db.receipt_archive()?
        .into_iter()
        .map(|receipt| {
            // Read from the boxes, not stored (`receipt.detected_merchant`
            // stays empty, see todo.md); parsing takes microseconds.
            let merchant = match receipt.raw_text {
                Some(_) => db.receipt_text(&receipt.id)?.and_then(|text| {
                    let fragments = text.recognized();
                    let rows = text_rows(&fragments);
                    let currency =
                        detect_currency(rows.iter().map(String::as_str)).unwrap_or(fallback);
                    parse_receipt(&fragments, currency).ok()?.details.merchant
                }),
                None => None,
            };
            Ok(ArchiveEntry { receipt, merchant })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use invuso_core::domain::{ExpenseSource, Group, Money, Person};
    use invuso_core::receipt::BoundingBox;
    use invuso_core::split::SplitMode;

    use super::*;
    use crate::storage::{
        NewExpense, NewExpensePayment, NewGroup, OcrFragment, Profile, ReceiptStatus, ReceiptText,
    };

    fn eur() -> Currency {
        Currency::from_code("EUR").unwrap()
    }

    fn fragment(text: &str, left: i32, top: i32) -> OcrFragment {
        OcrFragment {
            text: text.into(),
            bbox: BoundingBox {
                left,
                top,
                right: left + 200,
                bottom: top + 20,
            },
            confidence: 0.9,
        }
    }

    fn analyzed(db: &Db, path: &str, lines: &[(&str, Option<&str>)]) -> String {
        let receipt = db.create_receipt(path, None).unwrap();
        let fragments = lines
            .iter()
            .enumerate()
            .flat_map(|(row, (text, price))| {
                let top = 40 * row as i32;
                std::iter::once(fragment(text, 10, top))
                    .chain(price.map(|price| fragment(price, 400, top)))
            })
            .collect();
        db.save_receipt_text(&receipt.id, &ReceiptText::new("test", fragments, 0.0))
            .unwrap();
        receipt.id
    }

    struct Setup {
        db: Db,
        me: Person,
        group: Group,
    }

    fn setup() -> Setup {
        let db = Db::open_in_memory().unwrap();
        db.save_profile(&Profile {
            name: "Ich".into(),
            home_currency: eur(),
            target_language: "de".into(),
        })
        .unwrap();
        let me = db.me().unwrap().unwrap();
        let group = db
            .create_group(NewGroup {
                name: "Japan Reise".into(),
                icon: "plane".into(),
                color: "cerulean".into(),
                base_currency: eur(),
                start_date: None,
                end_date: None,
                target_language: None,
            })
            .unwrap();
        Setup { db, me, group }
    }

    fn expense(s: &Setup, title: &str, receipt: &str) -> NewExpense {
        NewExpense {
            group_id: Some(s.group.id.clone()),
            title: title.into(),
            category_id: None,
            occurred_at: "2026-10-03T19:30:00+02:00".into(),
            total: Money::new(1_250, eur()),
            payments: vec![NewExpensePayment {
                person_id: s.me.id.clone(),
                payment_method_id: None,
                amount_minor: 1_250,
            }],
            split: SplitMode::Equal([s.me.id.clone()].into()),
            receipt_id: Some(receipt.into()),
            line_items: Vec::new(),
            source: ExpenseSource::Scan,
            note: None,
            location: None,
            coordinates: None,
            own_rate: None,
        }
    }

    #[test]
    fn lists_open_and_saved_receipts_with_their_merchant() {
        let s = setup();
        let rate = s.db.latest_rate(eur(), eur()).unwrap().unwrap();
        let lidl = analyzed(
            &s.db,
            "receipts/lidl.jpg",
            &[
                ("LIDL", None),
                ("Milch", Some("1,29")),
                ("SUMME", Some("1,29")),
            ],
        );
        let rewe = analyzed(
            &s.db,
            "receipts/rewe.jpg",
            &[
                ("REWE", None),
                ("Ramen", Some("12,50")),
                ("SUMME", Some("12,50")),
            ],
        );
        let fresh = s.db.create_receipt("receipts/new.jpg", None).unwrap();
        let saved =
            s.db.create_expense(expense(&s, "Abendessen", &rewe), &rate)
                .unwrap();

        let archive = receipt_archive(&s.db, eur()).unwrap();
        // Newest first.
        let ids: Vec<&str> = archive.iter().map(|e| e.receipt.id.as_str()).collect();
        assert_eq!(ids, [fresh.id.as_str(), rewe.as_str(), lidl.as_str()]);

        let [fresh_entry, rewe_entry, lidl_entry] = &archive[..] else {
            panic!("three receipts");
        };
        assert!(fresh_entry.is_open());
        assert_eq!(fresh_entry.receipt.status, ReceiptStatus::New);
        assert_eq!(fresh_entry.merchant, None);
        assert!(lidl_entry.is_open());
        assert_eq!(lidl_entry.receipt.status, ReceiptStatus::Analyzed);
        assert_eq!(lidl_entry.merchant.as_deref(), Some("LIDL"));
        assert!(!rewe_entry.is_open());
        assert_eq!(rewe_entry.receipt.status, ReceiptStatus::Reviewed);
        let held = rewe_entry.receipt.expense.as_ref().unwrap();
        assert_eq!(held.id, saved.id);
        assert_eq!(held.title, "Abendessen");
        assert_eq!(held.group_name.as_deref(), Some("Japan Reise"));

        // Deleting the expense leaves the receipt waiting again.
        s.db.delete_expense(&saved.id).unwrap();
        let archive = receipt_archive(&s.db, eur()).unwrap();
        assert!(archive.iter().all(ArchiveEntry::is_open));
    }

    #[test]
    fn search_covers_merchant_title_and_text_in_any_case() {
        let s = setup();
        let rate = s.db.latest_rate(eur(), eur()).unwrap().unwrap();
        let lidl = analyzed(
            &s.db,
            "receipts/lidl.jpg",
            &[
                ("LIDL", None),
                ("Milch", Some("1,29")),
                ("SUMME", Some("1,29")),
            ],
        );
        let rewe = analyzed(
            &s.db,
            "receipts/rewe.jpg",
            &[
                ("REWE", None),
                ("Miso-Ramen", Some("12,50")),
                ("SUMME", Some("12,50")),
            ],
        );
        s.db.create_receipt("receipts/new.jpg", None).unwrap();
        s.db.create_expense(expense(&s, "Abendessen", &rewe), &rate)
            .unwrap();
        let archive = receipt_archive(&s.db, eur()).unwrap();
        let hits = |query: &str| -> Vec<&str> {
            archive
                .iter()
                .filter(|e| e.matches(query))
                .map(|e| e.receipt.id.as_str())
                .collect()
        };

        assert_eq!(hits("lidl"), [lidl.as_str()]);
        assert_eq!(hits("RAMEN"), [rewe.as_str()]);
        assert_eq!(hits("abendessen"), [rewe.as_str()]);
        assert_eq!(hits("milch"), [lidl.as_str()]);
        assert_eq!(hits("  ").len(), 3);
        assert!(hits("sushi").is_empty());
    }
}
