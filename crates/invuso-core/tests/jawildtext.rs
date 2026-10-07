//! Parser evaluation on the receipt_kie subset of llm-jp/jawildtext
//! (1,151 Japanese receipts, Apache-2.0), exported by
//! `spikes/ocr/jawildtext_export.py`, or on any export of the same layout
//! (`spikes/ocr/synthetic_export.py`: German, British and US receipts). The annotated text regions stand in
//! for a perfect text recognition, so every miss is the parser's.
//!
//! Not in the repository (3.7 GB); run with
//! `cargo test -p invuso-core --test jawildtext -- --ignored --nocapture`.
//! `JAWILDTEXT_BOXES=ocr.tsv` reads the app's recognition instead of the
//! annotation (written by the OCR test of `invuso-app`), and
//! `JAWILDTEXT_LABEL` names the report file; `JAWILDTEXT_SHOW=0001,0002`
//! prints rows, items and the expected items of these receipts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use invuso_core::domain::Currency;
use invuso_core::receipt::{BoundingBox, ItemKind, ParsedReceipt, RecognizedText, parse_receipt};

fn fragments(tsv: &str) -> Vec<RecognizedText> {
    tsv.lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| {
            let mut columns = line.splitn(5, '\t');
            let mut number = || columns.next()?.parse::<i32>().ok();
            let bbox = BoundingBox {
                left: number()?,
                top: number()?,
                right: number()?,
                bottom: number()?,
            };
            Some(RecognizedText {
                text: columns.next()?.to_string(),
                bbox,
            })
        })
        .collect()
}

#[derive(Default)]
struct Truth {
    /// `JPY` unless the export names another one.
    currency: Option<String>,
    total: Option<i64>,
    tax: Option<i64>,
    date: String,
    time: String,
    store: String,
    /// Line item prices in yen.
    items: Vec<i64>,
}

fn truth(tsv: &str) -> Truth {
    let mut truth = Truth::default();
    for line in tsv.lines() {
        let columns: Vec<&str> = line.split('\t').collect();
        match columns.as_slice() {
            ["currency", value, ..] if !value.is_empty() => {
                truth.currency = Some(value.to_string());
            }
            ["total", value, ..] => truth.total = value.parse().ok(),
            ["tax", value, ..] => truth.tax = value.parse().ok(),
            ["date", value, ..] => truth.date = value.to_string(),
            ["time", value, ..] => truth.time = value.to_string(),
            ["store", value, ..] => truth.store = value.to_string(),
            ["item", _, price, ..] => truth.items.extend(price.parse::<i64>().ok()),
            _ => {}
        }
    }
    truth
}

/// Line items of `truth` found among the parsed ones (by total or unit
/// price), each parsed item used once.
/// Also returns the texts of parsed items that match none.
fn matched_items(truth: &[i64], parsed: &ParsedReceipt) -> (usize, Vec<String>) {
    // The annotation lists articles only; discounts and added tax are
    // items here so that they add up to the total.
    let mut free: Vec<(i64, Option<i64>, String)> = parsed
        .items
        .iter()
        .filter(|item| !matches!(item.kind, ItemKind::Discount | ItemKind::Tax))
        .map(|item| {
            (
                item.total_price.amount_minor(),
                item.unit_price.map(|unit| unit.amount_minor()),
                format!("{} = {}", item.text, item.total_price.amount_minor()),
            )
        })
        .collect();
    let found = truth
        .iter()
        .filter(|price| {
            let at = free
                .iter()
                .position(|(total, _, _)| total == *price)
                .or_else(|| free.iter().position(|(_, unit, _)| *unit == Some(**price)));
            at.map(|at| free.remove(at)).is_some()
        })
        .count();
    (found, free.into_iter().map(|(_, _, text)| text).collect())
}

fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

#[derive(Default)]
struct Score {
    receipts: usize,
    with_total: usize,
    total_right: usize,
    sums_match: usize,
    truth_items: usize,
    parsed_items: usize,
    found_items: usize,
    with_date: usize,
    date_right: usize,
    with_time: usize,
    time_right: usize,
    with_store: usize,
    store_right: usize,
    with_tax: usize,
    tax_right: usize,
}

fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        "–".to_string()
    } else {
        format!(
            "{:.1} % ({part}/{whole})",
            part as f64 * 100.0 / whole as f64
        )
    }
}

#[test]
#[ignore = "needs the exported jawildtext data in spikes/ocr/datasets"]
fn parser_on_jawildtext() {
    let root: PathBuf = std::env::var("JAWILDTEXT_DIR").map_or_else(
        |_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../spikes/ocr/datasets/jawildtext/export")
        },
        PathBuf::from,
    );
    let boxes_file = std::env::var("JAWILDTEXT_BOXES").unwrap_or_else(|_| "boxes.tsv".into());
    let label = std::env::var("JAWILDTEXT_LABEL").unwrap_or_else(|_| "parser".into());
    let yen = Currency::from_code("JPY").unwrap();

    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("export missing: run spikes/ocr/jawildtext_export.py")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join(&boxes_file).exists())
        .collect();
    dirs.sort();

    let mut score = Score::default();
    let mut report = vec![
        "id\ttotal\tparsed_total\tcheck\ttruth_items\tparsed_items\tfound\tdate\ttime\tstore\tparsed_store"
            .to_string(),
    ];
    let mut misses: BTreeMap<&'static str, usize> = BTreeMap::new();
    let show = std::env::var("JAWILDTEXT_SHOW").unwrap_or_default();
    let mut extras: Vec<String> = Vec::new();
    for dir in &dirs {
        let id = dir.file_name().unwrap().to_string_lossy().to_string();
        let boxes = std::fs::read_to_string(dir.join(&boxes_file)).unwrap();
        let truth = truth(&std::fs::read_to_string(dir.join("truth.tsv")).unwrap());
        let currency = truth
            .currency
            .as_deref()
            .and_then(|code| Currency::from_code(code).ok())
            .unwrap_or(yen);
        let parsed = parse_receipt(&fragments(&boxes), currency).unwrap();
        let details = &parsed.details;

        score.receipts += 1;
        let parsed_total = parsed.total.map(|m| m.amount_minor());
        let total_right = truth.total.is_some() && parsed_total == truth.total;
        if truth.total.is_some() {
            score.with_total += 1;
            score.total_right += usize::from(total_right);
            if !total_right {
                *misses
                    .entry(if parsed_total.is_none() {
                        "total not found"
                    } else {
                        "total wrong"
                    })
                    .or_default() += 1;
            }
        }
        let sums_match = matches!(parsed.check, invuso_core::receipt::TotalCheck::Matches);
        score.sums_match += usize::from(sums_match);
        let (found, extra) = matched_items(&truth.items, &parsed);
        extras.extend(extra.into_iter().map(|text| format!("{id}	{text}")));
        score.truth_items += truth.items.len();
        let articles = parsed
            .items
            .iter()
            .filter(|item| !matches!(item.kind, ItemKind::Discount | ItemKind::Tax))
            .count();
        score.parsed_items += articles;
        score.found_items += found;
        if found < truth.items.len() {
            *misses.entry("items missed").or_default() += 1;
        }
        if articles > found {
            *misses.entry("extra items").or_default() += 1;
        }

        let date_right = !truth.date.is_empty() && details.date.as_deref() == Some(&*truth.date);
        if !truth.date.is_empty() {
            score.with_date += 1;
            score.date_right += usize::from(date_right);
        }
        let time_right = !truth.time.is_empty() && details.time.as_deref() == Some(&*truth.time);
        if !truth.time.is_empty() {
            score.with_time += 1;
            score.time_right += usize::from(time_right);
        }
        // Contained VAT plus tax added on top, against the stated tax.
        if let Some(tax) = truth.tax.filter(|tax| *tax > 0) {
            let read: i64 = parsed
                .vat
                .iter()
                .map(|line| line.amount.amount_minor())
                .chain(
                    parsed
                        .items
                        .iter()
                        .filter(|item| item.kind == ItemKind::Tax)
                        .map(|item| item.total_price.amount_minor()),
                )
                .sum();
            score.with_tax += 1;
            score.tax_right += usize::from(read == tax);
        }
        let store = squeeze(&truth.store);
        let parsed_store = details.merchant.as_deref().map(squeeze).unwrap_or_default();
        let store_right = !store.is_empty()
            && !parsed_store.is_empty()
            && (store.contains(&parsed_store) || parsed_store.contains(&store));
        if !store.is_empty() {
            score.with_store += 1;
            score.store_right += usize::from(store_right);
        }

        if show.split(',').any(|wanted| wanted == id) {
            println!(
                "===== {id}: truth total {:?}, items {:?}",
                truth.total, truth.items
            );
            for row in &parsed.rows {
                println!("  {:>9} {}", format!("{:?}", row.kind), row.text);
            }
            for item in &parsed.items {
                println!(
                    "  -> {} | {} | {:?} | {:?}",
                    item.text,
                    item.quantity,
                    item.total_price.amount_minor(),
                    item.kind
                );
            }
            println!("  total {parsed_total:?}, details {details:?}");
            println!(
                "  tax truth {:?}, vat {:?}",
                truth.tax,
                parsed
                    .vat
                    .iter()
                    .map(|line| (line.rate, line.amount.amount_minor()))
                    .collect::<Vec<_>>()
            );
        }
        report.push(format!(
            "{id}\t{}\t{}\t{}\t{}\t{}\t{found}\t{}\t{}\t{}\t{}",
            truth.total.map(|t| t.to_string()).unwrap_or_default(),
            parsed_total.map(|t| t.to_string()).unwrap_or_default(),
            if sums_match { "ok" } else { "differs" },
            truth.items.len(),
            parsed.items.len(),
            if date_right { "ok" } else { "–" },
            if time_right { "ok" } else { "–" },
            if store_right { "ok" } else { "–" },
            details.merchant.as_deref().unwrap_or(""),
        ));
    }
    assert!(score.receipts > 0, "no receipts in {}", root.display());

    let out = root.join(format!("../report-{label}.tsv"));
    std::fs::write(&out, report.join("\n") + "\n").unwrap();
    std::fs::write(
        root.join(format!("../extra-{label}.tsv")),
        extras.join("\n") + "\n",
    )
    .unwrap();
    println!(
        "{} receipts ({label}), report: {}",
        score.receipts,
        out.display()
    );
    println!(
        "total right          {}",
        percent(score.total_right, score.with_total)
    );
    println!(
        "items add up         {}",
        percent(score.sums_match, score.receipts)
    );
    println!(
        "items found (recall) {}",
        percent(score.found_items, score.truth_items)
    );
    println!(
        "items right (prec.)  {}",
        percent(score.found_items, score.parsed_items)
    );
    println!(
        "date right           {}",
        percent(score.date_right, score.with_date)
    );
    println!(
        "time right           {}",
        percent(score.time_right, score.with_time)
    );
    println!(
        "store right          {}",
        percent(score.store_right, score.with_store)
    );
    println!(
        "tax right            {}",
        percent(score.tax_right, score.with_tax)
    );
    println!("receipts with: {misses:?}");
}
