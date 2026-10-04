use std::fmt;

use iso_currency::IntoEnumIterator;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CurrencyError {
    #[error("unknown ISO 4217 currency code `{0}`")]
    UnknownCode(String),
    /// Precious metals and other units without minor units (e.g. `XAU`)
    /// cannot carry an integer money amount.
    #[error("currency `{0}` has no minor unit and cannot be used for money")]
    NoMinorUnit(String),
}

/// An ISO 4217 currency that can carry money, i.e. one with a defined number
/// of minor-unit digits (EUR = 2, JPY = 0, KWD = 3, CLF = 4).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Currency {
    iso: iso_currency::Currency,
    exponent: u32,
}

impl Currency {
    /// Parses an ISO 4217 alphabetic code; case-insensitive.
    pub fn from_code(code: &str) -> Result<Self, CurrencyError> {
        let normalized = code.trim().to_ascii_uppercase();
        let iso = iso_currency::Currency::from_code(&normalized)
            .ok_or_else(|| CurrencyError::UnknownCode(code.to_string()))?;
        let exponent = iso
            .exponent()
            .ok_or_else(|| CurrencyError::NoMinorUnit(normalized.clone()))?;
        Ok(Self {
            iso,
            exponent: u32::from(exponent),
        })
    }

    /// Three-letter ISO code, e.g. `"EUR"`.
    pub fn code(&self) -> &'static str {
        self.iso.code()
    }

    /// Number of digits after the decimal separator (ISO 4217 exponent).
    pub fn exponent(&self) -> u32 {
        self.exponent
    }

    /// English ISO name, e.g. `"Japanese yen"`.
    pub fn name(&self) -> &str {
        self.iso.name()
    }

    /// Display symbol, e.g. `"¥"`; falls back to `¤` where ISO has none.
    pub fn symbol(&self) -> String {
        self.iso.symbol().to_string()
    }

    /// Currencies a user can pick, sorted by code: everything that carries
    /// money, minus funds (e.g. `CHE`), special units (e.g. `XDR`) and
    /// superseded codes (e.g. `HRK`), which nobody pays with any more.
    pub fn selectable() -> Vec<Self> {
        let mut all: Vec<Self> = iso_currency::Currency::iter()
            .filter(|iso| !iso.is_fund() && !iso.is_special() && iso.is_superseded().is_none())
            .filter_map(|iso| Self::from_code(iso.code()).ok())
            .collect();
        all.sort_by_key(|currency| currency.code());
        all
    }

    /// Search match on code or name, case-insensitive; an empty query
    /// matches everything.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || self.code().to_lowercase().contains(&query)
            || self.name().to_lowercase().contains(&query)
    }
}

impl fmt::Debug for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Currency({})", self.code())
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponents_follow_iso_4217() {
        assert_eq!(Currency::from_code("EUR").unwrap().exponent(), 2);
        assert_eq!(Currency::from_code("JPY").unwrap().exponent(), 0);
        assert_eq!(Currency::from_code("KWD").unwrap().exponent(), 3);
        assert_eq!(Currency::from_code("CLF").unwrap().exponent(), 4);
    }

    #[test]
    fn code_is_case_insensitive() {
        let jpy = Currency::from_code(" jpy ").unwrap();
        assert_eq!(jpy.code(), "JPY");
        assert_eq!(jpy, Currency::from_code("JPY").unwrap());
    }

    #[test]
    fn rejects_unknown_and_unitless_codes() {
        assert_eq!(
            Currency::from_code("ABC"),
            Err(CurrencyError::UnknownCode("ABC".into()))
        );
        assert_eq!(
            Currency::from_code("XAU"),
            Err(CurrencyError::NoMinorUnit("XAU".into()))
        );
    }

    #[test]
    fn selectable_lists_only_current_payment_currencies() {
        let codes: Vec<_> = Currency::selectable().iter().map(|c| c.code()).collect();
        for code in ["EUR", "USD", "JPY", "CHF", "GBP", "KWD"] {
            assert!(codes.contains(&code), "missing {code}");
        }
        // special unit, fund, superseded
        for code in ["XAU", "XDR", "CHE", "CLF", "HRK"] {
            assert!(!codes.contains(&code), "unexpected {code}");
        }
        assert!(codes.is_sorted());
    }

    #[test]
    fn matches_code_and_name_case_insensitively() {
        let jpy = Currency::from_code("JPY").unwrap();
        assert!(jpy.matches(""));
        assert!(jpy.matches("  jp "));
        assert!(jpy.matches("YEN"));
        assert!(!jpy.matches("euro"));
    }
}
