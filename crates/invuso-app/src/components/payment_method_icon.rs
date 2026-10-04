use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdBanknote, LdCoins, LdCreditCard, LdLandmark, LdPiggyBank, LdSmartphone, LdTrainFront,
        LdWallet,
    },
};

use crate::components::color_classes;

/// Lucide glyph for an icon key from `preferences::PAYMENT_ICONS`.
#[component]
pub fn PaymentIconGlyph(
    icon: String,
    #[props(default = "h-5 w-5".to_string())] class: String,
) -> Element {
    match icon.as_str() {
        "banknote" => rsx! { Icon { icon: LdBanknote, class } },
        "wallet" => rsx! { Icon { icon: LdWallet, class } },
        "smartphone" => rsx! { Icon { icon: LdSmartphone, class } },
        "landmark" => rsx! { Icon { icon: LdLandmark, class } },
        "train-front" => rsx! { Icon { icon: LdTrainFront, class } },
        "coins" => rsx! { Icon { icon: LdCoins, class } },
        "piggy-bank" => rsx! { Icon { icon: LdPiggyBank, class } },
        // "credit-card" and keys this version does not know.
        _ => rsx! { Icon { icon: LdCreditCard, class } },
    }
}

/// Round badge with a payment method's icon on its color, the counterpart
/// of a person's `Avatar` in lists.
#[component]
pub fn PaymentMethodIcon(icon: String, color: String) -> Element {
    rsx! {
        span {
            class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full {color_classes(&color)}",
            aria_hidden: "true",
            PaymentIconGlyph { icon }
        }
    }
}
