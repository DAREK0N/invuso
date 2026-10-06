//! Local SQLite storage (CORE-04, idee.md 4). Views never touch SQL; they
//! call the repository methods on [`Db`].

// Self-expiring: once the M1 screens call the repositories, this
// expectation goes unfulfilled and the compiler asks for its removal.
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "repositories are wired up by the M1 screens")
)]

mod backup;
mod cash;
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
mod receipts;
mod settings;
mod settlements;
mod translations;

pub use cash::{CashEntry, NewExchange, NewWithdrawal, WithdrawalFee};
pub use categories::NewCategory;
pub(crate) use db::now_ms;
pub use db::{Db, StorageError};
pub use exchange_rates::{
    CROSS_SOURCE, HistoryEntry, MANUAL_SOURCE, NearRate, NewExchangeRate, RateQuote,
};
pub use expenses::{
    ExpenseParties, NewExpense, NewExpensePayment, RecentExpense, TimelineEntry, TimelinePayer,
};
pub use groups::NewGroup;
pub use payment_methods::NewPaymentMethod;
pub use people::NewPerson;
pub use profile::Profile;
pub use receipts::{OcrFragment, ReceiptFiles, ReceiptText};
pub use settings::{
    APP_LANGUAGE, CONVERTER_FROM, CONVERTER_MANUAL_RATE, CONVERTER_TO, CORNER_RADIUS,
    FAVORITE_CURRENCY_LIST, LAST_EXPENSE_CURRENCY, LAST_EXPENSE_GROUP, RECENT_CURRENCY_LIST,
    SURFACE_SHADOWS, THEME, TRANSLATION_CONFIDENCE, UI_SCALE,
};
pub use settlements::NewSettlement;
pub use translations::UNKNOWN_LANGUAGE;
