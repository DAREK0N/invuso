use serde::Deserialize;
use serde_json::value::RawValue;

use super::{RateError, RateProvider, get_text, non_empty, usable_rate};
use crate::storage::NewExchangeRate;

/// Frankfurter API v2 (<https://frankfurter.dev>): free, no key, rates
/// blended from many central banks, about 165 currencies, each with the
/// day it was published.
pub struct Frankfurter;

const URL: &str = "https://api.frankfurter.dev/v2/rates?base=EUR";

/// One element of the response array, e.g.
/// `{"date":"2026-10-04","base":"EUR","quote":"JPY","rate":178.23}`.
#[derive(Deserialize)]
struct Entry<'a> {
    date: String,
    base: String,
    quote: String,
    #[serde(borrow)]
    rate: &'a RawValue,
}

impl RateProvider for Frankfurter {
    fn source(&self) -> &'static str {
        "frankfurter"
    }

    fn latest(&self) -> Result<Vec<NewExchangeRate>, RateError> {
        parse(&get_text(URL)?)
    }

    fn on_date(&self, date: &str) -> Result<Vec<NewExchangeRate>, RateError> {
        parse(&get_text(&format!("{URL}&date={date}"))?)
    }
}

fn parse(body: &str) -> Result<Vec<NewExchangeRate>, RateError> {
    let entries: Vec<Entry<'_>> =
        serde_json::from_str(body).map_err(|e| RateError::Format(e.to_string()))?;
    non_empty(
        entries
            .iter()
            .filter_map(|e| usable_rate(&e.base, &e.quote, e.rate.get(), &e.date))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use invuso_core::Decimal;

    use super::*;

    #[test]
    fn parses_real_response() {
        let rates = parse(include_str!("testdata/frankfurter_v2_rates.json")).unwrap();
        // ANG was replaced by XCG and is no longer an ISO 4217 code.
        assert_eq!(rates.len(), 8);
        assert!(rates.iter().all(|r| r.rate.quote().code() != "ANG"));
        let find = |code: &str| {
            rates
                .iter()
                .find(|r| r.rate.quote().code() == code)
                .unwrap()
        };
        assert_eq!(
            find("JPY").rate.value(),
            Decimal::from_str("178.23").unwrap()
        );
        assert_eq!(
            find("KWD").rate.value(),
            Decimal::from_str("0.34814").unwrap()
        );
        assert_eq!(find("IDR").rate.value(), Decimal::from(20218));
        assert!(rates.iter().all(|r| r.rate.base().code() == "EUR"));
        // Each currency carries its own publication day.
        assert_eq!(find("AWG").rate_date, "2026-10-02");
        assert_eq!(find("USD").rate_date, "2026-10-04");
    }

    #[test]
    fn error_bodies_are_format_errors() {
        assert!(matches!(
            parse(r#"{"status":422,"message":"invalid currency: XYZ"}"#),
            Err(RateError::Format(_))
        ));
        assert!(matches!(parse("[]"), Err(RateError::Format(_))));
        assert!(matches!(parse("<html>"), Err(RateError::Format(_))));
    }
}
