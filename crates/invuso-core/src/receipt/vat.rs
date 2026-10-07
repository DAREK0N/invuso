//! VAT contained in the prices, as the receipt states it (shown in the
//! review, AP-38): `ENTH. MwSt 19% 7,38`, the VAT table
//! `1=19,00% 53,83 45,24 8,59`, `(内消費税等8% ¥95`. Tax added on top is an
//! item instead (`ItemKind::Tax`).

use rust_decimal::Decimal;

use super::tokens::{keyword_form, parse_price};
use super::{ReceiptRow, RowKind, VatLine};
use crate::domain::{Currency, Money};

/// The VAT lines of the receipt's tax rows, one per rate and amount.
pub(super) fn read_vat(rows: &[ReceiptRow], currency: Currency) -> Vec<VatLine> {
    let mut lines: Vec<VatLine> = Vec::new();
    for row in rows.iter().filter(|row| row.kind == RowKind::Tax) {
        let Some(line) = vat_of(&row.text, currency) else {
            continue;
        };
        let known = lines.iter().any(|other| {
            other.amount == line.amount && (other.rate == line.rate || line.rate.is_none())
        });
        if !known {
            // A rate-less mention of an amount already listed with its rate
            // adds nothing; the other way round, the rate wins.
            lines.retain(|other| !(other.rate.is_none() && other.amount == line.amount));
            lines.push(line);
        }
    }
    lines
}

/// First words of rows that print a base, not a tax.
const BASE_WORDS: &[&str] = &["netto", "brutto", "umsatz", "net", "gross"];

/// Words naming the tax itself.
const TAX_NAMES: &[&str] = &["mwst", "ust", "vat", "tax", "steuer", "tva", "iva"];

fn vat_of(text: &str, currency: Currency) -> Option<VatLine> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let first = tokens.first().map(|token| keyword_form(token))?;
    // `Netto ohne MwSt 3,82`: the base.
    if BASE_WORDS.contains(&first.as_str()) {
        return None;
    }
    // The rate is no amount, also when its `%` stands apart (`19,00 %`).
    let is_rate = |at: usize| {
        tokens[at].contains('%') || tokens.get(at + 1).is_some_and(|next| next.starts_with('%'))
    };
    let amounts: Vec<i64> = (0..tokens.len())
        .filter(|&at| !is_rate(at))
        .filter_map(|at| parse_price(tokens[at].trim_matches(['(', ')', '（', '）']), currency))
        .map(|price| price.minor)
        .filter(|amount| *amount > 0)
        .collect();
    let names_tax = tokens.iter().any(|token| {
        let word = keyword_form(token);
        TAX_NAMES.iter().any(|name| word.contains(name))
    }) || text.contains('税');
    let amount = match amounts.as_slice() {
        [] => return None,
        // `10%対象 ¥2,285`: the base alone; `8%対象 ¥2,224 税 177` has both.
        // `内税小計 ¥968`: a subtotal.
        [single] => {
            (names_tax && !text.contains("対象") && !text.contains("小計")).then_some(*single)?
        }
        [a, b] => *a.min(b),
        // Net + tax = gross: the tax is the part that completes the sum.
        many => many
            .iter()
            .copied()
            .filter(|tax| {
                many.iter()
                    .any(|net| many.iter().any(|gross| net + tax == *gross && net != tax))
            })
            .min()?,
    };
    Some(VatLine {
        rate: rate_of(&tokens),
        amount: Money::new(amount, currency),
    })
}

/// `19%`, `19,00%`, `1=19,00%`, `7 %`, `内消費税等8%`.
fn rate_of(tokens: &[&str]) -> Option<Decimal> {
    tokens.iter().enumerate().find_map(|(at, token)| {
        let number = match token.find('%')? {
            0 if at > 0 => tokens[at - 1],
            0 => return None,
            end => &token[..end],
        };
        let digits: String = number
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit() || matches!(c, ',' | '.'))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let rate: Decimal = digits.replace(',', ".").parse().ok()?;
        (rate > Decimal::ZERO && rate < Decimal::from(100)).then(|| rate.normalize())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt::BoundingBox;

    fn tax_rows(texts: &[&str]) -> Vec<ReceiptRow> {
        texts
            .iter()
            .map(|text| ReceiptRow {
                text: text.to_string(),
                bbox: BoundingBox::default(),
                kind: RowKind::Tax,
                fragments: Vec::new(),
            })
            .collect()
    }

    fn vat(texts: &[&str], code: &str) -> Vec<(Option<String>, i64)> {
        read_vat(&tax_rows(texts), Currency::from_code(code).unwrap())
            .into_iter()
            .map(|line| (line.rate.map(|r| r.to_string()), line.amount.amount_minor()))
            .collect()
    }

    #[test]
    fn vat_from_tables_and_single_rows() {
        assert_eq!(
            vat(
                &["1=19,00% 53,83 45,24 8,59", "2=7,00% 7,50 7,01 0,49"],
                "EUR"
            ),
            [(Some("19".into()), 859), (Some("7".into()), 49)]
        );
        assert_eq!(
            vat(&["ENTH. MwSt 19% 7,38"], "EUR"),
            [(Some("19".into()), 738)]
        );
        assert_eq!(
            vat(&["19,00 % MwSt A 4,55 0,73", "Netto ohne MwSt 3,82"], "EUR"),
            [(Some("19".into()), 73)]
        );
        assert_eq!(
            vat(&["A 7 % 1,30 18,60 19,90"], "EUR"),
            [(Some("7".into()), 130)]
        );
        assert_eq!(vat(&["(内消費税等8% ¥95"], "JPY"), [(Some("8".into()), 95)]);
        assert_eq!(
            vat(&["8%対象 ¥2,224 税 177"], "JPY"),
            [(Some("8".into()), 177)]
        );
        // The same amount stated twice, once with its rate.
        assert_eq!(
            vat(&["enth. MwSt 1,30", "A 7 % 1,30 18,60 19,90"], "EUR"),
            [(Some("7".into()), 130)]
        );
    }

    #[test]
    fn bases_and_headers_are_no_vat() {
        assert!(
            vat(
                &[
                    "10%対象 ¥2,285",
                    "Netto: 29,40 EUR",
                    "MWST Netto Steuer Brutto"
                ],
                "JPY"
            )
            .is_empty()
        );
        assert!(vat(&["Netto: 29,40 EUR"], "EUR").is_empty());
    }
}
