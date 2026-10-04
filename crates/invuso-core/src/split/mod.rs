//! Splitting expenses, balances and debt simplification (idee.md 8.1–8.4).
//!
//! All amounts are integer minor units of one currency; callers convert to
//! the group's base currency first (see [`rescale`]). Every function is
//! deterministic: ties are broken by [`PersonId`](crate::domain::PersonId)
//! order, so all devices compute identical results.

mod allocate;
mod balance;
mod items;
mod mode;
mod settle;
mod summary;

pub use allocate::{allocate, rescale};
pub use balance::{ExpenseEntry, PersonTotals, SettlementEntry, balances};
pub use items::{ItemLine, split_by_items};
pub use mode::{SplitMode, split};
pub use settle::{Transfer, simplify_debts};
pub use summary::{GroupSummary, summarize};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SplitError {
    #[error("nobody to split between")]
    NoParticipants,
    #[error("weights must not be negative")]
    NegativeWeight,
    #[error("all weights are zero")]
    ZeroTotalWeight,
    #[error("percentages add up to {0}, not 100")]
    PercentNot100(rust_decimal::Decimal),
    #[error("exact amounts add up to {actual}, not {expected}")]
    ExactSumMismatch { expected: i64, actual: i64 },
    #[error("parts mix positive and negative amounts and cannot be scaled")]
    MixedSigns,
    #[error("payments ({paid}) and shares ({shared}) of an expense differ")]
    UnbalancedExpense { paid: i64, shared: i64 },
    #[error("balances add up to {0}, not 0")]
    BalancesDoNotSumToZero(i64),
    #[error("amount out of range")]
    Overflow,
}
