//! Calculator input of the currency converter (FX-06): what the keypad
//! builds, e.g. `1200+850`, and its exact value. Multiplication and
//! division come before addition and subtraction; everything is computed
//! with decimals and rounded only once, to the currency of the amount.

use rust_decimal::Decimal;
use thiserror::Error;

use crate::domain::{Currency, Money, MoneyError};

/// Integer digits one number may have: 10^12 even in a currency with four
/// decimals stays far below `i64::MAX` minor units.
pub const MAX_INTEGER_DIGITS: usize = 12;

/// Numbers one expression may chain; enough for adding up a receipt.
const MAX_NUMBERS: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl Operator {
    fn symbol(self) -> char {
        match self {
            Self::Add => '+',
            Self::Subtract => '-',
            Self::Multiply => '*',
            Self::Divide => '/',
        }
    }

    fn from_symbol(symbol: char) -> Option<Self> {
        match symbol {
            '+' => Some(Self::Add),
            '-' => Some(Self::Subtract),
            '*' => Some(Self::Multiply),
            '/' => Some(Self::Divide),
            _ => None,
        }
    }
}

/// One key of the keypad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// `0`–`9`.
    Digit(u8),
    /// The decimal separator.
    Decimal,
    Operator(Operator),
    Backspace,
    Clear,
    /// Replaces the expression by its result.
    Equals,
}

/// A piece of an expression, for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part<'a> {
    /// Digits with an optional `.` and fraction, e.g. `1200`, `12.5`, `3.`.
    Number(&'a str),
    Operator(Operator),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExpressionError {
    #[error("division by zero")]
    DivisionByZero,
    #[error("number too large")]
    Overflow,
}

impl From<MoneyError> for ExpressionError {
    fn from(_: MoneyError) -> Self {
        Self::Overflow
    }
}

/// What the keypad has typed so far, kept in a canonical form: digits, `.`
/// as decimal separator and `+ - * /`. Only keypad input builds it, so it
/// never starts with an operator, never has two in a row and no number has
/// more decimals than the currency allows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Expression(String);

impl Expression {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// True once there is something to calculate, e.g. `1200+`.
    pub fn has_operator(&self) -> bool {
        self.0.chars().any(|c| Operator::from_symbol(c).is_some())
    }

    /// Numbers and operators in order.
    pub fn parts(&self) -> Vec<Part<'_>> {
        let mut parts = Vec::new();
        let mut start = 0;
        for (index, c) in self.0.char_indices() {
            if let Some(operator) = Operator::from_symbol(c) {
                parts.push(Part::Number(&self.0[start..index]));
                parts.push(Part::Operator(operator));
                start = index + 1;
            }
        }
        if start < self.0.len() {
            parts.push(Part::Number(&self.0[start..]));
        }
        parts
    }

    /// The expression after pressing `key` while typing in `currency`.
    /// Keys that would make it invalid change nothing.
    pub fn press(&self, key: Key, currency: Currency) -> Self {
        let mut text = self.0.clone();
        let current = self.current_number();
        let decimals = currency.exponent() as usize;
        match key {
            Key::Digit(digit) if digit <= 9 => {
                let fraction = current.split_once('.').map(|(_, f)| f.len());
                let full = match fraction {
                    Some(len) => len >= decimals,
                    None => current.len() >= MAX_INTEGER_DIGITS,
                };
                if full || (current.is_empty() && self.numbers() >= MAX_NUMBERS) {
                    return self.clone();
                }
                // No leading zeros: "0" followed by 5 becomes "5".
                if current == "0" {
                    text.pop();
                }
                text.push(char::from(b'0' + digit));
            }
            Key::Digit(_) => {}
            Key::Decimal => {
                if decimals == 0 || current.contains('.') {
                    return self.clone();
                }
                if current.is_empty() {
                    if self.numbers() >= MAX_NUMBERS {
                        return self.clone();
                    }
                    text.push('0');
                }
                text.push('.');
            }
            Key::Operator(operator) => {
                match text.chars().last() {
                    None => return self.clone(),
                    Some(last) if Operator::from_symbol(last).is_some() || last == '.' => {
                        text.pop();
                    }
                    Some(_) => {}
                }
                text.push(operator.symbol());
            }
            Key::Backspace => {
                text.pop();
            }
            Key::Clear => text.clear(),
            Key::Equals => {
                return match self.amount(currency) {
                    Ok(Some(money)) if self.has_operator() && !money.is_negative() => {
                        Self(money.to_decimal().normalize().to_string())
                    }
                    _ => self.clone(),
                };
            }
        }
        Self(text)
    }

    /// The expression with every number cut to what `currency` allows,
    /// e.g. after switching from EUR to JPY `12.50+3` becomes `12+3`.
    pub fn fit(&self, currency: Currency) -> Self {
        let decimals = currency.exponent() as usize;
        let mut text = String::with_capacity(self.0.len());
        for part in self.parts() {
            match part {
                Part::Operator(operator) => text.push(operator.symbol()),
                Part::Number(number) => match number.split_once('.') {
                    None => text.push_str(number),
                    Some((integer, _)) if decimals == 0 => text.push_str(integer),
                    Some((integer, fraction)) => {
                        text.push_str(integer);
                        text.push('.');
                        text.extend(fraction.chars().take(decimals));
                    }
                },
            }
        }
        Self(text)
    }

    /// The exact value; `None` while nothing is typed. A trailing operator
    /// is ignored, so the result stays visible while the next number is
    /// still missing.
    pub fn evaluate(&self) -> Result<Option<Decimal>, ExpressionError> {
        let mut sum: Option<Decimal> = None;
        // The running product of the current `+`/`-` term.
        let mut term: Option<Decimal> = None;
        let mut pending: Option<Operator> = None;
        for part in self.parts() {
            match part {
                Part::Operator(operator) => pending = Some(operator),
                Part::Number(number) => {
                    let value = parse_number(number)?;
                    term = Some(match (term, pending) {
                        (None, _) => value,
                        (Some(left), Some(Operator::Multiply)) => {
                            left.checked_mul(value).ok_or(ExpressionError::Overflow)?
                        }
                        (Some(left), Some(Operator::Divide)) => {
                            if value.is_zero() {
                                return Err(ExpressionError::DivisionByZero);
                            }
                            left.checked_div(value).ok_or(ExpressionError::Overflow)?
                        }
                        (Some(left), add_or_subtract) => {
                            sum = Some(add(sum, left)?);
                            if add_or_subtract == Some(Operator::Subtract) {
                                -value
                            } else {
                                value
                            }
                        }
                    });
                    pending = None;
                }
            }
        }
        match term {
            None => Ok(None),
            Some(term) => add(sum, term).map(Some),
        }
    }

    /// The value as an amount of `currency`, rounded half to even to its
    /// minor unit (idee.md 8.4).
    pub fn amount(&self, currency: Currency) -> Result<Option<Money>, ExpressionError> {
        match self.evaluate()? {
            None => Ok(None),
            Some(value) => Ok(Some(Money::from_decimal(value, currency)?)),
        }
    }

    /// The number being typed: everything after the last operator.
    fn current_number(&self) -> &str {
        let start = self
            .0
            .rfind(|c| Operator::from_symbol(c).is_some())
            .map_or(0, |index| index + 1);
        &self.0[start..]
    }

    fn numbers(&self) -> usize {
        self.parts()
            .iter()
            .filter(|part| matches!(part, Part::Number(_)))
            .count()
    }
}

fn add(sum: Option<Decimal>, value: Decimal) -> Result<Decimal, ExpressionError> {
    match sum {
        None => Ok(value),
        Some(sum) => sum.checked_add(value).ok_or(ExpressionError::Overflow),
    }
}

fn parse_number(number: &str) -> Result<Decimal, ExpressionError> {
    let number = number.strip_suffix('.').unwrap_or(number);
    if number.is_empty() {
        return Ok(Decimal::ZERO);
    }
    number
        .parse::<Decimal>()
        .map_err(|_| ExpressionError::Overflow)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn cur(code: &str) -> Currency {
        Currency::from_code(code).unwrap()
    }

    /// Types `keys` (digits, `.`, `+-*/`, `<` backspace, `C`, `=`).
    fn typed(keys: &str, currency: &str) -> Expression {
        let currency = cur(currency);
        keys.chars().fold(Expression::new(), |expression, c| {
            let key = match c {
                '0'..='9' => Key::Digit(c as u8 - b'0'),
                '.' => Key::Decimal,
                '<' => Key::Backspace,
                'C' => Key::Clear,
                '=' => Key::Equals,
                _ => Key::Operator(Operator::from_symbol(c).unwrap()),
            };
            expression.press(key, currency)
        })
    }

    fn value(keys: &str, currency: &str) -> Option<Decimal> {
        typed(keys, currency).evaluate().unwrap()
    }

    fn d(text: &str) -> Decimal {
        Decimal::from_str(text).unwrap()
    }

    #[test]
    fn adds_two_yen_amounts() {
        let expression = typed("1200+850", "JPY");
        assert_eq!(expression.0, "1200+850");
        assert!(expression.has_operator());
        assert_eq!(
            expression.amount(cur("JPY")).unwrap(),
            Some(Money::new(2050, cur("JPY")))
        );
    }

    #[test]
    fn multiplication_and_division_come_first() {
        assert_eq!(value("2+3*4", "EUR"), Some(d("14")));
        assert_eq!(value("10-6/4", "EUR"), Some(d("8.5")));
        assert_eq!(value("2*3+4*5-1", "EUR"), Some(d("25")));
        assert_eq!(value("100/4/5", "EUR"), Some(d("5")));
        assert_eq!(value("5-8", "EUR"), Some(d("-3")));
    }

    #[test]
    fn rounds_once_at_the_end() {
        // 10 / 3 × 3 = 9.999…9 (28 digits) → 10.00 €, not 3.33 × 3 = 9.99 €.
        assert_eq!(
            typed("10/3*3", "EUR").amount(cur("EUR")).unwrap(),
            Some(Money::new(1000, cur("EUR")))
        );
        // 1000 ¥ / 3 = 333.33… → 333 ¥.
        assert_eq!(
            typed("1000/3", "JPY").amount(cur("JPY")).unwrap(),
            Some(Money::new(333, cur("JPY")))
        );
        // Ties go to the even minor unit: 0.125 € → 0.12 €.
        assert_eq!(
            typed("0.25/2", "EUR").amount(cur("EUR")).unwrap(),
            Some(Money::new(12, cur("EUR")))
        );
        // Three decimals (KWD).
        assert_eq!(
            typed("1.005+0.0025", "KWD").amount(cur("KWD")).unwrap(),
            Some(Money::new(1007, cur("KWD")))
        );
    }

    #[test]
    fn trailing_operator_and_separator_are_ignored_for_the_value() {
        assert_eq!(value("1200+", "JPY"), Some(d("1200")));
        assert_eq!(value("12.", "EUR"), Some(d("12")));
        assert_eq!(value("", "EUR"), None);
    }

    #[test]
    fn division_by_zero_is_an_error() {
        assert_eq!(
            typed("5/0", "EUR").evaluate(),
            Err(ExpressionError::DivisionByZero)
        );
        assert_eq!(
            typed("5/0.", "EUR").evaluate(),
            Err(ExpressionError::DivisionByZero)
        );
    }

    #[test]
    fn keeps_input_valid_for_the_currency() {
        // No decimal separator for yen, at most two decimals for euro.
        assert_eq!(typed("12.5", "JPY").0, "125");
        assert_eq!(typed("1.2345", "EUR").0, "1.23");
        assert_eq!(typed("1.5.5", "EUR").0, "1.55");
        assert_eq!(typed(".5", "EUR").0, "0.5");
        // No leading operator, no doubled operators, no leading zeros.
        assert_eq!(typed("+5", "EUR").0, "5");
        assert_eq!(typed("5+*3", "EUR").0, "5*3");
        assert_eq!(typed("5.+3", "EUR").0, "5+3");
        assert_eq!(typed("007+0", "EUR").0, "7+0");
        assert_eq!(typed("0.05", "EUR").0, "0.05");
        // At most twelve integer digits per number.
        assert_eq!(typed("1234567890123", "EUR").0, "123456789012");
    }

    #[test]
    fn backspace_clear_and_equals() {
        assert_eq!(typed("12+3<<", "EUR").0, "12");
        assert_eq!(typed("12+3C", "EUR").0, "");
        assert_eq!(typed("<", "EUR").0, "");
        assert_eq!(typed("1200+850=", "JPY").0, "2050");
        assert_eq!(typed("10/4=", "EUR").0, "2.5");
        // Equals keeps the expression if it cannot become a plain amount.
        assert_eq!(typed("5-8=", "EUR").0, "5-8");
        assert_eq!(typed("5/0=", "EUR").0, "5/0");
        assert_eq!(typed("12=", "EUR").0, "12");
        // The result can be calculated with further.
        assert_eq!(value("10/4=*2", "EUR"), Some(d("5")));
    }

    #[test]
    fn fits_to_a_currency_with_fewer_decimals() {
        let euro = typed("12.50+3.2", "EUR");
        assert_eq!(euro.fit(cur("JPY")).0, "12+3");
        assert_eq!(euro.fit(cur("KWD")).0, "12.50+3.2");
        assert_eq!(typed("1.255", "KWD").fit(cur("EUR")).0, "1.25");
        assert_eq!(typed("12.", "EUR").fit(cur("JPY")).0, "12");
    }

    #[test]
    fn parts_for_display() {
        assert_eq!(
            typed("1200+85.5*", "EUR").parts(),
            vec![
                Part::Number("1200"),
                Part::Operator(Operator::Add),
                Part::Number("85.5"),
                Part::Operator(Operator::Multiply),
            ]
        );
        assert!(Expression::new().parts().is_empty());
    }

    #[test]
    fn overflow_is_an_error_not_a_panic() {
        let big = "999999999999*999999999999*999999999999*999999999999";
        let expression = typed(big, "EUR");
        assert_eq!(expression.0, big);
        assert_eq!(expression.evaluate(), Err(ExpressionError::Overflow));
    }
}
