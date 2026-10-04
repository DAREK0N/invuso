use std::fmt;

use thiserror::Error;

use super::PersonId;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PaymentMethodError {
    #[error("unknown payment method kind `{0}`")]
    UnknownKind(String),
    /// Only the last four digits of a card may ever be stored (AGENTS.md 7.6).
    #[error("last digits must be exactly 4 digits")]
    InvalidLast4,
    #[error("only cards have last digits")]
    Last4NotAllowed,
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
    /// Design-token name, e.g. `"cerulean"`.
    pub color: String,
    /// Icon key, e.g. `"credit-card"`.
    pub icon: String,
    /// Hidden from pickers, still shown on old payments.
    pub archived: bool,
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
