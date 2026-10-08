use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use thiserror::Error;

use super::{GroupId, LineItem, LineItemError, Money, PaymentMethodId, PersonId, is_iso_date};
use crate::split::{ExpenseEntry, SplitError, SplitMode, rescale, split};

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
    #[error(transparent)]
    LineItem(#[from] LineItemError),
    #[error("coordinates out of range")]
    InvalidCoordinates,
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

/// Where an expense happened, from the device's location (EXP-10). Kept
/// next to the free-text `location`, because turning coordinates into a
/// place name would need an online service (AGENTS.md 7.6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    latitude: f64,
    longitude: f64,
}

impl GeoPoint {
    /// Degrees, latitude in −90..=90 and longitude in −180..=180.
    pub fn new(latitude: f64, longitude: f64) -> Result<Self, ExpenseError> {
        if (-90.0..=90.0).contains(&latitude) && (-180.0..=180.0).contains(&longitude) {
            Ok(Self {
                latitude,
                longitude,
            })
        } else {
            Err(ExpenseError::InvalidCoordinates)
        }
    }

    pub fn latitude(self) -> f64 {
        self.latitude
    }

    pub fn longitude(self) -> f64 {
        self.longitude
    }
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
    /// Archived receipt the expense was recorded from or with (RCP-03).
    pub receipt_id: Option<String>,
    /// Free text (EXP-10).
    pub note: Option<String>,
    /// Place as the user wrote it, e.g. "Shinjuku" (EXP-10).
    pub location: Option<String>,
    /// Device location, only when the user asked for it (EXP-10).
    pub coordinates: Option<GeoPoint>,
    pub payments: Vec<ExpensePayment>,
    /// Positions in receipt order (idee.md 4.1 `LineItem`); kept with any
    /// split mode, `SplitMode::Items` splits by them.
    pub line_items: Vec<LineItem>,
}

impl Expense {
    /// Each person's share in the expense's own currency (idee.md 8.1).
    pub fn shares(&self) -> Result<BTreeMap<PersonId, i64>, ExpenseError> {
        validate_split(self.total.amount_minor(), &self.split)
    }

    /// Each person's share in the base currency (idee.md 8.2 step 5): the
    /// own-currency shares rescaled to `total_in_base`, so they add up to
    /// it exactly instead of drifting by rounding each one on its own.
    pub fn shares_in_base(&self) -> Result<BTreeMap<PersonId, i64>, ExpenseError> {
        Ok(rescale(self.total_in_base.amount_minor(), &self.shares()?)?)
    }

    /// What each payer paid, in the base currency, rescaled like
    /// [`Expense::shares_in_base`] so it adds up to `total_in_base`.
    pub fn payments_in_base(&self) -> Result<BTreeMap<PersonId, i64>, ExpenseError> {
        let paid: BTreeMap<PersonId, i64> = self
            .payments
            .iter()
            .map(|p| (p.person_id.clone(), p.amount.amount_minor()))
            .collect();
        Ok(rescale(self.total_in_base.amount_minor(), &paid)?)
    }

    /// Payments and shares in the base currency, as balances need them
    /// (idee.md 8.3).
    pub fn entry(&self) -> Result<ExpenseEntry, ExpenseError> {
        Ok(ExpenseEntry {
            payments: self.payments_in_base()?,
            shares: self.shares_in_base()?,
        })
    }
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

/// Who would carry how much if the expense were saved as it stands
/// (OCR-39): the same shares a saved [`Expense`] gives.
#[derive(Debug, Clone, PartialEq)]
pub struct SharePreview {
    /// Each person's share in the expense's currency, as
    /// [`Expense::shares`] gives it.
    pub shares: BTreeMap<PersonId, i64>,
    /// What the lines nobody in particular had add up to (the
    /// "Allgemeinheit", idee.md 8.2 step 3), before they are shared; 0 when
    /// the expense is not split by line items.
    pub general: i64,
}

impl SharePreview {
    /// The shares in the base currency once the total is converted to
    /// `total_in_base`, as [`Expense::shares_in_base`] gives them.
    pub fn in_base(&self, total_in_base: i64) -> Result<BTreeMap<PersonId, i64>, ExpenseError> {
        Ok(rescale(total_in_base, &self.shares)?)
    }
}

/// The shares of an expense of `total_minor` split by `mode`, while it is
/// still being entered (OCR-39).
pub fn preview_shares(total_minor: i64, mode: &SplitMode) -> Result<SharePreview, ExpenseError> {
    let shares = validate_split(total_minor, mode)?;
    let general = match mode {
        SplitMode::Items { items, .. } => items
            .iter()
            .filter(|item| item.assigned_to.is_empty())
            .try_fold(0_i64, |sum, item| sum.checked_add(item.amount_minor))
            .ok_or(SplitError::Overflow)?,
        _ => 0,
    };
    Ok(SharePreview { shares, general })
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

    fn expense(total: Money, total_in_base: Money, split: SplitMode) -> Expense {
        Expense {
            id: ExpenseId::new("e"),
            group_id: None,
            title: "Ramen".to_string(),
            category_id: None,
            occurred_at: "2026-10-04T19:30:00+09:00".to_string(),
            total,
            fx_rate_id: None,
            total_in_base,
            split,
            source: ExpenseSource::Manual,
            receipt_id: None,
            note: None,
            location: None,
            coordinates: None,
            payments: Vec::new(),
            line_items: Vec::new(),
        }
    }

    fn cur(code: &str) -> super::super::Currency {
        super::super::Currency::from_code(code).unwrap()
    }

    #[test]
    fn coordinates_must_be_on_earth() {
        let tokyo = GeoPoint::new(35.6895, 139.6917).unwrap();
        assert_eq!((tokyo.latitude(), tokyo.longitude()), (35.6895, 139.6917));
        assert!(GeoPoint::new(-90.0, 180.0).is_ok());
        assert_eq!(
            GeoPoint::new(90.5, 0.0),
            Err(ExpenseError::InvalidCoordinates)
        );
        assert_eq!(
            GeoPoint::new(0.0, -180.1),
            Err(ExpenseError::InvalidCoordinates)
        );
        assert_eq!(
            GeoPoint::new(f64::NAN, 0.0),
            Err(ExpenseError::InvalidCoordinates)
        );
    }

    #[test]
    fn base_shares_add_up_to_the_converted_total() {
        // 1000 JPY = 5.63 EUR; converting each share on its own would give
        // 1.88 + 1.87 + 1.87 = 5.62 EUR.
        let e = expense(
            Money::new(1000, cur("JPY")),
            Money::new(563, cur("EUR")),
            SplitMode::Equal([p("a"), p("b"), p("c")].into()),
        );
        assert_eq!(
            e.shares(),
            Ok([(p("a"), 334), (p("b"), 333), (p("c"), 333)].into())
        );
        let base = e.shares_in_base().unwrap();
        assert_eq!(base.values().sum::<i64>(), 563);
        // Rests: a 0.042, b and c 0.479; the leftover cent goes to b by id.
        assert_eq!(base, [(p("a"), 188), (p("b"), 188), (p("c"), 187)].into());
    }

    #[test]
    fn preview_matches_the_saved_shares() {
        use crate::split::ItemLine;
        let one = rust_decimal::Decimal::ONE;
        let line = |amount: i64, ids: &[&str]| ItemLine {
            amount_minor: amount,
            assigned_to: ids.iter().map(|id| (p(id), one)).collect(),
        };
        // 1 487 ¥: beer for a, two thirds of a melon for b and c, rice for
        // everyone, a coupon on b's melon, 108 ¥ tax on top.
        let mode = SplitMode::Items {
            participants: [(p("a"), one), (p("b"), one), (p("c"), one)].into(),
            items: vec![
                line(480, &["a"]),
                line(500, &["b", "c"]),
                line(-50, &["b", "c"]),
                line(449, &[]),
            ],
        };
        let preview = preview_shares(1_487, &mode).unwrap();
        assert_eq!(preview.general, 449);
        // 1 JPY = 0.00563 EUR → 8.37 EUR.
        let e = expense(
            Money::new(1_487, cur("JPY")),
            Money::new(837, cur("EUR")),
            mode,
        );
        assert_eq!(e.shares(), Ok(preview.shares.clone()));
        assert_eq!(e.shares_in_base(), preview.in_base(837));
        assert_eq!(preview.shares.values().sum::<i64>(), 1_487);
        assert_eq!(preview.in_base(837).unwrap().values().sum::<i64>(), 837);
    }

    #[test]
    fn preview_of_other_modes_has_no_general_part() {
        let mode = SplitMode::Equal([p("a"), p("b")].into());
        let preview = preview_shares(1_001, &mode).unwrap();
        assert_eq!(preview.general, 0);
        assert_eq!(preview.shares, [(p("a"), 501), (p("b"), 500)].into());
        // Three decimals in the base currency (BHD).
        assert_eq!(
            preview.in_base(4_701),
            Ok([(p("a"), 2_353), (p("b"), 2_348)].into())
        );
        assert_eq!(
            preview_shares(1_001, &SplitMode::Equal(BTreeSet::new())),
            Err(ExpenseError::NoParticipants)
        );
    }

    #[test]
    fn base_payments_add_up_to_the_converted_total() {
        let mut e = expense(
            Money::new(3000, cur("JPY")),
            Money::new(1683, cur("EUR")),
            SplitMode::Equal([p("a")].into()),
        );
        e.payments = ["a", "b"]
            .map(|id| ExpensePayment {
                person_id: p(id),
                payment_method_id: None,
                amount: Money::new(1500, cur("JPY")),
            })
            .into();
        // 841.5 each: the odd cent goes to the first id.
        assert_eq!(
            e.payments_in_base(),
            Ok([(p("a"), 842), (p("b"), 841)].into())
        );
        e.payments.clear();
        assert!(e.payments_in_base().is_err());
    }

    #[test]
    fn base_shares_equal_shares_in_the_same_currency() {
        let e = expense(
            Money::new(4000, cur("EUR")),
            Money::new(4000, cur("EUR")),
            SplitMode::Exact([(p("a"), 2999), (p("b"), 1001)].into()),
        );
        assert_eq!(e.shares_in_base(), e.shares());
    }

    #[test]
    fn base_shares_keep_zero_weights_and_three_decimals() {
        // 10 EUR = 4.700 BHD (three decimals); b carries nothing.
        let e = expense(
            Money::new(1000, cur("EUR")),
            Money::new(4700, cur("BHD")),
            SplitMode::Weights(
                [
                    (p("a"), rust_decimal::Decimal::ONE),
                    (p("b"), rust_decimal::Decimal::ZERO),
                ]
                .into(),
            ),
        );
        assert_eq!(e.shares_in_base(), Ok([(p("a"), 4700), (p("b"), 0)].into()));
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
