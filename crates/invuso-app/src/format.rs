//! Localized display and entry of numbers and amounts (UI-05, UI-06). The
//! separators and the symbol position come from the app language
//! (`number.*` in `locales/`); everything else here is plain logic, tested
//! with fixed formats.

use invuso_core::Decimal;
use invuso_core::domain::{Currency, Money};

/// Up to this many integer digits can be typed: 10^12 even in a currency
/// with 4 decimals stays far below `i64::MAX` minor units.
const MAX_INTEGER_DIGITS: usize = 12;

/// Significant digits a rate is shown with, e.g. `177.71` or `0.0056271`.
const RATE_DIGITS: u32 = 5;

/// Symbols that name exactly one currency. Everything else shows its code,
/// because `$`, `¥` or `kr` alone would leave open which currency is meant.
const UNAMBIGUOUS_SYMBOLS: [(&str, &str); 14] = [
    ("EUR", "€"),
    ("GBP", "£"),
    ("JPY", "¥"),
    ("KRW", "₩"),
    ("INR", "₹"),
    ("ILS", "₪"),
    ("THB", "฿"),
    ("VND", "₫"),
    ("UAH", "₴"),
    ("PHP", "₱"),
    ("TRY", "₺"),
    ("KZT", "₸"),
    ("GEL", "₾"),
    ("RUB", "₽"),
];

/// Separators and symbol position of one app language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    pub decimal: char,
    pub group: char,
    /// `€12.50` (true) or `12,50 €` (false).
    pub symbol_before: bool,
}

impl NumberFormat {
    /// Format of the current app language.
    pub fn current() -> Self {
        let first = |text: &str, fallback| text.chars().next().unwrap_or(fallback);
        Self {
            decimal: first(&t!("number.decimal_separator"), '.'),
            group: first(&t!("number.group_separator"), ','),
            symbol_before: t!("number.symbol_before") == "true",
        }
    }
}

/// What a currency is shown with next to an amount: its symbol where that
/// is unambiguous, otherwise its ISO code.
pub fn currency_symbol(currency: Currency) -> &'static str {
    UNAMBIGUOUS_SYMBOLS
        .iter()
        .find(|(code, _)| *code == currency.code())
        .map_or(currency.code(), |(_, symbol)| symbol)
}

/// `1.234,56 €`, `-€12.50`, `1.000 ¥`, `12,50 USD`: grouped, with exactly
/// as many decimals as the currency has.
pub fn format_money(money: Money, format: NumberFormat) -> String {
    let symbol = currency_symbol(money.currency());
    let number = format_minor(
        money.amount_minor().unsigned_abs(),
        money.currency(),
        format,
    );
    let sign = if money.is_negative() { "-" } else { "" };
    if format.symbol_before {
        // A code needs a gap ("USD 12.50"), a symbol sits close ("€12.50").
        let gap = if symbol.chars().all(|c| c.is_ascii_alphabetic()) {
            "\u{a0}"
        } else {
            ""
        };
        format!("{sign}{symbol}{gap}{number}")
    } else {
        format!("{sign}{number}\u{a0}{symbol}")
    }
}

/// The amount alone, without symbol, e.g. `1.234,56`.
fn format_minor(minor: u64, currency: Currency, format: NumberFormat) -> String {
    let exponent = currency.exponent();
    let scale = 10_u64.pow(exponent);
    let integer = group_digits(&(minor / scale).to_string(), format.group);
    if exponent == 0 {
        return integer;
    }
    let fraction = minor % scale;
    format!(
        "{integer}{}{fraction:0width$}",
        format.decimal,
        width = exponent as usize
    )
}

fn group_digits(digits: &str, separator: char) -> String {
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(separator);
        }
        grouped.push(digit);
    }
    grouped
}

/// An exchange rate for reading, rounded to a few significant digits:
/// `177,71` or `0,0056271`.
pub fn format_rate(rate: Decimal, format: NumberFormat) -> String {
    let shown = rate.round_sf(RATE_DIGITS).unwrap_or(rate).normalize();
    let text = shown.abs().to_string();
    let (integer, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let sign = if shown.is_sign_negative() { "-" } else { "" };
    let integer = group_digits(integer, format.group);
    if fraction.is_empty() {
        format!("{sign}{integer}")
    } else {
        format!("{sign}{integer}{}{fraction}", format.decimal)
    }
}

/// Checks what was typed into an amount field and returns it in canonical
/// form, or `None` if it is no valid amount for `currency` and the field
/// should keep its previous text. Accepts digits and one decimal separator
/// (`,` or `.`, whatever the keyboard offers; shown as the app language's),
/// at most as many decimals as the currency has. No sign, no grouping.
pub fn clean_amount_input(raw: &str, currency: Currency, format: NumberFormat) -> Option<String> {
    let raw = raw.trim();
    let (integer, fraction) = match raw.find([',', '.']) {
        Some(index) => (&raw[..index], Some(&raw[index + 1..])),
        None => (raw, None),
    };
    let digits = |part: &str| part.chars().all(|c| c.is_ascii_digit());
    if !digits(integer) || integer.len() > MAX_INTEGER_DIGITS {
        return None;
    }
    // A leading zero only stays in front of the separator ("0,5", not "05").
    let integer = match integer.trim_start_matches('0') {
        "" if integer.is_empty() => "",
        "" => "0",
        rest => rest,
    };
    match fraction {
        None => Some(integer.to_string()),
        Some(fraction) => {
            let exponent = currency.exponent() as usize;
            if exponent == 0 || !digits(fraction) || fraction.len() > exponent {
                return None;
            }
            let integer = if integer.is_empty() { "0" } else { integer };
            Some(format!("{integer}{}{fraction}", format.decimal))
        }
    }
}

/// Shortens canonical amount text to what `currency` allows, e.g. after
/// switching from EUR to JPY `12,50` becomes `12`.
pub fn fit_amount_text(text: &str, currency: Currency, format: NumberFormat) -> String {
    let Some((integer, fraction)) = text.split_once(format.decimal) else {
        return text.to_string();
    };
    let exponent = currency.exponent() as usize;
    if exponent == 0 {
        integer.to_string()
    } else {
        let kept: String = fraction.chars().take(exponent).collect();
        format!("{integer}{}{kept}", format.decimal)
    }
}

/// Canonical amount text as the field shows it while typing, with
/// thousands separators: `1234,5` → `1.234,5`.
pub fn display_amount_text(text: &str, format: NumberFormat) -> String {
    match text.split_once(format.decimal) {
        Some((integer, fraction)) => format!(
            "{}{}{fraction}",
            group_digits(integer, format.group),
            format.decimal
        ),
        None => group_digits(text, format.group),
    }
}

/// Turns an edit of the grouped field back into plain typed text for
/// [`clean_amount_input`]. `shown` is what the field showed before, `raw`
/// what it holds now. Separators the app inserted are dropped; a separator
/// the user typed is a decimal separator, because the keyboard may only
/// offer `.` or `,`. A pasted `1.234,56` keeps its meaning.
pub fn amount_edit(shown: &str, raw: &str, format: NumberFormat) -> String {
    let old: Vec<char> = shown.chars().collect();
    let new: Vec<char> = raw.chars().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(old.len().min(new.len()) - prefix)
        .take_while(|(a, b)| a == b)
        .count();
    let inserted = &new[prefix..new.len() - suffix];
    let removed = &old[prefix..old.len() - suffix];

    let mut head = old[..prefix].to_vec();
    // Deleting just an inserted separator would change nothing; the user
    // meant the digit in front of it.
    if inserted.is_empty()
        && !removed.is_empty()
        && removed.iter().all(|&c| c == format.group)
        && let Some(index) = head.iter().rposition(char::is_ascii_digit)
    {
        head.remove(index);
    }
    let pasted_grouping = inserted.contains(&format.decimal) && inserted.contains(&format.group);
    let shown_part =
        |chars: &[char]| -> String { chars.iter().filter(|&&c| c != format.group).collect() };
    let typed: String = inserted
        .iter()
        .filter(|&&c| !(pasted_grouping && c == format.group))
        .collect();
    format!(
        "{}{typed}{}",
        shown_part(&head),
        shown_part(&old[old.len() - suffix..])
    )
}

/// One keystroke in an amount field: the new canonical text, or `None` to
/// keep the old one, and whether the field must be redrawn because it does
/// not show what the user typed (rejected or regrouped input).
pub fn amount_keystroke(
    shown: &str,
    raw: &str,
    currency: Currency,
    format: NumberFormat,
) -> (Option<String>, bool) {
    let edit = amount_edit(shown, raw, format);
    match clean_amount_input(&edit, currency, format) {
        Some(text) => {
            let redraw = display_amount_text(&text, format) != raw;
            (Some(text), redraw)
        }
        None => (None, true),
    }
}

/// Canonical text of an amount, to prefill a field: `2000 EUR` → `20`,
/// `1550 EUR` → `15,50`. Negative amounts have no canonical text.
pub fn amount_text(money: Money, format: NumberFormat) -> String {
    let minor = money.amount_minor().max(0).unsigned_abs();
    let exponent = money.currency().exponent();
    let scale = 10_u64.pow(exponent);
    let (integer, fraction) = (minor / scale, minor % scale);
    if fraction == 0 {
        integer.to_string()
    } else {
        format!(
            "{integer}{}{fraction:0width$}",
            format.decimal,
            width = exponent as usize
        )
    }
}

/// The amount of canonical text from [`clean_amount_input`]; `None` while
/// nothing is entered.
pub fn parse_amount(text: &str, currency: Currency, format: NumberFormat) -> Option<Money> {
    let (integer, fraction) = text.split_once(format.decimal).unwrap_or((text, ""));
    if integer.is_empty() && fraction.is_empty() {
        return None;
    }
    let exponent = currency.exponent();
    if fraction.len() > exponent as usize {
        return None;
    }
    let number = |part: &str| {
        if part.is_empty() {
            Some(0)
        } else {
            part.parse::<i64>().ok()
        }
    };
    let padding = 10_i64.pow(exponent - fraction.len() as u32);
    let minor = number(integer)?
        .checked_mul(10_i64.pow(exponent))?
        .checked_add(number(fraction)?.checked_mul(padding)?)?;
    Some(Money::new(minor, currency))
}

const MINUTE_MS: i64 = 60 * 1000;
const HOUR_MS: i64 = 60 * MINUTE_MS;
const DAY_MS: i64 = 24 * HOUR_MS;

/// "gerade eben", "vor 5 Min.", "vor 3 Std.", "vor 2 Tagen".
pub fn age_text(then_ms: i64, now_ms: i64) -> String {
    let age = (now_ms - then_ms).max(0);
    if age < MINUTE_MS {
        t!("age.just_now").to_string()
    } else if age < HOUR_MS {
        t!("age.minutes", count = age / MINUTE_MS).to_string()
    } else if age < DAY_MS {
        t!("age.hours", count = age / HOUR_MS).to_string()
    } else if age < 2 * DAY_MS {
        t!("age.day").to_string()
    } else {
        t!("age.days", count = age / DAY_MS).to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    const DE: NumberFormat = NumberFormat {
        decimal: ',',
        group: '.',
        symbol_before: false,
    };
    const EN: NumberFormat = NumberFormat {
        decimal: '.',
        group: ',',
        symbol_before: true,
    };

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn money(minor: i64, code: &str) -> Money {
        Money::new(minor, cur(code))
    }

    #[test]
    fn formats_money_per_language() {
        assert_eq!(format_money(money(123_456, "EUR"), DE), "1.234,56\u{a0}€");
        assert_eq!(format_money(money(123_456, "EUR"), EN), "€1,234.56");
        assert_eq!(format_money(money(-1250, "EUR"), DE), "-12,50\u{a0}€");
        assert_eq!(format_money(money(-1250, "EUR"), EN), "-€12.50");
        assert_eq!(format_money(money(5, "EUR"), DE), "0,05\u{a0}€");
        assert_eq!(format_money(money(0, "EUR"), DE), "0,00\u{a0}€");
    }

    #[test]
    fn decimals_follow_the_currency() {
        assert_eq!(format_money(money(1_000, "JPY"), DE), "1.000\u{a0}¥");
        assert_eq!(format_money(money(1_234_567, "JPY"), EN), "¥1,234,567");
        assert_eq!(format_money(money(1_005, "KWD"), DE), "1,005\u{a0}KWD");
        assert_eq!(format_money(money(i64::MIN, "JPY"), DE).len(), 30);
    }

    #[test]
    fn ambiguous_symbols_show_the_code() {
        assert_eq!(format_money(money(1250, "USD"), DE), "12,50\u{a0}USD");
        assert_eq!(format_money(money(1250, "USD"), EN), "USD\u{a0}12.50");
        assert_eq!(currency_symbol(cur("CNY")), "CNY");
        assert_eq!(currency_symbol(cur("CHF")), "CHF");
        assert_eq!(currency_symbol(cur("GBP")), "£");
    }

    #[test]
    fn formats_rates_with_few_digits() {
        let d = |s| Decimal::from_str(s).unwrap();
        assert_eq!(format_rate(d("177.71378084"), DE), "177,71");
        assert_eq!(format_rate(d("0.005627"), DE), "0,005627");
        assert_eq!(format_rate(d("0.00562713"), EN), "0.0056271");
        assert_eq!(format_rate(d("16234.9"), DE), "16.235");
        assert_eq!(format_rate(d("1"), DE), "1");
    }

    #[test]
    fn cleans_typed_amounts() {
        let eur = cur("EUR");
        let clean = |raw| clean_amount_input(raw, eur, DE);
        assert_eq!(clean("").as_deref(), Some(""));
        assert_eq!(clean("12").as_deref(), Some("12"));
        assert_eq!(clean("12.5").as_deref(), Some("12,5"));
        assert_eq!(clean("12,50").as_deref(), Some("12,50"));
        assert_eq!(clean(",5").as_deref(), Some("0,5"));
        assert_eq!(clean("007").as_deref(), Some("7"));
        assert_eq!(clean("0").as_deref(), Some("0"));
        assert_eq!(clean("12,").as_deref(), Some("12,"));
        assert_eq!(clean("12,345"), None);
        assert_eq!(clean("1,2,3"), None);
        assert_eq!(clean("1.234,5"), None);
        assert_eq!(clean("-5"), None);
        assert_eq!(clean("12a"), None);
        assert_eq!(clean("1234567890123"), None);
        assert_eq!(clean_amount_input("12,5", eur, EN).as_deref(), Some("12.5"));
    }

    #[test]
    fn whole_unit_currencies_take_no_separator() {
        assert_eq!(
            clean_amount_input("1000", cur("JPY"), DE).as_deref(),
            Some("1000")
        );
        assert_eq!(clean_amount_input("1000,", cur("JPY"), DE), None);
        assert_eq!(
            clean_amount_input("1,005", cur("KWD"), DE).as_deref(),
            Some("1,005")
        );
    }

    #[test]
    fn fits_text_to_another_currency() {
        assert_eq!(fit_amount_text("12,50", cur("JPY"), DE), "12");
        assert_eq!(fit_amount_text("1,005", cur("EUR"), DE), "1,00");
        assert_eq!(fit_amount_text("12,5", cur("KWD"), DE), "12,5");
        assert_eq!(fit_amount_text("1000", cur("EUR"), DE), "1000");
    }

    #[test]
    fn shows_thousands_while_typing() {
        assert_eq!(display_amount_text("", DE), "");
        assert_eq!(display_amount_text("123", DE), "123");
        assert_eq!(display_amount_text("1234", DE), "1.234");
        assert_eq!(display_amount_text("1234567,5", DE), "1.234.567,5");
        assert_eq!(display_amount_text("1234,", DE), "1.234,");
        assert_eq!(display_amount_text("1234.56", EN), "1,234.56");
    }

    /// Typing `key` at the end of the field, as the browser reports it.
    fn type_at_end(text: &str, key: &str, format: NumberFormat) -> Option<String> {
        let shown = display_amount_text(text, format);
        let edit = amount_edit(&shown, &format!("{shown}{key}"), format);
        clean_amount_input(&edit, cur("EUR"), format)
    }

    #[test]
    fn typed_separator_is_the_decimal_one() {
        assert_eq!(type_at_end("123", "4", DE).as_deref(), Some("1234"));
        assert_eq!(type_at_end("1234", "5", DE).as_deref(), Some("12345"));
        // German grouping uses '.', yet a typed '.' still means decimal.
        assert_eq!(type_at_end("1234", ".", DE).as_deref(), Some("1234,"));
        assert_eq!(type_at_end("1234", ",", DE).as_deref(), Some("1234,"));
        assert_eq!(type_at_end("1234", ",", EN).as_deref(), Some("1234."));
        assert_eq!(type_at_end("1234,5", "6", DE).as_deref(), Some("1234,56"));
        assert_eq!(type_at_end("1234,56", "7", DE), None);
        assert_eq!(type_at_end("1234,5", ".", DE), None);
    }

    #[test]
    fn edits_in_the_grouped_text() {
        // Backspace at the end.
        assert_eq!(amount_edit("12.345", "12.34", DE), "1234");
        // Deleting the separator deletes the digit in front of it.
        assert_eq!(amount_edit("1.234", "1234", DE), "234");
        // A digit typed in the middle.
        assert_eq!(amount_edit("1.234", "19.234", DE), "19234");
        // Pasted with grouping into an empty field.
        assert_eq!(amount_edit("", "1.234,56", DE), "1234,56");
        assert_eq!(amount_edit("", "1,234.56", EN), "1234.56");
        assert_eq!(amount_edit("", "12.5", DE), "12.5");
    }

    #[test]
    fn parses_canonical_text() {
        let eur = cur("EUR");
        assert_eq!(parse_amount("", eur, DE), None);
        assert_eq!(parse_amount(",", eur, DE), None);
        assert_eq!(parse_amount("12", eur, DE), Some(money(1200, "EUR")));
        assert_eq!(parse_amount("12,5", eur, DE), Some(money(1250, "EUR")));
        assert_eq!(parse_amount("0,05", eur, DE), Some(money(5, "EUR")));
        assert_eq!(parse_amount("12,", eur, DE), Some(money(1200, "EUR")));
        assert_eq!(
            parse_amount("1000", cur("JPY"), DE),
            Some(money(1000, "JPY"))
        );
        assert_eq!(
            parse_amount("1,5", cur("KWD"), DE),
            Some(money(1500, "KWD"))
        );
        assert_eq!(parse_amount("1,005", eur, DE), None);
    }

    #[test]
    fn typed_amount_round_trips_through_display() {
        let eur = cur("EUR");
        let text = clean_amount_input("1234.5", eur, DE).unwrap();
        let amount = parse_amount(&text, eur, DE).unwrap();
        assert_eq!(format_money(amount, DE), "1.234,50\u{a0}€");
    }

    #[test]
    fn amount_text_round_trips_through_parse() {
        let eur = Currency::from_code("EUR").unwrap();
        let jpy = Currency::from_code("JPY").unwrap();
        for (minor, currency, text) in [
            (2_000, eur, "20"),
            (1_550, eur, "15,50"),
            (5, eur, "0,05"),
            (3_000, jpy, "3000"),
        ] {
            let money = Money::new(minor, currency);
            assert_eq!(amount_text(money, DE), text);
            assert_eq!(parse_amount(text, currency, DE), Some(money));
        }
        assert_eq!(amount_text(Money::new(1_550, eur), EN), "15.50");
    }

    #[test]
    fn keystrokes_are_kept_rejected_or_redrawn() {
        let eur = Currency::from_code("EUR").unwrap();
        assert_eq!(
            amount_keystroke("12", "123", eur, DE),
            (Some("123".into()), false)
        );
        // Grouping appears: the field must show "1.234".
        assert_eq!(
            amount_keystroke("123", "1234", eur, DE),
            (Some("1234".into()), true)
        );
        assert_eq!(amount_keystroke("1,23", "1,234", eur, DE), (None, true));
    }
}
