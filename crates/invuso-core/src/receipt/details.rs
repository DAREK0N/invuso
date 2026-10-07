//! Header and footer of a receipt (OCR-16): who sold, when, in which
//! currency and how it was paid. Everything here is a suggestion the user
//! can overwrite (idee.md 1.4 principle 5), so an unclear receipt yields
//! nothing rather than a guess.

use std::collections::BTreeMap;

use super::language::detect_language;
use super::tokens::keyword_form;
use super::{ReceiptRow, RowKind};
use crate::domain::{Currency, is_iso_date};

/// How the receipt says it was paid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentKind {
    Cash,
    Card,
}

/// What the receipt says besides its items.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReceiptDetails {
    /// The shop's name as printed in the header.
    pub merchant: Option<String>,
    /// Day of the purchase, `YYYY-MM-DD`.
    pub date: Option<String>,
    /// Time of the purchase, `HH:MM`.
    pub time: Option<String>,
    pub payment: Option<PaymentKind>,
}

/// Reads merchant, date, time and payment from the classified rows.
pub(super) fn read_details(rows: &[ReceiptRow]) -> ReceiptDetails {
    let japanese = detect_language(rows.iter().map(|row| row.text.as_str())) == Some("ja");
    // US tills print the month first (`03/18/2021`).
    let month_first = rows.iter().any(|row| row.text.contains('$'));
    let dated = rows.iter().enumerate().find_map(|(index, row)| {
        (!is_validity_row(&row.text))
            .then(|| find_date(&row.text, japanese, month_first))
            .flatten()
            .map(|date| (index, date))
    });
    let time = match dated {
        // The time printed next to the date, else the closest one.
        Some((at, _)) => {
            let mut nearest: Vec<usize> = (0..rows.len()).collect();
            nearest.sort_by_key(|index| index.abs_diff(at));
            nearest
                .into_iter()
                .find_map(|index| find_time(&rows[index].text))
        }
        None => rows.iter().find_map(|row| find_time(&row.text)),
    };
    ReceiptDetails {
        merchant: find_merchant(rows),
        date: dated.map(|(_, date)| date),
        time,
        payment: find_payment(rows),
    }
}

/// The currency the receipt's amounts are in, read from currency signs and
/// codes (`€`, `EUR`, `¥`, `円`, `CHF` …); a Japanese or Korean receipt
/// without any is in yen or won. `None` if the receipt names none or
/// several equally often. A lone `$` counts as US dollars only when nothing
/// else is printed.
pub fn detect_currency<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<Currency> {
    let texts: Vec<&str> = texts.into_iter().collect();
    let mut votes: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut dollars = 0_usize;
    for text in &texts {
        let has_digit = text.chars().any(|c| c.is_ascii_digit());
        let alone = text.split_whitespace().count() == 1;
        for (signs, code) in CURRENCY_SIGNS {
            if signs.iter().any(|sign| text.contains(sign)) {
                *votes.entry(code).or_default() += 1;
            }
        }
        if text.contains('$') && !text.contains("US$") {
            dollars += 1;
        }
        for word in text.split(|c: char| !c.is_alphabetic()) {
            let code = match word.to_ascii_uppercase().as_str() {
                "EURO" | "EUROS" => "EUR",
                "YEN" => "JPY",
                _ if word.len() == 3
                    && word.chars().all(|c| c.is_ascii_uppercase())
                    && (has_digit || alone)
                    && !CODE_LIKE_WORDS.contains(&word) =>
                {
                    match Currency::from_code(word) {
                        Ok(currency) => currency.code(),
                        Err(_) => continue,
                    }
                }
                _ => continue,
            };
            *votes.entry(code).or_default() += 1;
        }
    }
    let best = votes.values().copied().max().unwrap_or(0);
    let mut leaders = votes.iter().filter(|(_, count)| **count == best);
    let code = match (leaders.next(), leaders.next()) {
        (Some((code, _)), None) => Some(*code),
        (Some(_), Some(_)) => return None,
        _ if dollars > 0 => Some("USD"),
        _ => match detect_language(texts.iter().copied()) {
            Some("ja") => Some("JPY"),
            Some("ko") => Some("KRW"),
            _ => None,
        },
    };
    code.and_then(|code| Currency::from_code(code).ok())
}

/// Signs that name one currency only.
const CURRENCY_SIGNS: &[(&[&str], &str)] = &[
    (&["€"], "EUR"),
    (&["¥", "￥", "円"], "JPY"),
    (&["£"], "GBP"),
    (&["US$"], "USD"),
    (&["₩", "원"], "KRW"),
    (&["₹"], "INR"),
    (&["฿"], "THB"),
    (&["₺"], "TRY"),
    (&["₽"], "RUB"),
    (&["₫"], "VND"),
    (&["₱"], "PHP"),
    (&["₪"], "ILS"),
    (&["zł"], "PLN"),
    (&["Kč"], "CZK"),
];

/// ISO codes that are also words on a receipt (`ALL`, `TOP`, `GEL`) or
/// abbreviations of something else.
const CODE_LIKE_WORDS: &[&str] = &[
    "ALL", "TOP", "CUP", "PEN", "TRY", "MOP", "BOB", "SOS", "BAM", "GEL", "MAD", "ANG",
];

/// Rows naming another date than the purchase: best before, valid until.
fn is_validity_row(text: &str) -> bool {
    let validity_word = text
        .split_whitespace()
        .map(keyword_form)
        .any(|word| ["bis", "gültig", "mhd", "valid", "until", "expires"].contains(&word.as_str()));
    validity_word || ["期限", "まで"].iter().any(|word| text.contains(word))
}

/// Whether the row holds a date of purchase.
pub(super) fn is_dated(text: &str, japanese: bool) -> bool {
    !is_validity_row(text) && find_date(text, japanese, false).is_some()
}

/// A date in the row: `2022年9月17日`, `2015/03/22`, `2026.06.21`,
/// `2020-12-04`, `04.05.19`, `12/06/09`, `10.09.2020` (also with the time
/// glued on: `10.09.202017:01`). Day and month are read in European order
/// unless the month would be above 12. On Japanese receipts a two-digit
/// year is a year of the imperial era (`29.04.01`), so it is skipped.
fn find_date(text: &str, japanese: bool, month_first: bool) -> Option<String> {
    kanji_date(text).or_else(|| numeric_date(text, japanese, month_first))
}

fn kanji_date(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == '年')
        .find_map(|(at, _)| {
            let year_start = at.checked_sub(4)?;
            if year_start > 0 && chars[year_start - 1].is_ascii_digit() {
                return None;
            }
            let year = number(&chars[year_start..at])?;
            let (month, after_month) = digits_then(&chars, at + 1, '月')?;
            let (day, _) = digits_then(&chars, after_month, '日')?;
            iso_date(year, month, day)
        })
}

/// One or two digits from `start` followed by `mark`, with spaces around
/// them (`2022年 9月17日`); the number and the index after the mark.
fn digits_then(chars: &[char], start: usize, mark: char) -> Option<(u32, usize)> {
    let start = skip_spaces(chars, start);
    let run = digit_run(chars, start);
    let mark_at = skip_spaces(chars, start + run);
    if !(1..=2).contains(&run) || chars.get(mark_at) != Some(&mark) {
        return None;
    }
    Some((number(&chars[start..start + run])?, mark_at + 1))
}

fn skip_spaces(chars: &[char], start: usize) -> usize {
    let spaces = chars.get(start..).map_or(0, |rest| {
        rest.iter().take_while(|c| c.is_whitespace()).count()
    });
    start + spaces
}

fn numeric_date(text: &str, japanese: bool, month_first: bool) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    for start in 0..chars.len() {
        let starts_number = chars[start].is_ascii_digit()
            && (start == 0
                || !(chars[start - 1].is_ascii_digit()
                    || matches!(chars[start - 1], '.' | ',' | '/' | '-')));
        if !starts_number {
            continue;
        }
        if let Some(date) = numeric_date_at(&chars, start, japanese, month_first) {
            return Some(date);
        }
    }
    None
}

fn numeric_date_at(
    chars: &[char],
    start: usize,
    japanese: bool,
    month_first: bool,
) -> Option<String> {
    let first_len = digit_run(chars, start);
    let separator = *chars.get(start + first_len)?;
    if !matches!(separator, '.' | '/' | '-') {
        return None;
    }
    // Tills pad one-digit parts with a space (`2019/11/ 9`).
    let second_at = skip_spaces(chars, start + first_len + 1);
    let second_len = digit_run(chars, second_at);
    if !(1..=2).contains(&second_len) || chars.get(second_at + second_len) != Some(&separator) {
        return None;
    }
    let third_at = skip_spaces(chars, second_at + second_len + 1);
    let third_len = digit_run(chars, third_at);
    let first = number(&chars[start..start + first_len])?;
    let second = number(&chars[second_at..second_at + second_len])?;
    if first_len == 4 {
        if !(1..=2).contains(&third_len) {
            return None;
        }
        let day = number(&chars[third_at..third_at + third_len])?;
        return iso_date(first, second, day);
    }
    if !(1..=2).contains(&first_len) {
        return None;
    }
    let year = match third_len {
        2 if !japanese => 2000 + number(&chars[third_at..third_at + 2])?,
        4 => number(&chars[third_at..third_at + 4])?,
        // `10.09.202017:01`: the hour glued to a four-digit year.
        5 | 6 if chars.get(third_at + third_len) == Some(&':') => {
            number(&chars[third_at..third_at + 4])?
        }
        _ => return None,
    };
    let (day, month) = if (second > 12 || month_first) && first <= 12 && second <= 31 {
        (second, first)
    } else {
        (first, second)
    };
    iso_date(year, month, day)
}

fn iso_date(year: u32, month: u32, day: u32) -> Option<String> {
    let date = format!("{year:04}-{month:02}-{day:02}");
    ((2000..=2099).contains(&year) && is_iso_date(&date)).then_some(date)
}

fn digit_run(chars: &[char], start: usize) -> usize {
    chars.get(start..).map_or(0, |rest| {
        rest.iter().take_while(|c| c.is_ascii_digit()).count()
    })
}

fn number(chars: &[char]) -> Option<u32> {
    if chars.is_empty() || !chars.iter().all(char::is_ascii_digit) {
        return None;
    }
    chars.iter().collect::<String>().parse().ok()
}

/// Marks between two times of a range (`6:00-24:00`, `10:00~19:00`).
const RANGE_MARKS: &[char] = &['-', '~', '〜', '～', '–'];

/// A time of day in the row, `HH:MM`; opening hours are no purchase time.
fn find_time(text: &str) -> Option<String> {
    if text.contains("営業") {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    // `17時28分`
    let kanji = chars
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == '時')
        .find_map(|(at, _)| {
            let mut start = at;
            while start > 0 && at - start < 2 && chars[start - 1].is_ascii_digit() {
                start -= 1;
            }
            let hour = number(&chars[start..at])?;
            let (minute, _) = digits_then(&chars, at + 1, '分')?;
            (hour <= 23 && minute <= 59).then(|| format!("{hour:02}:{minute:02}"))
        });
    if kanji.is_some() {
        return kanji;
    }
    for (at, _) in chars
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c, ':' | '：'))
    {
        let mut hour_start = at;
        while hour_start > 0 && chars[hour_start - 1].is_ascii_digit() {
            hour_start -= 1;
        }
        let hour_run = at - hour_start;
        let minute_run = digit_run(&chars, at + 1);
        // A longer run before the colon is a year with the hour glued on.
        if !matches!(hour_run, 1 | 2 | 4 | 6) || minute_run != 2 {
            continue;
        }
        let hour = number(&chars[at - hour_run.min(2)..at])?;
        let minute = number(&chars[at + 1..at + 3])?;
        let mut end = at + 3;
        if chars.get(end) == Some(&':') && digit_run(&chars, end + 1) == 2 {
            end += 3;
        }
        let before = chars[..hour_start]
            .iter()
            .rev()
            .find(|c| !c.is_whitespace());
        let after = chars[end..].iter().find(|c| !c.is_whitespace());
        let in_range = [before, after]
            .into_iter()
            .flatten()
            .any(|c| RANGE_MARKS.contains(c));
        if hour <= 23 && minute <= 59 && !in_range {
            return Some(format!("{hour:02}:{minute:02}"));
        }
    }
    None
}

/// Of the header rows (above the first item), the one most likely the
/// shop's name, in this order: the row right above the address (`EDEKA
/// Hornung` over `Hauptstraße 115`), a row naming a company (`IKEA
/// Deutschland GmbH & Co.KG`, without its legal form), the row printed
/// largest. Logos and slogans are printed largest but read badly, so they
/// only count when nothing else does. Addresses, phone numbers, web
/// addresses, dates and slip numbers are no name.
fn find_merchant(rows: &[ReceiptRow]) -> Option<String> {
    let header_end = rows
        .iter()
        .position(|row| row.kind != RowKind::Other)
        .unwrap_or(rows.len());
    let header = &rows[..header_end.min(MERCHANT_ROWS)];
    let clean = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");

    // A street the word list does not know (`Kiliansgraben 26`) still ends
    // in a house number right above the postal code.
    let mut address = header.iter().position(|row| is_address(&row.text));
    while let Some(at) = address
        && at > 0
        && ends_in_house_number(&header[at - 1].text)
    {
        address = Some(at - 1);
    }
    let above_address = address
        .and_then(|at| at.checked_sub(1))
        .map(|at| &header[at])
        .filter(|row| is_name_like(&row.text));
    if let Some(row) = above_address {
        return Some(without_legal_form(&row.text).unwrap_or_else(|| clean(&row.text)));
    }
    let company = header
        .iter()
        .filter(|row| is_name_like(&row.text))
        .find_map(|row| without_legal_form(&row.text));
    if company.is_some() {
        return company;
    }
    let candidates: Vec<&ReceiptRow> = header
        .iter()
        .filter(|row| is_name_like(&row.text))
        .collect();
    let tallest = candidates.iter().map(|row| row.bbox.height()).max()?;
    candidates
        .into_iter()
        .find(|row| row.bbox.height() == tallest)
        .map(|row| clean(&row.text))
}

/// The shop's name is printed in the first rows.
const MERCHANT_ROWS: usize = 12;

/// Legal forms of companies (lower case, letters only).
const LEGAL_FORMS: &[&str] = &[
    "gmbh", "kg", "ek", "ag", "se", "ohg", "ug", "gbr", "inc", "ltd", "llc", "sarl", "sas", "srl",
    "spa", "bv",
];

const JAPANESE_LEGAL_FORMS: &[&str] = &["株式会社", "有限会社"];

/// The text before a legal form (`Smyths Toys Deutschland SE & Co. KG` →
/// `Smyths Toys Deutschland`); `None` if the row names no company or
/// nothing is left in front of it.
fn without_legal_form(text: &str) -> Option<String> {
    // Japanese receipts name the operating company (`株式会社アントワークス`)
    // beside the shop's brand; the brand is the name people know.
    if JAPANESE_LEGAL_FORMS.iter().any(|form| text.contains(*form)) {
        return None;
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    let at = words.iter().position(|word| {
        LEGAL_FORMS.contains(&keyword_form(word).as_str()) || word.contains("GmbH")
    })?;
    let name = words[..at]
        .iter()
        .copied()
        .filter(|word| *word != "&")
        .collect::<Vec<_>>()
        .join(" ");
    (name.chars().filter(|c| c.is_alphabetic()).count() >= 2).then_some(name)
}

/// `Kiliansgraben 26`, `Am Markt 3a`: words, then a house number.
fn ends_in_house_number(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    let [.., street, number] = words.as_slice() else {
        return false;
    };
    let digits = number.trim_end_matches(|c: char| c.is_ascii_lowercase());
    street.chars().any(char::is_alphabetic)
        && (1..=4).contains(&digits.len())
        && digits.chars().all(|c| c.is_ascii_digit())
}

/// A street with its number (`Hauptstraße 115`, `Musterstr. 1`) or a postal
/// code with its town (`37355 Niederorschel`).
fn is_address(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    let street = text.chars().any(|c| c.is_ascii_digit())
        && words.iter().any(|word| {
            let form = keyword_form(word);
            STREET_ENDINGS.iter().any(|end| form.ends_with(end))
        });
    let postal = words.windows(2).any(|pair| {
        (4..=5).contains(&pair[0].len())
            && pair[0].chars().all(|c| c.is_ascii_digit())
            && pair[1].chars().next().is_some_and(char::is_alphabetic)
    });
    street || postal
}

/// Words of header rows that are no shop name.
const NOT_A_NAME: &[&str] = &[
    "tel",
    "fax",
    "niederlassung",
    "telefon",
    "phone",
    "steuer",
    "steuernummer",
    "ust",
    "uid",
    "rechnung",
    "beleg",
    "quittung",
    "kassenbon",
    "bon",
    "kasse",
    "filiale",
    "datum",
    "uhrzeit",
    "bediener",
    "kassierer",
    "tisch",
    "willkommen",
    "welcome",
    "danke",
    "receipt",
    "invoice",
    "eur",
];

/// Parts of header rows that are no shop name: advertising (`★`, `!`),
/// receipt and slip labels, staff, phone, opening hours, addresses and the
/// column header of the item table.
const NOT_A_JAPANESE_NAME: &[&str] = &[
    "★",
    "☆",
    "!",
    "！",
    ":",
    "：",
    "領収",
    "領取",
    "レシ",
    "伝票",
    "担当",
    "責任",
    "電話",
    "営業",
    "お買",
    "登録",
    "県",
    "市",
    "区",
    "丁目",
    "番地",
    "ビル",
    "金額",
    "金额",
    "数量",
    "品名",
    "ありがと",
    "お待ち",
    "下さい",
    "ください",
    "願い",
    "お越し",
    "ご来店",
    "ご利用",
    "株式会社",
    "有限会社",
    "アプリ",
];

/// Street words that make a row with a number an address.
const STREET_ENDINGS: &[&str] = &[
    "str", "straße", "strasse", "weg", "platz", "allee", "gasse", "ring", "damm",
];

fn is_name_like(text: &str) -> bool {
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    let digits = text.chars().filter(char::is_ascii_digit).count();
    let lower = text.to_lowercase();
    let words: Vec<String> = text
        .split_whitespace()
        .map(keyword_form)
        .filter(|word| !word.is_empty())
        .collect();
    let web = ["@", "www", "http", ".de", ".com", ".jp"]
        .iter()
        .any(|part| lower.contains(part));
    let street = digits > 0
        && words
            .iter()
            .any(|word| STREET_ENDINGS.iter().any(|end| word.ends_with(end)));
    letters >= 2
        && digits <= 2
        && !web
        && !street
        && !words.iter().any(|word| NOT_A_NAME.contains(&word.as_str()))
        && !NOT_A_JAPANESE_NAME.iter().any(|part| text.contains(part))
}

/// Leading words of rows that say the receipt was paid in cash.
const CASH_WORDS: &[&str] = &[
    "bar",
    "barzahlung",
    "bargeld",
    "gegeben",
    "bargegeben",
    "cash",
    "espèces",
    "especes",
    "efectivo",
    "contanti",
];

/// Leading words of rows that say it was paid by card.
const CARD_WORDS: &[&str] = &[
    "karte",
    "kartenzahlung",
    "ec",
    "eckarte",
    "eccash",
    "girocard",
    "kreditkarte",
    "debitkarte",
    "visa",
    "mastercard",
    "maestro",
    "vpay",
    "amex",
    "card",
    "credit",
    "debit",
    "carte",
    "tarjeta",
    "carta",
];

/// Cash or card, if the receipt names one of them and not both.
fn find_payment(rows: &[ReceiptRow]) -> Option<PaymentKind> {
    let (mut cash, mut card) = (false, false);
    for row in rows {
        let leading = row
            .text
            .split_whitespace()
            .map(keyword_form)
            .find(|word| !word.is_empty());
        if let Some(word) = leading.as_deref() {
            cash |= CASH_WORDS.contains(&word);
            card |= CARD_WORDS.contains(&word);
        }
        cash |= row.text.contains("現金");
        card |= ["クレジット", "カード"]
            .iter()
            .any(|word| row.text.contains(word));
    }
    match (cash, card) {
        (true, false) => Some(PaymentKind::Cash),
        (false, true) => Some(PaymentKind::Card),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt::BoundingBox;

    fn row(text: &str, height: i32, kind: RowKind) -> ReceiptRow {
        ReceiptRow {
            text: text.to_string(),
            bbox: BoundingBox {
                left: 0,
                top: 0,
                right: 100,
                bottom: height,
            },
            kind,
            fragments: Vec::new(),
        }
    }

    fn details(texts: &[&str]) -> ReceiptDetails {
        let rows: Vec<ReceiptRow> = texts
            .iter()
            .map(|text| row(text, 20, RowKind::Other))
            .collect();
        read_details(&rows)
    }

    fn date(text: &str) -> Option<String> {
        find_date(text, false, false)
    }

    #[test]
    fn dates_in_the_usual_layouts() {
        assert_eq!(date("04.05.19 12:00").as_deref(), Some("2019-05-04"));
        assert_eq!(date("Datum: 02.10.2026").as_deref(), Some("2026-10-02"));
        assert_eq!(date("2015/03/22 08:07").as_deref(), Some("2015-03-22"));
        assert_eq!(date("2026.06.21 17:30").as_deref(), Some("2026-06-21"));
        assert_eq!(
            date("2020-12-04T09:36:54.000+0100").as_deref(),
            Some("2020-12-04")
        );
        assert_eq!(date("10.09.202017:01").as_deref(), Some("2020-09-10"));
        assert_eq!(date("12/06/09").as_deref(), Some("2009-06-12"));
        // The month is the one that can be one.
        assert_eq!(date("10/23/2025").as_deref(), Some("2025-10-23"));
        // On a dollar receipt the month comes first.
        assert_eq!(
            find_date("03/04/2021", false, true).as_deref(),
            Some("2021-03-04")
        );
        assert_eq!(date("2022年 9月17日 13:07").as_deref(), Some("2022-09-17"));
        assert_eq!(date("2022年3月 3日(木)7:49").as_deref(), Some("2022-03-03"));
        assert_eq!(
            date("47 レシNO 01 2022年3月7日 (月)07:43").as_deref(),
            Some("2022-03-07")
        );
        assert_eq!(
            date("レシ0103 2019/11/ 9(±) 20:20").as_deref(),
            Some("2019-11-09")
        );
    }

    #[test]
    fn numbers_that_are_no_dates() {
        for text in [
            "1.234,56",
            "12.345.678,90",
            "1.2.3",
            "31.02.2024",
            "Tel. 089/123-45",
            "13.13.13",
            "4047777185192",
            "12:00",
        ] {
            assert_eq!(date(text), None, "{text}");
        }
        // A two-digit year on a Japanese receipt counts by the era.
        assert_eq!(find_date("29.04.01", true, false), None);
        assert_eq!(
            find_date("2017.04.01", true, false).as_deref(),
            Some("2017-04-01")
        );
    }

    #[test]
    fn times_and_opening_hours() {
        assert_eq!(find_time("04.05.19 12:00").as_deref(), Some("12:00"));
        assert_eq!(find_time("(木)7:49").as_deref(), Some("07:49"));
        assert_eq!(find_time("10.09.202017:01").as_deref(), Some("17:01"));
        assert_eq!(find_time("09:36:54").as_deref(), Some("09:36"));
        assert_eq!(find_time("13：07").as_deref(), Some("13:07"));
        assert_eq!(
            find_time("2025年10月31日(金) 17時28分").as_deref(),
            Some("17:28")
        );
        for text in [
            "営業時間6:00-24:00",
            "10:00~19:00(全日)",
            "25:00",
            "12:5",
            "Mo-Fr",
        ] {
            assert_eq!(find_time(text), None, "{text}");
        }
    }

    #[test]
    fn time_next_to_the_date_wins() {
        let found = details(&["Kasse 2 08:15", "Brot 1,00", "04.05.19 12:00"]);
        assert_eq!(found.date.as_deref(), Some("2019-05-04"));
        assert_eq!(found.time.as_deref(), Some("12:00"));
        let found = details(&["(月)07:43", "2022年3月7日"]);
        assert_eq!(found.time.as_deref(), Some("07:43"));
        // A best-before date is not the day of purchase.
        let found = details(&["Gutschein gültig bis 31.12.2026", "01.10.2026"]);
        assert_eq!(found.date.as_deref(), Some("2026-10-01"));
    }

    #[test]
    fn merchant_is_the_largest_name_in_the_header() {
        let rows = [
            row("Willkommen bei", 20, RowKind::Other),
            row("Bäckerei  Muster", 40, RowKind::Other),
            row("Musterstraße 1", 20, RowKind::Other),
            row("50000 Köln", 20, RowKind::Other),
            row("Tel. 0221 000000", 20, RowKind::Other),
            row("Brot 1,00", 20, RowKind::Item),
            row("BIG TEXT LATER", 60, RowKind::Other),
        ];
        assert_eq!(
            read_details(&rows).merchant.as_deref(),
            Some("Bäckerei Muster")
        );
        // Same size: the first name.
        let found = details(&["Fressnapf Köln", "Musterstraße 1", "www.fressnapf.de"]);
        assert_eq!(found.merchant.as_deref(), Some("Fressnapf Köln"));
        let found = details(&["2019年11月12日 07:42", "伝票No 985", "愛知県名古屋市中区"]);
        assert_eq!(found.merchant, None);
    }

    #[test]
    fn merchant_above_the_address_beats_a_large_slogan() {
        // Edeka: logo, slogan in script (read as nonsense, printed largest),
        // then name and address.
        let rows = [
            row("EDEKA", 60, RowKind::Other),
            row("HORNUNG", 70, RowKind::Other),
            row("NkAfrde", 90, RowKind::Other),
            row("EDEKA Hornung", 20, RowKind::Other),
            row("Hauptstraße 115", 20, RowKind::Other),
            row("37355 Niederorschel", 20, RowKind::Other),
            row("Brot 1,00", 20, RowKind::Item),
        ];
        assert_eq!(
            read_details(&rows).merchant.as_deref(),
            Some("EDEKA Hornung")
        );
        // A company's name without its legal form.
        // A street without a known ending, above the postal code.
        let found = details(&[
            "dm dm-drogerie markt",
            "Kiliansgraben 26",
            "99999 Musterstadt",
        ]);
        assert_eq!(found.merchant.as_deref(), Some("dm dm-drogerie markt"));
        let found = details(&["Smyths Toys Deutschland SE & Co. KG", "Filiale Leipzig"]);
        assert_eq!(found.merchant.as_deref(), Some("Smyths Toys Deutschland"));
        let found = details(&["Leoni Schneider e.K.", "Mühlhäuser Landstr. 37a"]);
        assert_eq!(found.merchant.as_deref(), Some("Leoni Schneider"));
        // Japanese receipts: the brand, not the operating company.
        let found = details(&["伝説のすた丼屋", "株式会社アントワークス"]);
        assert_eq!(found.merchant.as_deref(), Some("伝説のすた丼屋"));
    }

    #[test]
    fn payment_by_cash_or_card() {
        use PaymentKind::*;
        assert_eq!(details(&["Bar 20,00"]).payment, Some(Cash));
        assert_eq!(details(&["BAR GEGEBEN: 25,00"]).payment, Some(Cash));
        assert_eq!(
            details(&["Karte 0,77", "Kartenzahlung"]).payment,
            Some(Card)
        );
        assert_eq!(details(&["EC-Cash 3,20"]).payment, Some(Card));
        assert_eq!(details(&["預り：現金 ¥1,510"]).payment, Some(Cash));
        assert_eq!(details(&["クレジット ¥1,000"]).payment, Some(Card));
        // Advertising for a loyalty card says nothing.
        assert_eq!(
            details(&["Bar Euro 50,09", "Eine PAYBACK Karte erhalten Sie"]).payment,
            Some(Cash)
        );
        assert_eq!(details(&["Bar 10,00", "Karte 5,00"]).payment, None);
        assert_eq!(details(&["Summe 5,00"]).payment, None);
    }

    fn currency(texts: &[&str]) -> Option<&'static str> {
        detect_currency(texts.iter().copied()).map(|c| c.code())
    }

    #[test]
    fn currency_from_signs_and_codes() {
        assert_eq!(currency(&["Brot 3,20 €"]), Some("EUR"));
        assert_eq!(currency(&["EUR", "Brot 3,20"]), Some("EUR"));
        assert_eq!(currency(&["SUMME EUR 3,60"]), Some("EUR"));
        assert_eq!(currency(&["Bar Euro 50,09"]), Some("EUR"));
        assert_eq!(currency(&["おにぎり ¥150"]), Some("JPY"));
        assert_eq!(currency(&["1,410円"]), Some("JPY"));
        assert_eq!(currency(&["TOTAL CHF 12.50"]), Some("CHF"));
        assert_eq!(currency(&["Tea £2.50"]), Some("GBP"));
        assert_eq!(currency(&["TOTAL $12.50"]), Some("USD"));
        assert_eq!(currency(&["TOTAL 12.50 THB"]), Some("THB"));
        // Kana without any sign: yen.
        assert_eq!(currency(&["ラーメン 330", "合計 330"]), Some("JPY"));
    }

    #[test]
    fn unclear_currency_is_none() {
        assert_eq!(currency(&["Brot 3,20", "Summe 3,20"]), None);
        assert_eq!(currency(&["Wurst 3,20 €", "Total CHF 3.50"]), None);
        // Capital words are no codes: `ALL`, a code without an amount.
        assert_eq!(currency(&["ALL YOU CAN EAT 15,00"]), None);
        assert_eq!(currency(&["DANKE FÜR IHREN EINKAUF BEI USA SHOP"]), None);
        assert_eq!(currency(&[]), None);
    }
}
