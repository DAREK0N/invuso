use std::fmt;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy};
use thiserror::Error;

use super::Currency;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MoneyError {
    #[error("cannot combine {left} with {right}")]
    CurrencyMismatch { left: Currency, right: Currency },
    #[error("amount out of range")]
    Overflow,
}

/// An exact amount of money (CORE-05): integer count of the currency's
/// smallest unit, e.g. `1234 EUR` = 12.34 €, `1234 JPY` = 1,234 ¥.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Money {
    amount_minor: i64,
    currency: Currency,
}

impl Money {
    pub fn new(amount_minor: i64, currency: Currency) -> Self {
        Self {
            amount_minor,
            currency,
        }
    }

    pub fn zero(currency: Currency) -> Self {
        Self::new(0, currency)
    }

    /// Rounds a decimal amount in major units (e.g. `12.345` EUR) to the
    /// currency's minor unit, half to even (idee.md 8.4).
    pub fn from_decimal(amount: Decimal, currency: Currency) -> Result<Self, MoneyError> {
        let minor = amount
            .checked_mul(Decimal::from(10_i64.pow(currency.exponent())))
            .ok_or(MoneyError::Overflow)?
            .round_dp_with_strategy(0, RoundingStrategy::MidpointNearestEven)
            .to_i64()
            .ok_or(MoneyError::Overflow)?;
        Ok(Self::new(minor, currency))
    }

    pub fn amount_minor(&self) -> i64 {
        self.amount_minor
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    /// The amount in major units, e.g. `12.34` for `1234 EUR`. Exact.
    pub fn to_decimal(&self) -> Decimal {
        Decimal::new(self.amount_minor, self.currency.exponent())
    }

    pub fn is_zero(&self) -> bool {
        self.amount_minor == 0
    }

    pub fn is_negative(&self) -> bool {
        self.amount_minor < 0
    }

    pub fn checked_add(self, other: Money) -> Result<Money, MoneyError> {
        self.ensure_same_currency(other)?;
        self.amount_minor
            .checked_add(other.amount_minor)
            .map(|amount| Money::new(amount, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    pub fn checked_sub(self, other: Money) -> Result<Money, MoneyError> {
        self.ensure_same_currency(other)?;
        self.amount_minor
            .checked_sub(other.amount_minor)
            .map(|amount| Money::new(amount, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    pub fn checked_neg(self) -> Result<Money, MoneyError> {
        self.amount_minor
            .checked_neg()
            .map(|amount| Money::new(amount, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    fn ensure_same_currency(&self, other: Money) -> Result<(), MoneyError> {
        if self.currency == other.currency {
            Ok(())
        } else {
            Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            })
        }
    }
}

impl fmt::Debug for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Money({} {})", self.to_decimal(), self.currency)
    }
}

/// Plain, locale-independent form (`12.34 EUR`); localized formatting is a UI concern.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.to_decimal(), self.currency)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    #[test]
    fn decimal_round_trip_respects_exponent() {
        assert_eq!(
            Money::new(1234, cur("EUR")).to_decimal(),
            Decimal::from_str("12.34").unwrap()
        );
        assert_eq!(
            Money::new(1234, cur("JPY")).to_decimal(),
            Decimal::from(1234)
        );
        assert_eq!(
            Money::new(1234, cur("KWD")).to_decimal(),
            Decimal::from_str("1.234").unwrap()
        );
    }

    #[test]
    fn from_decimal_rounds_half_to_even() {
        let eur = cur("EUR");
        let d = |s: &str| Decimal::from_str(s).unwrap();
        assert_eq!(
            Money::from_decimal(d("0.125"), eur).unwrap().amount_minor(),
            12
        );
        assert_eq!(
            Money::from_decimal(d("0.135"), eur).unwrap().amount_minor(),
            14
        );
        assert_eq!(
            Money::from_decimal(d("-0.125"), eur)
                .unwrap()
                .amount_minor(),
            -12
        );
        assert_eq!(
            Money::from_decimal(d("2.5"), cur("JPY"))
                .unwrap()
                .amount_minor(),
            2
        );
    }

    #[test]
    fn arithmetic_rejects_mixed_currencies() {
        let result = Money::new(100, cur("EUR")).checked_add(Money::new(100, cur("JPY")));
        assert!(matches!(result, Err(MoneyError::CurrencyMismatch { .. })));
    }

    #[test]
    fn arithmetic_detects_overflow() {
        let eur = cur("EUR");
        assert_eq!(
            Money::new(i64::MAX, eur).checked_add(Money::new(1, eur)),
            Err(MoneyError::Overflow)
        );
        assert_eq!(
            Money::new(i64::MIN, eur).checked_neg(),
            Err(MoneyError::Overflow)
        );
    }

    #[test]
    fn display_is_plain() {
        assert_eq!(Money::new(-1205, cur("EUR")).to_string(), "-12.05 EUR");
    }
}
