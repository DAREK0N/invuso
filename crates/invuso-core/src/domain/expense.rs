use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

use super::{GroupId, Money, PaymentMethodId, PersonId, is_iso_date};
use crate::split::{SplitError, SplitMode, split};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExpenseError {
    #[error("the amount must be greater than zero")]
    NonPositiveTotal,
    #[error("nobody paid")]
    NoPayer,
    #[error("every payment must be greater than zero")]
    NonPositivePayment,
    #[error("a person is listed as payer twice")]
    DuplicatePayer,
    #[error("payments add up to {actual}, not {expected}")]
    PaymentsMismatch { expected: i64, actual: i64 },
    #[error("nobody shares the expense")]
    NoParticipants,
    #[error("a share must not be negative")]
    NegativeShare,
    #[error(transparent)]
    Split(#[from] SplitError),
    #[error("`{0}` is not a local date and time with UTC offset")]
    InvalidOccurredAt(String),
    #[error("unknown expense source `{0}`")]
    UnknownSource(String),
}

/// Identifier of an `Expense` (idee.md 4.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExpenseId(pub String);

impl ExpenseId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ExpenseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identifier of a `Category` (idee.md 4.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CategoryId(pub String);

impl CategoryId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Expense category such as food or transport (idee.md 4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: CategoryId,
    /// For default categories a stable key (e.g. `"food"`) the UI
    /// translates; for the user's own categories the name itself.
    pub name: String,
    /// Icon key, e.g. `"utensils"`.
    pub icon: String,
    /// Design-token name, e.g. `"cerulean"`.
    pub color: String,
    pub is_default: bool,
}

/// How an expense was entered (idee.md 4.1 `Expense.source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseSource {
    Manual,
    Scan,
    Import,
}

impl ExpenseSource {
    pub const ALL: [Self; 3] = [Self::Manual, Self::Scan, Self::Import];

    /// Stable code stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Scan => "scan",
            Self::Import => "import",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, ExpenseError> {
        Self::ALL
            .into_iter()
            .find(|source| source.code() == code)
            .ok_or_else(|| ExpenseError::UnknownSource(code.to_string()))
    }
}

/// Who paid how much of an expense, and with what (idee.md 4.1
/// `ExpensePayment`). The amount is in the expense's currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpensePayment {
    pub person_id: PersonId,
    pub payment_method_id: Option<PaymentMethodId>,
    pub amount: Money,
}

/// A recorded expense (idee.md 4.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Expense {
    pub id: ExpenseId,
    /// `None` for a personal expense (EXP-06).
    pub group_id: Option<GroupId>,
    pub title: String,
    pub category_id: Option<CategoryId>,
    /// Local date and time with UTC offset, e.g. `2026-10-04T19:30:00+09:00`.
    pub occurred_at: String,
    pub total: Money,
    /// Archived rate `total` was converted with; `None` when the expense is
    /// already in the base currency.
    pub fx_rate_id: Option<String>,
    pub total_in_base: Money,
    pub split: SplitMode,
    pub source: ExpenseSource,
    pub payments: Vec<ExpensePayment>,
}

/// Checks who paid (EXP-02): at least one payer, each person once, every
/// part positive and all parts adding up to exactly `total_minor`.
pub fn validate_payments(
    total_minor: i64,
    payments: &[(PersonId, i64)],
) -> Result<(), ExpenseError> {
    if total_minor <= 0 {
        return Err(ExpenseError::NonPositiveTotal);
    }
    if payments.is_empty() {
        return Err(ExpenseError::NoPayer);
    }
    if payments.iter().any(|(_, amount)| *amount <= 0) {
        return Err(ExpenseError::NonPositivePayment);
    }
    let distinct: BTreeSet<&PersonId> = payments.iter().map(|(person, _)| person).collect();
    if distinct.len() != payments.len() {
        return Err(ExpenseError::DuplicatePayer);
    }
    let actual = payments
        .iter()
        .try_fold(0_i64, |acc, (_, amount)| acc.checked_add(*amount))
        .ok_or(ExpenseError::PaymentsMismatch {
            expected: total_minor,
            actual: i64::MAX,
        })?;
    if actual != total_minor {
        return Err(ExpenseError::PaymentsMismatch {
            expected: total_minor,
            actual,
        });
    }
    Ok(())
}

/// Checks the people an expense is split between (EXP-04).
pub fn validate_participants(participants: &BTreeSet<PersonId>) -> Result<(), ExpenseError> {
    if participants.is_empty() {
        Err(ExpenseError::NoParticipants)
    } else {
        Ok(())
    }
}

/// Checks how an expense is shared (EXP-04, SPL-01) and returns each
/// person's share of `total_minor`: someone must take part, percentages
/// must add up to 100 and exact amounts to the total (idee.md 8.1). Exact
/// amounts must not be negative; credits belong to line items (8.2).
pub fn validate_split(
    total_minor: i64,
    mode: &SplitMode,
) -> Result<BTreeMap<PersonId, i64>, ExpenseError> {
    validate_participants(&mode.participants())?;
    if let SplitMode::Exact(amounts) = mode
        && amounts.values().any(|amount| *amount < 0)
    {
        return Err(ExpenseError::NegativeShare);
    }
    Ok(split(total_minor, mode)?)
}

/// Checks `occurred_at`: `YYYY-MM-DDTHH:MM:SS` followed by `Z` or `±HH:MM`,
/// with a real calendar date and time.
pub fn validate_occurred_at(occurred_at: &str) -> Result<(), ExpenseError> {
    let invalid = || ExpenseError::InvalidOccurredAt(occurred_at.to_string());
    let (date, rest) = occurred_at.split_once('T').ok_or_else(invalid)?;
    if !is_iso_date(date) || rest.len() < 9 {
        return Err(invalid());
    }
    let (time, offset) = rest.split_at(8);
    let time_ok = time.as_bytes()[2] == b':'
        && time.as_bytes()[5] == b':'
        && two_digits(&time[0..2]).is_some_and(|h| h < 24)
        && two_digits(&time[3..5]).is_some_and(|m| m < 60)
        && two_digits(&time[6..8]).is_some_and(|s| s < 60);
    let offset_ok = offset == "Z"
        || (offset.len() == 6
            && matches!(offset.as_bytes()[0], b'+' | b'-')
            && offset.as_bytes()[3] == b':'
            && two_digits(&offset[1..3]).is_some_and(|h| h <= 23)
            && two_digits(&offset[4..6]).is_some_and(|m| m < 60));
    if time_ok && offset_ok {
        Ok(())
    } else {
        Err(invalid())
    }
}

/// The local calendar date of a valid `occurred_at` (its first ten
/// characters): the day whose exchange rate applies (idee.md 8.4).
pub fn local_date(occurred_at: &str) -> &str {
    occurred_at.get(0..10).unwrap_or(occurred_at)
}

fn two_digits(text: &str) -> Option<u8> {
    let bytes = text.as_bytes();
    (bytes.len() == 2 && bytes.iter().all(u8::is_ascii_digit))
        .then(|| (bytes[0] - b'0') * 10 + (bytes[1] - b'0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str) -> PersonId {
        PersonId::new(id)
    }

    #[test]
    fn payments_must_add_up_exactly() {
        assert_eq!(
            validate_payments(4000, &[(p("a"), 2000), (p("b"), 2000)]),
            Ok(())
        );
        assert_eq!(
            validate_payments(4000, &[(p("a"), 2000), (p("b"), 1999)]),
            Err(ExpenseError::PaymentsMismatch {
                expected: 4000,
                actual: 3999
            })
        );
        assert_eq!(validate_payments(3000, &[(p("a"), 3000)]), Ok(()));
    }

    #[test]
    fn rejects_empty_zero_negative_and_duplicate_payments() {
        assert_eq!(
            validate_payments(0, &[(p("a"), 0)]),
            Err(ExpenseError::NonPositiveTotal)
        );
        assert_eq!(
            validate_payments(-5, &[(p("a"), -5)]),
            Err(ExpenseError::NonPositiveTotal)
        );
        assert_eq!(validate_payments(10, &[]), Err(ExpenseError::NoPayer));
        assert_eq!(
            validate_payments(10, &[(p("a"), 10), (p("b"), 0)]),
            Err(ExpenseError::NonPositivePayment)
        );
        assert_eq!(
            validate_payments(10, &[(p("a"), 15), (p("b"), -5)]),
            Err(ExpenseError::NonPositivePayment)
        );
        assert_eq!(
            validate_payments(10, &[(p("a"), 5), (p("a"), 5)]),
            Err(ExpenseError::DuplicatePayer)
        );
    }

    #[test]
    fn payment_overflow_is_a_mismatch_not_a_panic() {
        assert!(matches!(
            validate_payments(i64::MAX, &[(p("a"), i64::MAX), (p("b"), 1)]),
            Err(ExpenseError::PaymentsMismatch { .. })
        ));
    }

    #[test]
    fn needs_a_participant() {
        assert_eq!(
            validate_participants(&BTreeSet::new()),
            Err(ExpenseError::NoParticipants)
        );
        assert_eq!(validate_participants(&BTreeSet::from([p("a")])), Ok(()));
    }

    #[test]
    fn split_is_checked_with_its_shares() {
        use rust_decimal::Decimal;

        let both = BTreeSet::from([p("a"), p("b")]);
        let shares = validate_split(1001, &SplitMode::Equal(both)).unwrap();
        assert_eq!(shares.values().sum::<i64>(), 1001);

        assert_eq!(
            validate_split(100, &SplitMode::Equal(BTreeSet::new())),
            Err(ExpenseError::NoParticipants)
        );
        assert_eq!(
            validate_split(100, &SplitMode::Weights(BTreeMap::new())),
            Err(ExpenseError::NoParticipants)
        );
        assert_eq!(
            validate_split(
                100,
                &SplitMode::Percent(BTreeMap::from([(p("a"), Decimal::from(99))]))
            ),
            Err(ExpenseError::Split(SplitError::PercentNot100(
                Decimal::from(99)
            )))
        );
        assert_eq!(
            validate_split(
                100,
                &SplitMode::Weights(BTreeMap::from([(p("a"), Decimal::ZERO)]))
            ),
            Err(ExpenseError::Split(SplitError::ZeroTotalWeight))
        );
        // Sums match, but a negative part would hide a credit.
        assert_eq!(
            validate_split(
                100,
                &SplitMode::Exact(BTreeMap::from([(p("a"), 150), (p("b"), -50)]))
            ),
            Err(ExpenseError::NegativeShare)
        );
        assert_eq!(
            validate_split(
                100,
                &SplitMode::Exact(BTreeMap::from([(p("a"), 100), (p("b"), 0)]))
            )
            .unwrap()[&p("b")],
            0
        );
    }

    #[test]
    fn occurred_at_needs_date_time_and_offset() {
        for valid in [
            "2026-10-04T19:30:00+09:00",
            "2026-10-04T00:00:00-03:30",
            "2024-02-29T23:59:59Z",
        ] {
            assert_eq!(validate_occurred_at(valid), Ok(()), "{valid}");
        }
        for invalid in [
            "2026-10-04",
            "2026-10-04T19:30:00",
            "2026-10-04T19:30+09:00",
            "2026-10-04T24:00:00+09:00",
            "2026-10-04T19:60:00+09:00",
            "2025-02-29T12:00:00Z",
            "2026-10-04T19:30:00+0900",
            "2026-10-04 19:30:00+09:00",
            "",
        ] {
            assert!(validate_occurred_at(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn local_date_is_the_date_part() {
        assert_eq!(local_date("2026-10-04T23:30:00+09:00"), "2026-10-04");
    }

    #[test]
    fn source_codes_round_trip() {
        for source in ExpenseSource::ALL {
            assert_eq!(ExpenseSource::from_code(source.code()), Ok(source));
        }
        assert!(ExpenseSource::from_code("fax").is_err());
    }
}
