use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdChevronRight};
use invuso_core::domain::Money;

use crate::components::{CategoryIconGlyph, MoneyText};

/// Tappable expense in a list (GRP-21, HOME-03): category icon, title and
/// a subtitle; the amount in the base currency and, if different, in its
/// own.
#[component]
pub fn ExpenseRow(
    icon: String,
    title: String,
    subtitle: String,
    total: Money,
    total_in_base: Money,
    onclick: EventHandler<()>,
) -> Element {
    let foreign = total.currency() != total_in_base.currency();

    rsx! {
        button {
            class: "flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            span {
                class: "flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-jet-black-800 text-floral-white-300",
                aria_hidden: "true",
                CategoryIconGlyph { icon, class: "h-5 w-5".to_string() }
            }
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "truncate text-base text-floral-white-50", "{title}" }
                span { class: "truncate text-sm text-floral-white-400", "{subtitle}" }
            }
            span { class: "flex shrink-0 flex-col items-end",
                MoneyText { amount: total_in_base, class: "text-base font-semibold text-floral-white-100" }
                if foreign {
                    MoneyText { amount: total, class: "text-sm text-floral-white-400" }
                }
            }
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
        }
    }
}
