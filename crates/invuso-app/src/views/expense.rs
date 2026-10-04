use dioxus::prelude::*;

use super::PlaceholderPage;

mod form;
mod split;

pub use form::{ExpenseEdit, ExpenseNew};

#[component]
pub fn ExpenseDetail(id: String) -> Element {
    rsx! { PlaceholderPage { title: t!("page.expense_detail").to_string(), show_back: true } }
}
