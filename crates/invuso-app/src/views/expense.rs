use dioxus::prelude::*;

use super::PlaceholderPage;

#[component]
pub fn ExpenseNew() -> Element {
    rsx! { PlaceholderPage { title: t!("page.expense_new").to_string(), show_back: true } }
}

#[component]
pub fn ExpenseDetail(id: String) -> Element {
    rsx! { PlaceholderPage { title: t!("page.expense_detail").to_string(), show_back: true } }
}

#[component]
pub fn ExpenseEdit(id: String) -> Element {
    rsx! { PlaceholderPage { title: t!("page.expense_edit").to_string(), show_back: true } }
}
