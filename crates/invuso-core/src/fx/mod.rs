//! Currency conversion with archived rates (idee.md 8.4). Fetching rates is
//! I/O and lives in the app; this module only does the arithmetic.

use rust_decimal::Decimal;
use thiserror::Error;

use crate::domain::{Currency, Money, MoneyError};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FxError {
    #[error("exchange rate must be positive")]
    NonPositiveRate,
    #[error("a rate from a currency to itself must be 1")]
    SelfRateNotOne,
    #[error("rate {base}→{quote} cannot convert {amount}")]
    WrongCurrency {
        base: Currency,
        quote: Currency,
        amount: Currency,
    },
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// `1 base = value quote`, e.g. `1 EUR = 162.35 JPY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    base: Currency,
    quote: Currency,
    value: Decimal,
}

impl Rate {
    pub fn new(base: Currency, quote: Currency, value: Decimal) -> Result<Self, FxError> {
        if value <= Decimal::ZERO {
            return Err(FxError::NonPositiveRate);
        }
        if base == quote && value != Decimal::ONE {
            return Err(FxError::SelfRateNotOne);
        }
        Ok(Self { base, quote, value })
    }

    pub fn base(&self) -> Currency {
        self.base
    }

    pub fn quote(&self) -> Currency {
        self.quote
    }

    pub fn value(&self) -> Decimal {
        self.value
    }

    /// The opposite direction, e.g. `1 JPY = 0.00616 EUR` (28 significant digits).
    pub fn inverse(&self) -> Self {
        Self {
            base: self.quote,
            quote: self.base,
            value: Decimal::ONE / self.value,
        }
    }
}

/// Converts `amount` (in the rate's base currency) into the quote currency,
/// rounding half to even to the quote's minor unit.
pub fn convert(amount: Money, rate: &Rate) -> Result<Money, FxError> {
    if amount.currency() != rate.base {
        return Err(FxError::WrongCurrency {
            base: rate.base,
            quote: rate.quote,
            amount: amount.currency(),
        });
    }
    let converted = amount
        .to_decimal()
        .checked_mul(rate.value)
        .ok_or(MoneyError::Overflow)?;
    Ok(Money::from_decimal(converted, rate.quote)?)
}

/// Picks the rate for `date` from `(date, rate)` entries: the one of that
/// day, otherwise the closest earlier one (idee.md 8.4). `None` if every
/// known rate is newer than `date`.
pub fn rate_for_date<'a, D: Ord, R>(rates: &'a [(D, R)], date: &D) -> Option<&'a R> {
    rates
        .iter()
        .filter(|(rate_date, _)| rate_date <= date)
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, rate)| rate)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn d(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    #[test]
    fn eur_to_jpy_rounds_to_whole_yen() {
        let rate = Rate::new(cur("EUR"), cur("JPY"), d("162.35")).unwrap();
        let yen = convert(Money::new(1250, cur("EUR")), &rate).unwrap();
        // 12.50 × 162.35 = 2029.375 → 2029
        assert_eq!(yen, Money::new(2029, cur("JPY")));
    }

    #[test]
    fn jpy_to_eur_rounds_half_to_even() {
        // 1 JPY = 0.005 EUR: 1 ¥ = 0.005 € → 0.00 (even), 3 ¥ = 0.015 € → 0.02.
        let rate = Rate::new(cur("JPY"), cur("EUR"), d("0.005")).unwrap();
        assert_eq!(
            convert(Money::new(1, cur("JPY")), &rate)
                .unwrap()
                .amount_minor(),
            0
        );
        assert_eq!(
            convert(Money::new(3, cur("JPY")), &rate)
                .unwrap()
                .amount_minor(),
            2
        );
    }

    #[test]
    fn three_decimal_currency() {
        let rate = Rate::new(cur("EUR"), cur("KWD"), d("0.3312")).unwrap();
        let kwd = convert(Money::new(10_000, cur("EUR")), &rate).unwrap();
        assert_eq!(kwd, Money::new(33_120, cur("KWD")));
    }

    #[test]
    fn negative_amounts_convert_symmetrically() {
        let rate = Rate::new(cur("EUR"), cur("JPY"), d("162.35")).unwrap();
        let yen = convert(Money::new(-1250, cur("EUR")), &rate).unwrap();
        assert_eq!(yen.amount_minor(), -2029);
    }

    #[test]
    fn inverse_round_trips_closely() {
        let rate = Rate::new(cur("EUR"), cur("JPY"), d("162.35")).unwrap();
        let back = convert(Money::new(16_235, cur("JPY")), &rate.inverse()).unwrap();
        assert_eq!(back, Money::new(10_000, cur("EUR")));
    }

    #[test]
    fn rejects_wrong_direction_and_bad_rates() {
        let rate = Rate::new(cur("EUR"), cur("JPY"), d("162.35")).unwrap();
        assert!(matches!(
            convert(Money::new(100, cur("JPY")), &rate),
            Err(FxError::WrongCurrency { .. })
        ));
        assert_eq!(
            Rate::new(cur("EUR"), cur("JPY"), Decimal::ZERO),
            Err(FxError::NonPositiveRate)
        );
        assert_eq!(
            Rate::new(cur("EUR"), cur("EUR"), d("1.1")),
            Err(FxError::SelfRateNotOne)
        );
    }

    #[test]
    fn picks_same_day_or_closest_earlier_rate() {
        // Dates as sortable ISO strings; weekends have no ECB rate.
        let rates = [("2026-10-01", 1), ("2026-10-02", 2), ("2026-10-05", 3)];
        assert_eq!(rate_for_date(&rates, &"2026-10-02"), Some(&2));
        assert_eq!(rate_for_date(&rates, &"2026-10-04"), Some(&2));
        assert_eq!(rate_for_date(&rates, &"2026-10-09"), Some(&3));
        assert_eq!(rate_for_date(&rates, &"2026-09-30"), None);
    }
}
