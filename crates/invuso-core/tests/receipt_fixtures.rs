//! Real German receipts as an OCR engine saw them (CORE-11, AP-17).
//!
//! Fixtures in `fixtures/receipts/` are raw PP-OCRv6 fragments from the
//! OCR prototype (`spikes/ocr`), anonymized; sources and licences are in
//! each file's header.

use invuso_core::Decimal;
use invuso_core::domain::Currency;
use invuso_core::receipt::{
    BoundingBox, ItemKind, ParsedReceipt, RecognizedText, RowKind, TotalCheck, parse_receipt,
};

fn fragments(tsv: &str) -> Vec<RecognizedText> {
    tsv.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut columns = line.splitn(5, '\t');
            let mut number = || -> i32 { columns.next().unwrap().parse().unwrap() };
            let bbox = BoundingBox {
                left: number(),
                top: number(),
                right: number(),
                bottom: number(),
            };
            RecognizedText {
                text: columns.next().unwrap().to_string(),
                bbox,
            }
        })
        .collect()
}

fn parse(tsv: &str) -> ParsedReceipt {
    parse_receipt(&fragments(tsv), Currency::from_code("EUR").unwrap()).unwrap()
}

/// (text, quantity, unit price, total) with amounts in cents.
type Expected<'a> = (&'a str, &'a str, Option<i64>, i64);

fn assert_items(receipt: &ParsedReceipt, expected: &[Expected]) {
    let actual: Vec<_> = receipt
        .items
        .iter()
        .map(|item| {
            (
                item.text.clone(),
                item.quantity,
                item.unit_price.map(|m| m.amount_minor()),
                item.total_price.amount_minor(),
            )
        })
        .collect();
    let expected: Vec<_> = expected
        .iter()
        .map(|(text, quantity, unit, total)| {
            (
                text.to_string(),
                quantity.parse::<Decimal>().unwrap(),
                *unit,
                *total,
            )
        })
        .collect();
    assert_eq!(actual, expected);
}

fn cents(money: Option<invuso_core::domain::Money>) -> Option<i64> {
    money.map(|m| m.amount_minor())
}

const LIDL_AURICH: &str = include_str!("fixtures/receipts/de_lidl_aurich_01.tsv");
const LIDL_HESEL: &str = include_str!("fixtures/receipts/de_lidl_hesel_01.tsv");
const FRESSNAPF: &str = include_str!("fixtures/receipts/de_fressnapf_2020.tsv");
const IKEA: &str = include_str!("fixtures/receipts/de_ikea_2009.tsv");
const AUGUSTINER: &str = include_str!("fixtures/receipts/de_augustiner_2020.tsv");

#[test]
fn lidl_aurich_with_quantity_row() {
    let receipt = parse(LIDL_AURICH);
    assert_items(
        &receipt,
        &[
            ("Dattelcherrytomaten", "1", Some(149), 149),
            ("Grüne Oliven o. Kern", "1", Some(399), 399),
            ("Erbsen extra fein", "1", Some(59), 59),
            ("Gemüsemais", "2", Some(49), 98),
            ("Milchbrötchen", "1", Some(99), 99),
            ("B Schlafover 0301558", "1", Some(499), 499),
            ("B Schlafover 0301438", "1", Some(499), 499),
        ],
    );
    assert_eq!(cents(receipt.total), Some(1802));
    assert_eq!(cents(receipt.tendered), Some(1802));
    assert_eq!(receipt.check, TotalCheck::Matches);
    // The VAT table after the payment, including its `Summe` row, is no item.
    let summe = receipt
        .rows
        .iter()
        .find(|r| r.text.starts_with("Summe"))
        .unwrap();
    assert_eq!(summe.kind, RowKind::Other);
}

#[test]
fn lidl_hesel_single_item() {
    let receipt = parse(LIDL_HESEL);
    assert_items(&receipt, &[("Bio Gurken", "1", Some(77), 77)]);
    assert_eq!(cents(receipt.total), Some(77));
    assert_eq!(receipt.check, TotalCheck::Matches);
}

#[test]
fn fressnapf_with_discount_between_subtotal_and_total() {
    let receipt = parse(FRESSNAPF);
    assert_items(
        &receipt,
        &[
            ("ANIO Ersatzfilter 2 Stk", "1", Some(999), 999),
            ("ANIO Ersatzpumpe", "1", Some(999), 999),
            ("PREM Multicat 12L", "1", Some(1499), 1499),
            ("MwSt.-Senkung", "1", Some(-88), -88),
        ],
    );
    assert_eq!(receipt.items[3].kind, ItemKind::Discount);
    assert_eq!(cents(receipt.subtotal), Some(3497));
    assert_eq!(cents(receipt.total), Some(3409));
    assert_eq!(cents(receipt.tendered), Some(5009));
    assert_eq!(cents(receipt.change), Some(1600));
    assert_eq!(receipt.check, TotalCheck::Matches);
}

#[test]
fn ikea_with_minus_after_every_amount() {
    let receipt = parse(IKEA);
    assert_items(
        &receipt,
        &[
            ("BITS MAGNTAFEL", "1", Some(999), 999),
            ("BABBLA WHITEBST", "1", Some(399), 399),
        ],
    );
    assert_eq!(cents(receipt.subtotal), Some(1398));
    assert_eq!(cents(receipt.total), Some(1398));
    assert_eq!(cents(receipt.tendered), Some(1398));
    assert_eq!(receipt.check, TotalCheck::Matches);
}

#[test]
fn augustiner_restaurant_bill_with_unit_and_total_columns() {
    let receipt = parse(AUGUSTINER);
    // `1 0,4 Schorle` was read as `10,4 Schorle`; the prices still say 1 ×.
    assert_items(
        &receipt,
        &[
            ("10,4 Schorle", "1", Some(360), 360),
            ("T-RINDERSTEAK", "1", Some(1990), 1990),
        ],
    );
    let tax_rows = receipt
        .rows
        .iter()
        .filter(|r| r.kind == RowKind::Tax)
        .count();
    assert_eq!(tax_rows, 2);
    assert_eq!(cents(receipt.total), Some(2350));
    assert_eq!(cents(receipt.tendered), Some(2350));
    assert_eq!(receipt.check, TotalCheck::Matches);
}

#[test]
fn missing_item_is_detected_against_the_total() {
    // Drop the `Milchbrötchen 0,99 A` row as if the camera had missed it.
    let mut fragments = fragments(LIDL_AURICH);
    fragments.retain(|f| f.text != "Milchbrötchen" && f.text != "0,99 A");
    let receipt = parse_receipt(&fragments, Currency::from_code("EUR").unwrap()).unwrap();
    assert_eq!(receipt.items.len(), 6);
    match receipt.check {
        TotalCheck::Differs {
            items_sum,
            difference,
        } => {
            assert_eq!(items_sum.amount_minor(), 1703);
            assert_eq!(difference.amount_minor(), 99);
        }
        other => panic!("expected a difference, got {other:?}"),
    }
}

#[test]
fn misread_price_is_detected_against_the_total() {
    let mut fragments = fragments(FRESSNAPF);
    let price = fragments.iter_mut().find(|f| f.text == "14,99").unwrap();
    price.text = "14,39".into();
    let receipt = parse_receipt(&fragments, Currency::from_code("EUR").unwrap()).unwrap();
    assert!(matches!(
        receipt.check,
        TotalCheck::Differs { difference, .. } if difference.amount_minor() == 60
    ));
}
