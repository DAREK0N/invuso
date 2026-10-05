//! Pure domain logic of Invuso.
//!
//! This crate must stay free of UI, platform, database, network and file
//! system dependencies so the Android app, and later the server and the web
//! version, can share it unchanged (see `idee.md` 2.3 and 2.5).
//!
//! Money is always an integer amount in the currency's smallest unit; rates
//! and weights are decimals. Floating point never touches money.

#![forbid(unsafe_code)]

pub mod domain;
pub mod fx;
pub mod receipt;
pub mod split;

pub use rust_decimal::Decimal;
