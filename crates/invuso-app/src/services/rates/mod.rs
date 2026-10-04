//! Exchange rates from the network (FX-01, FX-09, FX-11; idee.md 10.3):
//! Frankfurter first, a second free provider for the currencies Frankfurter
//! does not carry. Every fetch is archived in the database.

mod currency_api;
mod frankfurter;
mod refresh;

use std::str::FromStr;
use std::time::Duration;

use invuso_core::Decimal;
use invuso_core::domain::{Currency, is_iso_date};
use invuso_core::fx::Rate;
use thiserror::Error;

use crate::storage::NewExchangeRate;

pub use currency_api::CurrencyApi;
pub use frankfurter::Frankfurter;
pub use refresh::use_rate_refresh;

#[derive(Debug, Error)]
pub enum RateError {
    #[error("network error: {0}")]
    Http(#[from] Box<ureq::Error>),
    #[error("unexpected response: {0}")]
    Format(String),
}

impl From<ureq::Error> for RateError {
    fn from(error: ureq::Error) -> Self {
        Self::Http(Box::new(error))
    }
}

/// A source of exchange rates. Calls block on the network and must run on
/// a background thread.
pub trait RateProvider: Send + Sync {
    /// Stored as `ExchangeRate.source`.
    fn source(&self) -> &'static str;

    /// The newest rates, quoted against EUR.
    fn latest(&self) -> Result<Vec<NewExchangeRate>, RateError>;

    /// Rates of a past day (`YYYY-MM-DD`), quoted against EUR (FX-09).
    fn on_date(&self, date: &str) -> Result<Vec<NewExchangeRate>, RateError>;
}

/// Long enough for a slow mobile connection, short enough that a dead one
/// does not keep the refresh hanging.
const TIMEOUT: Duration = Duration::from_secs(20);

fn get_text(url: &str) -> Result<String, RateError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .build()
        .into();
    let mut response = agent.get(url).call()?;
    Ok(response.body_mut().read_to_string()?)
}

/// Turns one reported rate into a record, or `None` for entries the app
/// cannot use (crypto tokens, metals, the base itself, broken numbers).
/// `raw` is the number exactly as written in the JSON, so no float is
/// involved.
fn usable_rate(base: &str, quote: &str, raw: &str, date: &str) -> Option<NewExchangeRate> {
    let base = Currency::from_code(base).ok()?;
    let quote = Currency::from_code(quote).ok()?;
    if base == quote || !is_iso_date(date) {
        return None;
    }
    let value = Decimal::from_str(raw)
        .or_else(|_| Decimal::from_scientific(raw))
        .ok()?;
    Some(NewExchangeRate {
        rate: Rate::new(base, quote, value).ok()?,
        rate_date: date.to_string(),
    })
}

fn non_empty(rates: Vec<NewExchangeRate>) -> Result<Vec<NewExchangeRate>, RateError> {
    if rates.is_empty() {
        Err(RateError::Format("no usable rates".into()))
    } else {
        Ok(rates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_exact_decimals_and_skips_unusable_entries() {
        let rate = usable_rate("EUR", "JPY", "177.71378084", "2026-10-04").unwrap();
        assert_eq!(
            rate.rate.value(),
            Decimal::from_str("177.71378084").unwrap()
        );
        let tiny = usable_rate("EUR", "USD", "1.3e-5", "2026-10-04").unwrap();
        assert_eq!(tiny.rate.value(), Decimal::from_str("0.000013").unwrap());
        assert_eq!(usable_rate("EUR", "BTC", "0.00001", "2026-10-04"), None);
        assert_eq!(usable_rate("EUR", "XAU", "0.0003", "2026-10-04"), None);
        assert_eq!(usable_rate("EUR", "EUR", "1", "2026-10-04"), None);
        assert_eq!(usable_rate("EUR", "JPY", "0", "2026-10-04"), None);
        assert_eq!(usable_rate("EUR", "JPY", "abc", "2026-10-04"), None);
        assert_eq!(usable_rate("EUR", "JPY", "170", "04.10.2026"), None);
    }
}
