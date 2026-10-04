//! Pure domain logic of Invuso.
//!
//! This crate must stay free of UI, platform, database, network and file
//! system dependencies so the Android app, and later the server and the web
//! version, can share it unchanged (see `idee.md` 2.3 and 2.5).

#![forbid(unsafe_code)]
