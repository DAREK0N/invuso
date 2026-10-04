//! One component per route (idee.md 6). Screens not built yet render
//! `PlaceholderPage` until their milestone.

mod cash;
mod converter;
mod expense;
mod groups;
mod home;
mod not_found;
mod onboarding;
mod placeholder;
mod receipt;
mod settings;

pub use cash::*;
pub use converter::*;
pub use expense::*;
pub use groups::*;
pub use home::*;
pub use not_found::*;
pub use onboarding::*;
pub use receipt::*;
pub use settings::*;

use placeholder::PlaceholderPage;
