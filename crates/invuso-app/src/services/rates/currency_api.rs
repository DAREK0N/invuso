use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::value::RawValue;

use super::{RateError, RateProvider, get_text, non_empty, usable_rate};
use crate::storage::NewExchangeRate;

/// fawazahmed0/exchange-api (<https://github.com/fawazahmed0/exchange-api>):
/// free, no key, daily, 200+ currencies. The fallback for currencies
/// Frankfurter does not carry (idee.md 10.3).
pub struct CurrencyApi;

/// The project names two mirrors and asks clients to try the second when
/// the first fails. `{tag}` is `latest` or a `YYYY-MM-DD` date.
const MIRRORS: [&str; 2] = [
    "https://cdn.jsdelivr.net/npm/@fawazahmed0/currency-api@{tag}/v1/currencies/eur.min.json",
    "https://{tag}.currency-api.pages.dev/v1/currencies/eur.min.json",
];

/// `{"date":"2026-10-04","eur":{"jpy":177.71378084,"btc":0.0000132…,…}}`
#[derive(Deserialize)]
struct Body<'a> {
    date: String,
    #[serde(borrow)]
    eur: BTreeMap<String, &'a RawValue>,
}

impl CurrencyApi {
    fn fetch(&self, tag: &str) -> Result<Vec<NewExchangeRate>, RateError> {
        let mut last_error = None;
        for mirror in MIRRORS {
            match get_text(&mirror.replace("{tag}", tag)) {
                Ok(body) => return parse(&body),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| RateError::Format("no mirror".into())))
    }
}

impl RateProvider for CurrencyApi {
    fn source(&self) -> &'static str {
        "currency-api"
    }

    fn latest(&self) -> Result<Vec<NewExchangeRate>, RateError> {
        self.fetch("latest")
    }

    fn on_date(&self, date: &str) -> Result<Vec<NewExchangeRate>, RateError> {
        self.fetch(date)
    }
}

fn parse(body: &str) -> Result<Vec<NewExchangeRate>, RateError> {
    let body: Body<'_> =
        serde_json::from_str(body).map_err(|e| RateError::Format(e.to_string()))?;
    non_empty(
        body.eur
            .iter()
            .filter_map(|(code, raw)| usable_rate("EUR", code, raw.get(), &body.date))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use invuso_core::Decimal;

    use super::*;

    #[test]
    fn parses_real_response_and_keeps_only_money_currencies() {
        let rates = parse(include_str!("testdata/currency_api_eur.json")).unwrap();
        let codes: Vec<_> = rates.iter().map(|r| r.rate.quote().code()).collect();
        for code in ["BGN", "CUC", "JPY", "KWD", "USD", "VED"] {
            assert!(codes.contains(&code), "{code} missing in {codes:?}");
        }
        // Crypto tokens, gold and the base itself are dropped.
        for code in ["BTC", "SHIB", "XAU", "EUR"] {
            assert!(!codes.contains(&code), "{code} kept");
        }
        let jpy = rates
            .iter()
            .find(|r| r.rate.quote().code() == "JPY")
            .unwrap();
        assert_eq!(jpy.rate.value(), Decimal::from_str("177.71378084").unwrap());
        assert_eq!(jpy.rate_date, "2026-10-04");
        assert!(rates.iter().all(|r| r.rate.base().code() == "EUR"));
    }

    #[test]
    fn broken_body_is_a_format_error() {
        assert!(matches!(parse("{}"), Err(RateError::Format(_))));
        assert!(matches!(
            parse(r#"{"date":"2026-10-04","eur":{"btc":1}}"#),
            Err(RateError::Format(_))
        ));
    }
}
