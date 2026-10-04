use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdBed, LdBus, LdHeartPulse, LdShoppingBag, LdShoppingCart, LdTag, LdTicket, LdUtensils,
    },
};

/// Lucide glyph for a category icon key (migration 0002).
#[component]
pub fn CategoryIconGlyph(
    icon: String,
    #[props(default = "h-4 w-4".to_string())] class: String,
) -> Element {
    match icon.as_str() {
        "utensils" => rsx! { Icon { icon: LdUtensils, class } },
        "shopping-cart" => rsx! { Icon { icon: LdShoppingCart, class } },
        "bus" => rsx! { Icon { icon: LdBus, class } },
        "bed" => rsx! { Icon { icon: LdBed, class } },
        "ticket" => rsx! { Icon { icon: LdTicket, class } },
        "shopping-bag" => rsx! { Icon { icon: LdShoppingBag, class } },
        "heart-pulse" => rsx! { Icon { icon: LdHeartPulse, class } },
        // "tag" and keys this version does not know.
        _ => rsx! { Icon { icon: LdTag, class } },
    }
}
