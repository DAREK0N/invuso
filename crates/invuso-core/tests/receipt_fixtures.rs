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

// Japanese receipts (OCR-03, OCR-17, AP-20), amounts in yen.

const LOTTERIA: &str = include_str!("fixtures/receipts/ja_lotteria.tsv");
const OHSHO: &str = include_str!("fixtures/receipts/ja_ohsho.tsv");
const SUKIYA: &str = include_str!("fixtures/receipts/ja_sukiya.tsv");
const YOSHINOYA: &str = include_str!("fixtures/receipts/ja_yoshinoya.tsv");
const SIMPLE_2017: &str = include_str!("fixtures/receipts/ja_simple_2017.tsv");

fn parse_yen(tsv: &str) -> ParsedReceipt {
    parse_receipt(&fragments(tsv), Currency::from_code("JPY").unwrap()).unwrap()
}

fn kind_of(receipt: &ParsedReceipt, text: &str) -> RowKind {
    receipt
        .rows
        .iter()
        .find(|r| r.text.contains(text))
        .unwrap_or_else(|| panic!("no row with {text}"))
        .kind
}

#[test]
fn lotteria_unit_price_count_and_total_columns() {
    let receipt = parse_yen(LOTTERIA);
    // `込` (tax included) and its misread `送` are glued to the amounts.
    assert_items(
        &receipt,
        &[
            ("贅沢バンバ-グステーキ", "1", Some(1000), 1000),
            ("ポテトセット", "1", Some(410), 410),
        ],
    );
    assert_eq!(cents(receipt.subtotal), Some(1410));
    assert_eq!(cents(receipt.total), Some(1410));
    assert_eq!(cents(receipt.tendered), Some(1510));
    assert_eq!(cents(receipt.change), Some(100));
    assert_eq!(receipt.check, TotalCheck::Matches);
}

#[test]
fn ohsho_counts_glued_to_names_and_spaced_sum_words() {
    let receipt = parse_yen(OHSHO);
    // `人数= 2` and the slip numbers above the table are no items.
    assert_items(
        &receipt,
        &[
            ("中華そば", "1", Some(748), 748),
            ("特選セットA", "1", Some(1120), 1120),
            ("TP 特セッ餃子+3個", "1", Some(165), 165),
        ],
    );
    assert_eq!(kind_of(&receipt, "小 計"), RowKind::Total);
    // `外税 0`: no tax added.
    assert_eq!(kind_of(&receipt, "税"), RowKind::Tax);
    assert_eq!(cents(receipt.total), Some(2033));
    assert_eq!(receipt.check, TotalCheck::Matches);
}

#[test]
fn sukiya_count_and_total_with_included_tax() {
    let receipt = parse_yen(SUKIYA);
    assert_items(&receipt, &[("(ミニ）まぜのっけ朝食", "1", Some(320), 320)]);
    assert_eq!(kind_of(&receipt, "商品代"), RowKind::Tax);
    assert_eq!(cents(receipt.total), Some(320));
    assert_eq!(receipt.check, TotalCheck::Matches);
}

#[test]
fn yoshinoya_total_printed_apart_from_its_amount() {
    let receipt = parse_yen(YOSHINOYA);
    assert_items(&receipt, &[("牛すき鍋膳·並", "1", Some(712), 712)]);
    assert_eq!(cents(receipt.subtotal), Some(712));
    assert_eq!(cents(receipt.total), Some(712));
    assert_eq!(kind_of(&receipt, "合計"), RowKind::Total);
    assert_eq!(receipt.check, TotalCheck::Matches);
}

#[test]
fn simple_2017_department_codes_and_yen_marks() {
    let receipt = parse_yen(SIMPLE_2017);
    assert_items(
        &receipt,
        &[("2", "1", Some(320), 320), ("2", "1", Some(300), 300)],
    );
    assert_eq!(kind_of(&receipt, "点"), RowKind::Other);
    assert_eq!(cents(receipt.total), Some(620));
    assert_eq!(cents(receipt.tendered), Some(1000));
    assert_eq!(cents(receipt.change), Some(380));
    assert_eq!(receipt.check, TotalCheck::Matches);
}

/// (fixture, items, total) of receipts whose items add up to the total.
fn assert_yen_receipt(tsv: &str, expected: &[Expected], total: i64) {
    let receipt = parse_yen(tsv);
    assert_items(&receipt, expected);
    assert_eq!(cents(receipt.total), Some(total));
    assert_eq!(receipt.check, TotalCheck::Matches);
}

#[test]
fn camelmart_tax_added_per_rate_between_subtotals() {
    let receipt = parse_yen(include_str!("fixtures/receipts/ja_camelmart.tsv"));
    // `外2¥150`: the tax class sits right before the yen sign; the third
    // article has no name on the receipt, its class reads `タト2`.
    assert_items(
        &receipt,
        &[
            ("リオンあんぱん", "1", Some(150), 150),
            ("鹿角ごみ収集袋小", "1", Some(130), 130),
            ("", "1", Some(160), 160),
            ("日用雜貨", "1", Some(200), 200),
            ("外税", "1", Some(33), 33),
            ("外税", "1", Some(24), 24),
        ],
    );
    assert_eq!(receipt.items[4].kind, ItemKind::Tax);
    assert_eq!(cents(receipt.subtotal), Some(640));
    assert_eq!(cents(receipt.total), Some(697));
    assert_eq!(cents(receipt.tendered), Some(702));
    assert_eq!(cents(receipt.change), Some(5));
    assert_eq!(receipt.check, TotalCheck::Matches);
}

#[test]
fn lawson_total_word_misread_with_latin_noise() {
    // `合 KR 言十`: `計` was read as `言十`.
    assert_yen_receipt(
        include_str!("fixtures/receipts/ja_lawson_naha.tsv"),
        &[("ノ-ヘ\"ル VC3000ノト\"アメ10ップ", "1", Some(103), 103)],
        103,
    );
}

#[test]
fn maxvalu_name_glued_to_price_discount_and_added_tax() {
    let receipt = parse_yen(include_str!("fixtures/receipts/ja_maxvalu.tsv"));
    assert_items(
        &receipt,
        &[
            ("サーモンチーズロールハー", "1", Some(200), 200),
            ("割引 30%", "1", Some(-60), -60),
            ("クリスタルガイザー", "1", Some(88), 88),
            ("外税", "1", Some(18), 18),
        ],
    );
    // `外税 8%対象額 ¥228` is the taxable base, not an addition.
    assert_eq!(kind_of(&receipt, "対象額"), RowKind::Tax);
    assert_eq!(cents(receipt.total), Some(246));
    assert_eq!(receipt.check, TotalCheck::Matches);
}

#[test]
fn sanoya_tax_class_before_names() {
    assert_yen_receipt(
        include_str!("fixtures/receipts/ja_sanoya.tsv"),
        &[
            ("森永・焼プリン", "1", Some(99), 99),
            ("マーボー茄子弁当", "1", Some(250), 250),
            ("外税", "1", Some(27), 27),
        ],
        376,
    );
}

#[test]
fn sugakiya_total_with_article_count() {
    // `合計 1点 ¥330` is the total, not a count row.
    assert_yen_receipt(
        include_str!("fixtures/receipts/ja_sugakiya_akamon.tsv"),
        &[("ラーメン", "1", Some(330), 330)],
        330,
    );
    // The price of the dessert sits below its name.
    assert_yen_receipt(
        include_str!("fixtures/receipts/ja_sugakiya_osu.tsv"),
        &[
            ("ラーメン", "1", Some(330), 330),
            ("デザートSTーベリー", "1", Some(280), 280),
        ],
        610,
    );
}

#[test]
fn yabaton_mark_after_the_yen_amount() {
    assert_yen_receipt(
        include_str!("fixtures/receipts/ja_yabaton.tsv"),
        &[("南九州厳選口ースとんか", "1", Some(1100), 1100)],
        1100,
    );
}

#[test]
fn cascade_without_printed_total() {
    // The photo ends before `合計`; the subtotal stands in for it.
    let receipt = parse_yen(include_str!("fixtures/receipts/ja_cascade.tsv"));
    assert_items(&receipt, &[("サンドイッチ", "1", Some(330), 330)]);
    assert_eq!(cents(receipt.total), Some(330));
}

#[test]
fn mcdonalds_set_with_count_in_brackets() {
    // The set's parts print `1コ`, which the recognizer reads as `13`; the
    // total check reports them as a difference for the review screen.
    let receipt = parse_yen(include_str!("fixtures/receipts/ja_mcd_yabacho.tsv"));
    assert_eq!(receipt.items[0].text, "(クーホ°ン535セット)");
    assert_eq!(receipt.items[0].quantity, Decimal::ONE);
    assert_eq!(cents(receipt.items[0].unit_price), Some(350));
    assert_eq!(cents(receipt.total), Some(350));
    assert_eq!(cents(receipt.tendered), Some(1000));
    assert_eq!(cents(receipt.change), Some(650));

    let receipt = parse_yen(include_str!("fixtures/receipts/ja_mcd_kanayama.tsv"));
    assert_eq!(receipt.items[0].text, "(エッグ”マックマフインセット)");
    assert_eq!(cents(receipt.items[0].unit_price), Some(400));
    assert_eq!(cents(receipt.total), Some(400));
    assert!(matches!(
        receipt.check,
        TotalCheck::Differs { difference, .. } if difference.amount_minor() == -39
    ));
}
