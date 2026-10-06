//! Work above the repositories: saving expenses with their rate, cash
//! withdrawals with their fee, group
//! totals and balances, the settlement as text, fetching exchange rates,
//! archiving receipt images, recognizing their text and translating its
//! lines (idee.md 2.3 `services/`).

pub mod cash;
pub mod converter;
pub mod expenses;
pub mod ocr;
pub mod rates;
pub mod receipts;
pub mod settlements;
pub mod summary;
pub mod translation;
