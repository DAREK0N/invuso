use std::fmt;

use rust_decimal::Decimal;
use thiserror::Error;

use super::{Currency, Person};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GroupError {
    /// Dates are stored as local calendar dates `YYYY-MM-DD` (idee.md 4.1).
    #[error("`{0}` is not a date of the form YYYY-MM-DD")]
    InvalidDate(String),
    #[error("the period ends before it starts")]
    EndBeforeStart,
}

/// Identifier of a `Group` (idee.md 4.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(pub String);

impl GroupId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Folder for expenses that belong together, e.g. "Japan Reise"
/// (idee.md 4.1, GRP-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    /// Icon key, e.g. `"plane"`.
    pub icon: String,
    /// Design-token name, e.g. `"cerulean"`.
    pub color: String,
    /// Currency all balances of the group are computed in.
    pub base_currency: Currency,
    /// Local calendar dates `YYYY-MM-DD`; both optional.
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    /// ISO 639-1 code receipts of this group are translated into; `None`
    /// follows the global setting (TRL-05).
    pub target_language: Option<String>,
}

/// A person in a group (idee.md 4.1 `GroupMember`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMember {
    pub person: Person,
    /// Default share in new expenses, e.g. 0.5 for a child (PER-04).
    pub default_weight: Decimal,
}

/// Checks an optional period: blank means "not set", otherwise each date
/// must be a real `YYYY-MM-DD` date and the end must not lie before the
/// start. Returns the trimmed dates.
pub fn validate_period(
    start: Option<&str>,
    end: Option<&str>,
) -> Result<(Option<String>, Option<String>), GroupError> {
    let start = optional_date(start)?;
    let end = optional_date(end)?;
    // ISO dates of equal length order like the dates they stand for.
    if let (Some(start), Some(end)) = (&start, &end)
        && end < start
    {
        return Err(GroupError::EndBeforeStart);
    }
    Ok((start, end))
}

fn optional_date(date: Option<&str>) -> Result<Option<String>, GroupError> {
    let Some(date) = date.map(str::trim).filter(|d| !d.is_empty()) else {
        return Ok(None);
    };
    if is_iso_date(date) {
        Ok(Some(date.to_string()))
    } else {
        Err(GroupError::InvalidDate(date.to_string()))
    }
}

/// `YYYY-MM-DD` with a day that exists in that month (Gregorian calendar).
pub fn is_iso_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    let digits_at = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !digits_at(0..4)
        || !digits_at(5..7)
        || !digits_at(8..10)
    {
        return false;
    }
    let number = |range: std::ops::Range<usize>| {
        bytes[range]
            .iter()
            .fold(0_u32, |acc, b| acc * 10 + u32::from(b - b'0'))
    };
    let (year, month, day) = (number(0..4), number(5..7), number(8..10));
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year > 0 && (1..=days_in_month).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_dates_mean_no_period() {
        assert_eq!(validate_period(None, None), Ok((None, None)));
        assert_eq!(validate_period(Some(""), Some("  ")), Ok((None, None)));
    }

    #[test]
    fn accepts_open_and_closed_periods() {
        assert_eq!(
            validate_period(Some(" 2026-03-01 "), None),
            Ok((Some("2026-03-01".into()), None))
        );
        assert_eq!(
            validate_period(None, Some("2026-03-14")),
            Ok((None, Some("2026-03-14".into())))
        );
        assert_eq!(
            validate_period(Some("2026-03-01"), Some("2026-03-01")),
            Ok((Some("2026-03-01".into()), Some("2026-03-01".into())))
        );
    }

    #[test]
    fn rejects_end_before_start() {
        assert_eq!(
            validate_period(Some("2026-03-14"), Some("2026-03-01")),
            Err(GroupError::EndBeforeStart)
        );
        assert_eq!(
            validate_period(Some("2026-01-01"), Some("2025-12-31")),
            Err(GroupError::EndBeforeStart)
        );
    }

    #[test]
    fn rejects_malformed_and_impossible_dates() {
        for bad in [
            "2026-3-1",
            "01.03.2026",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "2026-02-29",
            "2100-02-29",
            "0000-01-01",
            "2026-01-0a",
            "２０２６-01-01",
        ] {
            assert_eq!(
                validate_period(Some(bad), None),
                Err(GroupError::InvalidDate(bad.into())),
                "{bad}"
            );
        }
    }

    #[test]
    fn knows_leap_years() {
        for good in ["2024-02-29", "2000-02-29", "2026-12-31", "2026-01-31"] {
            assert!(validate_period(Some(good), None).is_ok(), "{good}");
        }
    }
}
