use dioxus::prelude::*;
use invuso_core::domain::Money;

use crate::format::{NumberFormat, format_money};

/// "Du bekommst 42,10 €" / "Du schuldest 12,00 €" / "Du bist ausgeglichen"
/// (PER-02), colored like a balance (idee.md 3.2). `class` adds layout.
#[component]
pub fn OwnBalance(balance: Money, #[props(default)] class: String) -> Element {
    let amount = format_money(
        Money::new(balance.amount_minor().saturating_abs(), balance.currency()),
        NumberFormat::current(),
    );
    let (text, color) = match balance.amount_minor().signum() {
        1 => (
            t!("summary.you_get", amount = amount),
            "text-muted-teal-300",
        ),
        -1 => (
            t!("summary.you_owe", amount = amount),
            "text-watermelon-300",
        ),
        _ => (t!("summary.you_even"), "text-floral-white-300"),
    };
    rsx! {
        p { class: "text-base font-medium tabular-nums {color} {class}", "{text}" }
    }
}
