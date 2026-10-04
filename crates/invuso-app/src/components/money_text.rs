use dioxus::prelude::*;
use invuso_core::domain::Money;

use crate::format::{NumberFormat, format_money};

/// Amount with currency in the app language's format (UI-05), e.g.
/// `1.234,56 €`. `signed` colors it as a balance: positive = gets money,
/// negative = owes money (idee.md 3.2). `class` sets size and weight.
#[component]
pub fn MoneyText(
    amount: Money,
    #[props(default)] signed: bool,
    #[props(default)] class: String,
) -> Element {
    let text = format_money(amount, NumberFormat::current());
    let color = match (signed, amount.amount_minor().signum()) {
        (true, 1) => "text-muted-teal-300",
        (true, -1) => "text-watermelon-300",
        _ => "",
    };

    rsx! {
        span { class: "whitespace-nowrap tabular-nums {color} {class}", "{text}" }
    }
}
