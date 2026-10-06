use std::collections::BTreeMap;
use std::fmt;

use thiserror::Error;

use super::{Currency, Money, MoneyError};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CashError {
    #[error("unknown cash movement kind `{0}`")]
    UnknownKind(String),
    #[error("the amount must be positive")]
    NonPositiveAmount,
    #[error("an exchange needs two different currencies")]
    SameCurrency,
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// Identifier of a `CashMovement` (idee.md 4.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CashMovementId(pub String);

impl CashMovementId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CashMovementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why cash came or went (idee.md 4.1 `CashMovement.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CashMovementKind {
    /// Taken from an ATM with a card (CASH-03).
    Withdrawal,
    /// Paid in cash for an expense (CASH-02).
    Expense,
    /// One side of a currency exchange (CASH-04).
    Exchange,
    Deposit,
    /// Difference found by counting the cash (CASH-05).
    Correction,
}

impl CashMovementKind {
    pub const ALL: [Self; 5] = [
        Self::Withdrawal,
        Self::Expense,
        Self::Exchange,
        Self::Deposit,
        Self::Correction,
    ];

    /// Stable code stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            Self::Withdrawal => "withdrawal",
            Self::Expense => "expense",
            Self::Exchange => "exchange",
            Self::Deposit => "deposit",
            Self::Correction => "correction",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, CashError> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.code() == code)
            .ok_or_else(|| CashError::UnknownKind(code.to_string()))
    }
}

/// Cash on hand per currency from signed movements (CASH-01). Currencies
/// whose movements add up to zero stay in the map, so an emptied wallet
/// still shows its currency.
pub fn cash_balances(
    movements: impl IntoIterator<Item = Money>,
) -> Result<BTreeMap<Currency, Money>, CashError> {
    let mut balances: BTreeMap<Currency, Money> = BTreeMap::new();
    for movement in movements {
        let balance = balances
            .entry(movement.currency())
            .or_insert_with(|| Money::zero(movement.currency()));
        *balance = balance.checked_add(movement)?;
    }
    Ok(balances)
}

/// The correction a cash count books (CASH-05): counted minus expected, or
/// `None` when the cash is as expected.
pub fn cash_correction(expected: Money, counted: Money) -> Result<Option<Money>, CashError> {
    if counted.is_negative() {
        return Err(CashError::NonPositiveAmount);
    }
    let difference = counted.checked_sub(expected)?;
    Ok((!difference.is_zero()).then_some(difference))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn yen(amount: i64) -> Money {
        Money::new(amount, cur("JPY"))
    }

    #[test]
    fn kinds_round_trip_through_their_codes() {
        for kind in CashMovementKind::ALL {
            assert_eq!(CashMovementKind::from_code(kind.code()), Ok(kind));
        }
        assert_eq!(
            CashMovementKind::from_code("gift"),
            Err(CashError::UnknownKind("gift".into()))
        );
    }

    #[test]
    fn withdrawal_payments_and_count_give_the_balance() {
        // The AP-22 check: 30 000 ¥ withdrawn, two cash payments, a count.
        let mut movements = vec![yen(30_000), yen(-1_280), yen(-4_500)];
        let expected = cash_balances(movements.clone()).unwrap()[&cur("JPY")];
        assert_eq!(expected, yen(24_220));
        let correction = cash_correction(expected, yen(24_000)).unwrap().unwrap();
        assert_eq!(correction, yen(-220));
        movements.push(correction);
        assert_eq!(cash_balances(movements).unwrap()[&cur("JPY")], yen(24_000));
    }

    #[test]
    fn balances_are_kept_per_currency_with_their_decimals() {
        let balances = cash_balances([
            yen(10_000),
            Money::new(4_500, cur("EUR")),
            Money::new(1_250, cur("KWD")),
            yen(-10_000),
            Money::new(-1, cur("KWD")),
        ])
        .unwrap();
        assert_eq!(
            balances.into_values().collect::<Vec<_>>(),
            [
                Money::new(4_500, cur("EUR")),
                yen(0),
                Money::new(1_249, cur("KWD"))
            ]
        );
        assert!(cash_balances([]).unwrap().is_empty());
    }

    #[test]
    fn correction_can_raise_lower_or_keep_the_balance() {
        assert_eq!(cash_correction(yen(500), yen(500)), Ok(None));
        assert_eq!(cash_correction(yen(-300), yen(0)), Ok(Some(yen(300))));
        assert_eq!(cash_correction(yen(0), yen(2_000)), Ok(Some(yen(2_000))));
        assert_eq!(
            cash_correction(yen(0), yen(-1)),
            Err(CashError::NonPositiveAmount)
        );
        assert!(matches!(
            cash_correction(Money::new(100, cur("EUR")), yen(100)),
            Err(CashError::Money(MoneyError::CurrencyMismatch { .. }))
        ));
    }

    #[test]
    fn overflow_is_an_error_not_a_panic() {
        assert!(matches!(
            cash_balances([yen(i64::MAX), yen(1)]),
            Err(CashError::Money(MoneyError::Overflow))
        ));
    }
}
