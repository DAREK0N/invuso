use std::fmt;

use thiserror::Error;

use super::{ExpenseError, GroupId, Money, PaymentMethodId, PersonId, validate_occurred_at};
use crate::split::SettlementEntry;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SettlementError {
    #[error("a settlement needs two different people")]
    SamePerson,
    #[error("the amount of a settlement must be positive")]
    NonPositiveAmount,
    #[error("invalid time: {0}")]
    OccurredAt(#[from] ExpenseError),
}

/// Identifier of a `Settlement` (idee.md 4.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SettlementId(pub String);

impl SettlementId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SettlementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A payment `from` → `to` that settles debts within a group (idee.md 4.1,
/// SPL-06). Partial payments are just smaller amounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    pub id: SettlementId,
    pub group_id: GroupId,
    pub from: PersonId,
    pub to: PersonId,
    pub amount: Money,
    pub payment_method_id: Option<PaymentMethodId>,
    /// Local time with offset, like `Expense::occurred_at`.
    pub occurred_at: String,
    pub note: Option<String>,
}

impl Settlement {
    /// The settlement as the balances count it, in its own currency.
    pub fn entry(&self) -> SettlementEntry {
        SettlementEntry {
            from: self.from.clone(),
            to: self.to.clone(),
            amount_minor: self.amount.amount_minor(),
        }
    }
}

/// Checks the parts every settlement needs: two different people, a
/// positive amount and a valid time.
pub fn validate_settlement(
    from: &PersonId,
    to: &PersonId,
    amount: Money,
    occurred_at: &str,
) -> Result<(), SettlementError> {
    if from == to {
        return Err(SettlementError::SamePerson);
    }
    if amount.amount_minor() <= 0 {
        return Err(SettlementError::NonPositiveAmount);
    }
    validate_occurred_at(occurred_at)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Currency;

    fn eur(minor: i64) -> Money {
        Money::new(minor, Currency::from_code("EUR").unwrap())
    }

    const AT: &str = "2026-10-06T18:00:00+02:00";

    #[test]
    fn accepts_a_payment_between_two_people() {
        assert_eq!(
            validate_settlement(&PersonId::from("ben"), &PersonId::from("anna"), eur(1), AT),
            Ok(())
        );
    }

    #[test]
    fn rejects_same_person_zero_negative_and_bad_time() {
        let (anna, ben) = (PersonId::from("anna"), PersonId::from("ben"));
        assert_eq!(
            validate_settlement(&anna, &anna, eur(100), AT),
            Err(SettlementError::SamePerson)
        );
        assert_eq!(
            validate_settlement(&ben, &anna, eur(0), AT),
            Err(SettlementError::NonPositiveAmount)
        );
        assert_eq!(
            validate_settlement(&ben, &anna, eur(-100), AT),
            Err(SettlementError::NonPositiveAmount)
        );
        assert!(matches!(
            validate_settlement(&ben, &anna, eur(100), "gestern"),
            Err(SettlementError::OccurredAt(_))
        ));
    }
}
