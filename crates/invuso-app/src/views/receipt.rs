use dioxus::prelude::*;

use super::PlaceholderPage;

#[component]
pub fn Scan() -> Element {
    rsx! { PlaceholderPage { title: t!("page.scan").to_string(), show_back: true } }
}

#[component]
pub fn ReceiptReview(receipt_id: String) -> Element {
    rsx! { PlaceholderPage { title: t!("page.receipt_review").to_string(), show_back: true } }
}

#[component]
pub fn ReceiptArchive() -> Element {
    rsx! { PlaceholderPage { title: t!("page.receipts").to_string(), show_back: true } }
}
