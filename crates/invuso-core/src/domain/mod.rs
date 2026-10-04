//! Entities and value objects shared by all other modules.

mod currency;
mod money;
mod person;

pub use currency::{Currency, CurrencyError};
pub use money::{Money, MoneyError};
pub use person::{Person, PersonId};
