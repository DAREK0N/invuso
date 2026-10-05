//! Token rules: prices (OCR-11), quantities (OCR-12) and the small words
//! around them. A token is one whitespace-separated piece of a row.

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};

use crate::domain::Currency;

/// A price token as printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Price {
    /// Amount in minor units of the receipt currency; a leading minus is
    /// applied, a trailing one is not (see `trailing_minus`).
    pub minor: i64,
    /// `0,25-`: some tills mark negative amounts this way, others (IKEA)
    /// print a minus after every amount. Which one applies is decided per
    /// receipt.
    pub trailing_minus: bool,
}

/// Currency marks glued to or standing next to an amount.
const SYMBOLS: [&str; 8] = ["€", "$", "¥", "￥", "円", "£", "EUR", "JPY"];

/// More digits than any receipt needs; keeps `i64` far from overflowing.
const MAX_PRICE_DIGITS: usize = 15;
const MAX_QUANTITY_DIGITS: usize = 9;

/// Parses a price in the receipt currency: `1,49`, `3.60`, `1.234,56`,
/// `-0,88`, `13,98-`, `9,99-A`, `€3,99`, `¥1,410`, `1,410円`.
///
/// The decimal separator may be `,` or `.`; it must be followed by exactly
/// as many digits as the currency has minor units (EUR 2, JPY 0, KWD 3), so
/// `1,410` is 1410 yen but no euro amount, and plain integers are prices
/// only in currencies without minor units. The other separator groups
/// thousands.
pub(super) fn parse_price(token: &str, currency: Currency) -> Option<Price> {
    let mut rest = token.trim();
    // IKEA prints the tax class right after the trailing minus: `9,99-A`.
    if let Some(stripped) = rest.strip_suffix(|c: char| c.is_ascii_uppercase())
        && stripped.ends_with('-')
    {
        rest = stripped;
    }

    let mut negative = false;
    let mut trailing_minus = false;
    loop {
        let before = rest;
        rest = strip_symbols(rest, currency);
        if !negative && let Some(r) = rest.strip_prefix(['-', '−']) {
            negative = true;
            rest = r;
        }
        if !trailing_minus && let Some(r) = rest.strip_suffix('-') {
            trailing_minus = true;
            rest = r;
        }
        if rest == before {
            break;
        }
    }

    let minor = parse_amount_body(rest, currency.exponent() as usize)?;
    Some(Price {
        minor: if negative { -minor } else { minor },
        trailing_minus,
    })
}

fn strip_symbols(mut token: &str, currency: Currency) -> &str {
    for symbol in SYMBOLS.iter().copied().chain([currency.code()]) {
        token = token.strip_prefix(symbol).unwrap_or(token);
        token = token.strip_suffix(symbol).unwrap_or(token);
    }
    token
}

fn parse_amount_body(body: &str, exponent: usize) -> Option<i64> {
    let starts_and_ends_with_digit = body.starts_with(|c: char| c.is_ascii_digit())
        && body.ends_with(|c: char| c.is_ascii_digit());
    if !starts_and_ends_with_digit
        || !body
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    {
        return None;
    }

    let (integer, fraction, decimal_separator) = if exponent > 0 {
        let at = body.rfind(['.', ','])?;
        let fraction = &body[at + 1..];
        if fraction.len() != exponent {
            return None;
        }
        (&body[..at], fraction, body[at..].chars().next())
    } else {
        (body, "", None)
    };

    let mut digits = String::with_capacity(body.len());
    let separators: Vec<char> = integer.chars().filter(|c| !c.is_ascii_digit()).collect();
    if let Some(&separator) = separators.first() {
        if separators.iter().any(|&s| s != separator) || Some(separator) == decimal_separator {
            return None;
        }
        for (i, group) in integer.split(separator).enumerate() {
            let valid = if i == 0 {
                (1..=3).contains(&group.len())
            } else {
                group.len() == 3
            };
            if !valid {
                return None;
            }
            digits.push_str(group);
        }
    } else {
        digits.push_str(integer);
    }
    digits.push_str(fraction);

    if digits.len() > MAX_PRICE_DIGITS {
        return None;
    }
    digits.parse().ok()
}

/// A positive quantity: `2`, `0,452`, `1.5`; a unit may be glued on
/// (`0,452kg`, `2St`).
pub(super) fn parse_quantity(token: &str) -> Option<Decimal> {
    let lower = token.trim().to_lowercase();
    let number = UNIT_WORDS
        .iter()
        .find_map(|unit| lower.strip_suffix(unit))
        .unwrap_or(&lower);
    parse_plain_number(number)
}

fn parse_plain_number(text: &str) -> Option<Decimal> {
    let digit_count = text.chars().filter(char::is_ascii_digit).count();
    let separator_count = text.chars().filter(|c| matches!(c, '.' | ',')).count();
    let valid = digit_count > 0
        && digit_count <= MAX_QUANTITY_DIGITS
        && separator_count <= 1
        && digit_count + separator_count == text.chars().count()
        && text.starts_with(|c: char| c.is_ascii_digit())
        && text.ends_with(|c: char| c.is_ascii_digit());
    if !valid {
        return None;
    }
    let value: Decimal = text.replace(',', ".").parse().ok()?;
    (value > Decimal::ZERO).then_some(value)
}

/// A positive whole number, e.g. the leading `1` of `1 T-RINDERSTEAK`.
pub(super) fn parse_count(token: &str) -> Option<Decimal> {
    if token.chars().all(|c| c.is_ascii_digit()) {
        parse_plain_number(token)
    } else {
        None
    }
}

/// Units that may stand between a quantity and the multiplication sign.
const UNIT_WORDS: [&str; 6] = ["stück", "stk", "st", "kg", "pcs", "pc"];

pub(super) fn is_unit_word(token: &str) -> bool {
    let lower = token.trim_end_matches('.').to_lowercase();
    UNIT_WORDS.contains(&lower.as_str())
}

/// `x`, `X`, `×`, `*` or `@` between quantity and unit price.
pub(super) fn is_times(token: &str) -> bool {
    matches!(token, "x" | "X" | "×" | "*" | "@")
}

/// `2x`, `2X`, `2×`: quantity with the sign glued on.
pub(super) fn quantity_before_times(token: &str) -> Option<Decimal> {
    let number = token.strip_suffix(['x', 'X', '×'])?;
    parse_quantity(number)
}

/// `x2`, `×2`: a count without unit price.
pub(super) fn quantity_after_times(token: &str) -> Option<Decimal> {
    let number = token.strip_prefix(['x', 'X', '×'])?;
    parse_count(number)
}

/// Tokens printed after the price that carry no amount: tax classes
/// (`A`, `B`, `AW`, `*`), currency marks and per-unit marks (`EUR/kg`).
pub(super) fn is_trailing_mark(token: &str, currency: Currency) -> bool {
    let is_tax_class =
        (1..=2).contains(&token.len()) && token.chars().all(|c| c.is_ascii_uppercase());
    let is_per_unit = token.split_once('/').is_some_and(|(money, unit)| {
        (money.is_empty() || is_currency_mark(money, currency)) && is_unit_word(unit)
    });
    is_tax_class || matches!(token, "*" | "#") || is_currency_mark(token, currency) || is_per_unit
}

pub(super) fn is_currency_mark(token: &str, currency: Currency) -> bool {
    SYMBOLS.contains(&token) || token.eq_ignore_ascii_case(currency.code())
}

/// `quantity × unit_price` in minor units, rounded half to even
/// (idee.md 8.4).
pub(super) fn line_total(quantity: Decimal, unit_minor: i64) -> Option<i64> {
    quantity
        .checked_mul(Decimal::from(unit_minor))?
        .round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven)
        .to_i64()
}

/// Whether `quantity × unit` explains `total`. Tills round weighed goods
/// differently (half up, half even, down), so less than one minor unit of
/// difference is accepted.
pub(super) fn quantity_fits(quantity: Decimal, unit_minor: i64, total_minor: i64) -> bool {
    quantity
        .checked_mul(Decimal::from(unit_minor))
        .and_then(|product| product.checked_sub(Decimal::from(total_minor)))
        .is_some_and(|difference| difference.abs() < Decimal::ONE)
}

/// The word as compared against keywords: lowercase letters only, so
/// `SUMME[`, `GEGEBEN:` and `Zw.-Summe` match `summe`, `gegeben`, `zwsumme`.
pub(super) fn keyword_form(token: &str) -> String {
    token
        .chars()
        .filter(|c| c.is_alphabetic())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn eur(token: &str) -> Option<i64> {
        parse_price(token, cur("EUR")).map(|p| p.minor)
    }

    fn dec(text: &str) -> Decimal {
        text.parse().unwrap()
    }

    #[test]
    fn euro_prices_with_comma_or_dot() {
        assert_eq!(eur("1,49"), Some(149));
        assert_eq!(eur("3.60"), Some(360));
        assert_eq!(eur("0,05"), Some(5));
        assert_eq!(eur("1.234,56"), Some(123_456));
        assert_eq!(eur("1,234.56"), Some(123_456));
        assert_eq!(eur("12.345.678,90"), Some(1_234_567_890));
    }

    #[test]
    fn euro_rejects_non_prices() {
        for token in [
            "2",
            "0,452",
            "10,4",
            "12L",
            "16.00%",
            "04.05.19",
            "12:00",
            "1.23,45",
            "1,234,56",
            "1.2345,00",
            ",99",
            "9,",
            "4047777185192",
            "333443/01",
            "",
        ] {
            assert_eq!(eur(token), None, "{token}");
        }
    }

    #[test]
    fn signs_and_symbols() {
        assert_eq!(eur("-0,88"), Some(-88));
        assert_eq!(eur("−0,88"), Some(-88));
        assert_eq!(eur("€3,99"), Some(399));
        assert_eq!(eur("3,99€"), Some(399));
        assert_eq!(eur("-€3,99"), Some(-399));
        assert_eq!(eur("3,99EUR"), Some(399));
        let ikea = parse_price("9,99-A", cur("EUR")).unwrap();
        assert_eq!(
            ikea,
            Price {
                minor: 999,
                trailing_minus: true
            }
        );
        let deposit = parse_price("0,25-", cur("EUR")).unwrap();
        assert_eq!(
            deposit,
            Price {
                minor: 25,
                trailing_minus: true
            }
        );
    }

    #[test]
    fn yen_has_no_minor_unit() {
        let jpy = cur("JPY");
        let yen = |t: &str| parse_price(t, jpy).map(|p| p.minor);
        assert_eq!(yen("748"), Some(748));
        assert_eq!(yen("1,410"), Some(1410));
        assert_eq!(yen("1.410"), Some(1410));
        assert_eq!(yen("¥1,410"), Some(1410));
        assert_eq!(yen("￥320"), Some(320));
        assert_eq!(yen("1,410円"), Some(1410));
        assert_eq!(yen("3.60"), None);
        assert_eq!(yen("14,10"), None);
    }

    #[test]
    fn three_digit_minor_unit() {
        let kwd = cur("KWD");
        assert_eq!(parse_price("1.234", kwd).map(|p| p.minor), Some(1234));
        assert_eq!(
            parse_price("1,234.500", kwd).map(|p| p.minor),
            Some(1_234_500)
        );
        assert_eq!(parse_price("1,23", kwd), None);
    }

    #[test]
    fn huge_numbers_are_not_prices() {
        assert_eq!(eur("1234567890123456,00"), None);
    }

    #[test]
    fn quantities() {
        assert_eq!(parse_quantity("2"), Some(dec("2")));
        assert_eq!(parse_quantity("0,452"), Some(dec("0.452")));
        assert_eq!(parse_quantity("1.5"), Some(dec("1.5")));
        assert_eq!(parse_quantity("0,452kg"), Some(dec("0.452")));
        assert_eq!(parse_quantity("3St"), Some(dec("3")));
        assert_eq!(parse_quantity("0"), None);
        assert_eq!(parse_quantity("1,2,3"), None);
        assert_eq!(parse_quantity("A"), None);
        assert_eq!(quantity_before_times("2x"), Some(dec("2")));
        assert_eq!(quantity_after_times("×3"), Some(dec("3")));
        assert_eq!(quantity_after_times("x0,5"), None);
        assert_eq!(parse_count("10,4"), None);
    }

    #[test]
    fn trailing_marks() {
        let eur = cur("EUR");
        for token in ["A", "B", "AW", "*", "EUR", "€", "EUR/kg", "€/kg", "/kg"] {
            assert!(is_trailing_mark(token, eur), "{token}");
        }
        for token in ["Kern", "ABC", "1,49", "12L"] {
            assert!(!is_trailing_mark(token, eur), "{token}");
        }
    }

    #[test]
    fn weighed_goods_round_within_one_minor_unit() {
        // 0.452 kg × 3.99 €/kg = 1.80348 €
        assert_eq!(line_total(dec("0.452"), 399), Some(180));
        assert!(quantity_fits(dec("0.452"), 399, 180));
        assert!(quantity_fits(dec("0.452"), 399, 181));
        assert!(!quantity_fits(dec("0.452"), 399, 179));
        assert!(quantity_fits(dec("2"), 49, 98));
        assert!(!quantity_fits(dec("2"), 49, 99));
    }

    #[test]
    fn keyword_form_keeps_letters_only() {
        assert_eq!(keyword_form("SUMME["), "summe");
        assert_eq!(keyword_form("GEGEBEN:"), "gegeben");
        assert_eq!(keyword_form("Rückgeld"), "rückgeld");
        assert_eq!(keyword_form("MwSt.-Senkung"), "mwstsenkung");
    }
}
