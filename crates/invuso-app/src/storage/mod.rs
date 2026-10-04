//! Local SQLite storage (CORE-04, idee.md 4). Views never touch SQL; they
//! call the repository methods on [`Db`].

// Self-expiring: once the M1 screens call the repositories, this
// expectation goes unfulfilled and the compiler asks for its removal.
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "repositories are wired up by the M1 screens")
)]

mod categories;
mod db;
mod exchange_rates;
mod expenses;
mod group_members;
mod groups;
mod migrations;
mod payment_methods;
mod people;
mod profile;
mod settings;

pub(crate) use db::now_ms;
pub use db::{Db, StorageError};
pub use exchange_rates::{NearRate, NewExchangeRate, RateQuote};
pub use expenses::{ExpenseListEntry, NewExpense, NewExpensePayment};
pub use groups::NewGroup;
pub use payment_methods::NewPaymentMethod;
pub use people::NewPerson;
pub use profile::Profile;
pub use settings::{
    CONVERTER_FROM, CONVERTER_TO, FAVORITE_CURRENCY_LIST, LAST_EXPENSE_CURRENCY,
    LAST_EXPENSE_GROUP, RECENT_CURRENCY_LIST,
};
