//! Local SQLite storage (CORE-04, idee.md 4). Views never touch SQL; they
//! call the repository methods on [`Db`].

// Self-expiring: once the M1 screens call the repositories, this
// expectation goes unfulfilled and the compiler asks for its removal.
#![cfg_attr(
    not(test),
    expect(dead_code, reason = "repositories are wired up by the M1 screens")
)]

mod db;
mod migrations;
mod people;
mod profile;
mod settings;

pub use db::{Db, StorageError};
pub use profile::Profile;
