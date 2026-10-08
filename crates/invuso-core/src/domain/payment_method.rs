use std::fmt;

use rust_decimal::Decimal;
use thiserror::Error;

use super::{Currency, Money, MoneyError, PersonId};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PaymentMethodError {
    #[error("unknown payment method kind `{0}`")]
    UnknownKind(String),
    /// Only the last four digits of a card may ever be stored (AGENTS.md 7.6).
    #[error("last digits must be exactly 4 digits")]
    InvalidLast4,
    #[error("only cards have last digits")]
    Last4NotAllowed,
    #[error("cash has no account currency")]
    CurrencyNotAllowed,
    #[error("only cards have fees")]
    FeesNotAllowed,
    #[error("foreign fee must be between 0 and 100 percent")]
    InvalidFeePercent,
    #[error("withdrawal fee must not be negative")]
    InvalidFixedFee,
    #[error("withdrawal fee must be in the account currency")]
    FeeCurrencyMismatch,
}

/// Identifier of a `PaymentMethod` (idee.md 4.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaymentMethodId(pub String);

impl PaymentMethodId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PaymentMethodId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What kind of money a payment method is (idee.md 4.1). All kinds count the
/// same for balances; the kind only describes and groups payments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaymentMethodKind {
    Cash,
    CreditCard,
    DebitCard,
    PayPal,
    BankTransfer,
    /// Prepaid transit cards such as Suica.
    IcCard,
    Other,
}

impl PaymentMethodKind {
    /// Every kind, in the order a picker shows them.
    pub const ALL: [Self; 7] = [
        Self::Cash,
        Self::CreditCard,
        Self::DebitCard,
        Self::PayPal,
        Self::BankTransfer,
        Self::IcCard,
        Self::Other,
    ];

    /// Stable code stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            Self::Cash => "cash",
            Self::CreditCard => "credit_card",
            Self::DebitCard => "debit_card",
            Self::PayPal => "paypal",
            Self::BankTransfer => "bank_transfer",
            Self::IcCard => "ic_card",
            Self::Other => "other",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, PaymentMethodError> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.code() == code)
            .ok_or_else(|| PaymentMethodError::UnknownKind(code.to_string()))
    }

    /// Whether the method has a card number whose last digits may be kept.
    pub fn has_card_number(self) -> bool {
        matches!(self, Self::CreditCard | Self::DebitCard)
    }

    /// Whether the method is an account in one currency (PAY-04). Cash is
    /// in whatever currency it is paid in.
    pub fn has_account_currency(self) -> bool {
        self != Self::Cash
    }

    /// Whether foreign and withdrawal fees can be kept (PAY-04; user
    /// decision in AP-31: cards only, like the last digits).
    pub fn has_fees(self) -> bool {
        self.has_card_number()
    }
}

/// Account currency and fees of a payment method (PAY-04). Empty means
/// unknown; the fees are only recorded, never added to expenses (PAY-07).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountTerms {
    /// What the method is charged in, e.g. USD for a US card.
    pub currency: Option<Currency>,
    /// Foreign transaction fee in percent, e.g. `1.75`.
    pub foreign_fee_percent: Option<Decimal>,
    /// Fixed fee per cash withdrawal, in the account currency if there is
    /// one.
    pub fixed_fee: Option<Money>,
}

impl AccountTerms {
    /// Checks the terms for a method of `kind`: no currency on cash, fees
    /// only on cards, 0–100 %, no negative fee, the fee in the account
    /// currency. Zero fees count as none.
    pub fn validate(self, kind: PaymentMethodKind) -> Result<Self, PaymentMethodError> {
        if self.currency.is_some() && !kind.has_account_currency() {
            return Err(PaymentMethodError::CurrencyNotAllowed);
        }
        let percent = self.foreign_fee_percent.filter(|p| !p.is_zero());
        let fixed_fee = self.fixed_fee.filter(|f| !f.is_zero());
        if (percent.is_some() || fixed_fee.is_some()) && !kind.has_fees() {
            return Err(PaymentMethodError::FeesNotAllowed);
        }
        if percent.is_some_and(|p| p.is_sign_negative() || p > Decimal::ONE_HUNDRED) {
            return Err(PaymentMethodError::InvalidFeePercent);
        }
        if let Some(fee) = fixed_fee {
            if fee.is_negative() {
                return Err(PaymentMethodError::InvalidFixedFee);
            }
            if self.currency.is_some_and(|c| c != fee.currency()) {
                return Err(PaymentMethodError::FeeCurrencyMismatch);
            }
        }
        Ok(Self {
            currency: self.currency,
            foreign_fee_percent: percent.map(|p| p.normalize()),
            fixed_fee,
        })
    }

    /// Fee of a cash withdrawal for which the card was `charged` (user
    /// decision in AP-31): the fixed fee plus the foreign fee in percent of
    /// the charged amount, rounded half to even (idee.md 8.4). `charged` is
    /// `None` when the cash is in the card's own currency, which is no
    /// foreign transaction. A fixed fee in another currency than the charge
    /// cannot be added up; it is suggested alone then.
    pub fn withdrawal_fee(&self, charged: Option<Money>) -> Result<Option<Money>, MoneyError> {
        let percent_part = match (charged, self.foreign_fee_percent) {
            (Some(charged), Some(percent)) => {
                let part = charged
                    .to_decimal()
                    .checked_mul(percent)
                    .and_then(|v| v.checked_div(Decimal::ONE_HUNDRED))
                    .ok_or(MoneyError::Overflow)?;
                Some(Money::from_decimal(part, charged.currency())?).filter(|m| !m.is_zero())
            }
            _ => None,
        };
        Ok(match (self.fixed_fee, percent_part) {
            (Some(fixed), Some(part)) if fixed.currency() == part.currency() => {
                Some(fixed.checked_add(part)?)
            }
            (Some(fixed), _) => Some(fixed),
            (None, part) => part,
        })
    }
}

/// A card, cash, PayPal account … that someone pays with (idee.md 4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentMethod {
    pub id: PaymentMethodId,
    /// E.g. "Visa DKB".
    pub name: String,
    pub kind: PaymentMethodKind,
    pub owner_person_id: Option<PersonId>,
    /// Last four digits of a card, never more (AGENTS.md 7.6).
    pub last4: Option<String>,
    pub account: AccountTerms,
    /// Design-token name, e.g. `"cerulean"`.
    pub color: String,
    /// Icon key, e.g. `"credit-card"`.
    pub icon: String,
    /// Hidden from pickers, still shown on old payments.
    pub archived: bool,
}

impl PaymentMethod {
    /// The currency a withdrawal with this method is charged in: the
    /// account currency, else `home` (CASH-03, PAY-04).
    pub fn charge_currency(&self, home: Currency) -> Currency {
        self.account.currency.unwrap_or(home)
    }
}

/// Checks the last digits of a card: blank means none, otherwise exactly four
/// ASCII digits, and only for kinds with a card number.
pub fn validate_last4(
    kind: PaymentMethodKind,
    last4: Option<&str>,
) -> Result<Option<String>, PaymentMethodError> {
    let Some(digits) = last4.map(str::trim).filter(|d| !d.is_empty()) else {
        return Ok(None);
    };
    if !kind.has_card_number() {
        return Err(PaymentMethodError::Last4NotAllowed);
    }
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(PaymentMethodError::InvalidLast4);
    }
    Ok(Some(digits.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_codes_round_trip() {
        for kind in PaymentMethodKind::ALL {
            assert_eq!(PaymentMethodKind::from_code(kind.code()), Ok(kind));
        }
        assert_eq!(
            PaymentMethodKind::from_code("visa"),
            Err(PaymentMethodError::UnknownKind("visa".into()))
        );
    }

    #[test]
    fn accepts_four_digits_on_cards() {
        let card = PaymentMethodKind::CreditCard;
        assert_eq!(validate_last4(card, Some("0042")), Ok(Some("0042".into())));
        assert_eq!(
            validate_last4(PaymentMethodKind::DebitCard, Some(" 1234 ")),
            Ok(Some("1234".into()))
        );
    }

    #[test]
    fn blank_means_none() {
        let card = PaymentMethodKind::CreditCard;
        assert_eq!(validate_last4(card, None), Ok(None));
        assert_eq!(validate_last4(card, Some("  ")), Ok(None));
        assert_eq!(validate_last4(PaymentMethodKind::Cash, Some("")), Ok(None));
    }

    #[test]
    fn rejects_anything_but_four_digits() {
        let card = PaymentMethodKind::CreditCard;
        for bad in [
            "123",
            "12345",
            "12a4",
            "12 4",
            "１２３４",
            "-123",
            "4111111111111111",
        ] {
            assert_eq!(
                validate_last4(card, Some(bad)),
                Err(PaymentMethodError::InvalidLast4),
                "{bad}"
            );
        }
    }

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    fn terms(currency: Option<&str>, percent: Option<&str>, fee: Option<Money>) -> AccountTerms {
        AccountTerms {
            currency: currency.map(cur),
            foreign_fee_percent: percent.map(|p| p.parse().unwrap()),
            fixed_fee: fee,
        }
    }

    #[test]
    fn card_keeps_currency_and_fees() {
        let usd = Money::new(500, cur("USD"));
        let checked = terms(Some("USD"), Some("1.750"), Some(usd))
            .validate(PaymentMethodKind::CreditCard)
            .unwrap();
        assert_eq!(checked, terms(Some("USD"), Some("1.75"), Some(usd)));
        assert_eq!(
            AccountTerms::default().validate(PaymentMethodKind::DebitCard),
            Ok(AccountTerms::default())
        );
        // 100 % is odd, but the upper bound.
        assert!(
            terms(None, Some("100"), None)
                .validate(PaymentMethodKind::CreditCard)
                .is_ok()
        );
    }

    #[test]
    fn zero_fees_mean_none() {
        let zero = Some(Money::new(0, cur("EUR")));
        let checked = terms(Some("EUR"), Some("0.00"), zero)
            .validate(PaymentMethodKind::CreditCard)
            .unwrap();
        assert_eq!(checked, terms(Some("EUR"), None, None));
        // No fee at all is fine on any kind.
        assert_eq!(
            terms(None, Some("0"), zero).validate(PaymentMethodKind::Cash),
            Ok(AccountTerms::default())
        );
    }

    #[test]
    fn account_currency_on_every_kind_but_cash() {
        for kind in PaymentMethodKind::ALL {
            let result = terms(Some("USD"), None, None).validate(kind);
            if kind == PaymentMethodKind::Cash {
                assert_eq!(result, Err(PaymentMethodError::CurrencyNotAllowed));
            } else {
                assert!(result.is_ok(), "{kind:?}");
            }
        }
    }

    #[test]
    fn fees_only_on_cards() {
        let fee = Some(Money::new(500, cur("EUR")));
        for kind in PaymentMethodKind::ALL
            .into_iter()
            .filter(|kind| !kind.has_card_number())
        {
            assert_eq!(
                terms(None, Some("1.5"), None).validate(kind),
                Err(PaymentMethodError::FeesNotAllowed)
            );
            assert_eq!(
                terms(None, None, fee).validate(kind),
                Err(PaymentMethodError::FeesNotAllowed)
            );
        }
    }

    #[test]
    fn rejects_invalid_fees() {
        let card = PaymentMethodKind::CreditCard;
        for bad in ["-1", "100.01", "250"] {
            assert_eq!(
                terms(None, Some(bad), None).validate(card),
                Err(PaymentMethodError::InvalidFeePercent),
                "{bad}"
            );
        }
        assert_eq!(
            terms(None, None, Some(Money::new(-1, cur("EUR")))).validate(card),
            Err(PaymentMethodError::InvalidFixedFee)
        );
        assert_eq!(
            terms(Some("USD"), None, Some(Money::new(500, cur("EUR")))).validate(card),
            Err(PaymentMethodError::FeeCurrencyMismatch)
        );
        // Without an account currency the fee may be in any currency.
        assert!(
            terms(None, None, Some(Money::new(500, cur("JPY"))))
                .validate(card)
                .is_ok()
        );
    }

    #[test]
    fn withdrawal_fee_adds_the_foreign_fee_to_the_fixed_fee() {
        let usd = |minor| Money::new(minor, cur("USD"));
        let card = terms(Some("USD"), Some("1.75"), Some(usd(500)));
        // 5,00 + 1,75 % of 130,50 (2,28375 → 2,28) = 7,28 USD.
        assert_eq!(card.withdrawal_fee(Some(usd(13_050))), Ok(Some(usd(728))));
        // Cash in the card's own currency: only the fixed fee.
        assert_eq!(card.withdrawal_fee(None), Ok(Some(usd(500))));
        // Half to even: 1 % of 0,50 = 0,005 → 0,00; of 1,50 = 0,015 → 0,02.
        let percent_only = terms(Some("USD"), Some("1"), None);
        assert_eq!(percent_only.withdrawal_fee(Some(usd(50))), Ok(None));
        assert_eq!(
            percent_only.withdrawal_fee(Some(usd(150))),
            Ok(Some(usd(2)))
        );
        // Currencies without minor units.
        let yen_card = terms(Some("JPY"), Some("2.2"), Some(Money::new(220, cur("JPY"))));
        assert_eq!(
            yen_card.withdrawal_fee(Some(Money::new(30_000, cur("JPY")))),
            Ok(Some(Money::new(880, cur("JPY"))))
        );
        assert_eq!(
            AccountTerms::default().withdrawal_fee(Some(usd(100))),
            Ok(None)
        );
    }

    #[test]
    fn withdrawal_fee_in_another_currency_is_suggested_alone() {
        let card = terms(None, Some("1.75"), Some(Money::new(250, cur("JPY"))));
        assert_eq!(
            card.withdrawal_fee(Some(Money::new(13_050, cur("EUR")))),
            Ok(Some(Money::new(250, cur("JPY"))))
        );
    }

    #[test]
    fn withdrawal_is_charged_in_the_account_currency() {
        let mut card = PaymentMethod {
            id: PaymentMethodId::new("visa"),
            name: "Visa".into(),
            kind: PaymentMethodKind::CreditCard,
            owner_person_id: None,
            last4: None,
            account: AccountTerms::default(),
            color: "cerulean".into(),
            icon: "credit-card".into(),
            archived: false,
        };
        assert_eq!(card.charge_currency(cur("EUR")), cur("EUR"));
        card.account.currency = Some(cur("USD"));
        assert_eq!(card.charge_currency(cur("EUR")), cur("USD"));
    }

    #[test]
    fn rejects_digits_on_non_cards() {
        for kind in PaymentMethodKind::ALL
            .into_iter()
            .filter(|kind| !kind.has_card_number())
        {
            assert_eq!(
                validate_last4(kind, Some("1234")),
                Err(PaymentMethodError::Last4NotAllowed)
            );
        }
    }
}
