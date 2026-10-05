//! Work above the repositories: saving expenses with their rate, group
//! totals and balances, fetching exchange rates, archiving receipt images
//! recognizing their text and translating its lines
//! (idee.md 2.3 `services/`).

pub mod expenses;
pub mod ocr;
pub mod rates;
pub mod receipts;
pub mod summary;
pub mod translation;
