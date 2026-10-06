use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdBed, LdBeer, LdBus, LdCamera, LdCar, LdCoffee, LdDumbbell, LdFilm, LdFuel, LdGift,
        LdGraduationCap, LdHeartPulse, LdHome, LdMusic, LdPawPrint, LdPill, LdPlane, LdReceipt,
        LdShirt, LdShoppingBag, LdShoppingCart, LdSmartphone, LdTag, LdTicket, LdTrainFront,
        LdUtensils,
    },
};

use crate::components::color_classes;

/// Lucide glyph for a category icon key from `preferences::CATEGORY_ICONS`
/// (the defaults of migration 0002 among them).
#[component]
pub fn CategoryIconGlyph(
    icon: String,
    #[props(default = "h-4 w-4".to_string())] class: String,
) -> Element {
    match icon.as_str() {
        "utensils" => rsx! { Icon { icon: LdUtensils, class } },
        "coffee" => rsx! { Icon { icon: LdCoffee, class } },
        "beer" => rsx! { Icon { icon: LdBeer, class } },
        "shopping-cart" => rsx! { Icon { icon: LdShoppingCart, class } },
        "shopping-bag" => rsx! { Icon { icon: LdShoppingBag, class } },
        "shirt" => rsx! { Icon { icon: LdShirt, class } },
        "gift" => rsx! { Icon { icon: LdGift, class } },
        "bus" => rsx! { Icon { icon: LdBus, class } },
        "train-front" => rsx! { Icon { icon: LdTrainFront, class } },
        "plane" => rsx! { Icon { icon: LdPlane, class } },
        "car" => rsx! { Icon { icon: LdCar, class } },
        "fuel" => rsx! { Icon { icon: LdFuel, class } },
        "bed" => rsx! { Icon { icon: LdBed, class } },
        "home" => rsx! { Icon { icon: LdHome, class } },
        "ticket" => rsx! { Icon { icon: LdTicket, class } },
        "film" => rsx! { Icon { icon: LdFilm, class } },
        "music" => rsx! { Icon { icon: LdMusic, class } },
        "camera" => rsx! { Icon { icon: LdCamera, class } },
        "heart-pulse" => rsx! { Icon { icon: LdHeartPulse, class } },
        "pill" => rsx! { Icon { icon: LdPill, class } },
        "dumbbell" => rsx! { Icon { icon: LdDumbbell, class } },
        "paw-print" => rsx! { Icon { icon: LdPawPrint, class } },
        "smartphone" => rsx! { Icon { icon: LdSmartphone, class } },
        "receipt" => rsx! { Icon { icon: LdReceipt, class } },
        "graduation-cap" => rsx! { Icon { icon: LdGraduationCap, class } },
        // "tag" and keys this version does not know.
        _ => rsx! { Icon { icon: LdTag, class } },
    }
}

/// Round badge with a category's icon on its color, for the category list.
#[component]
pub fn CategoryIcon(icon: String, color: String) -> Element {
    rsx! {
        span {
            class: "flex h-11 w-11 shrink-0 items-center justify-center rounded-full {color_classes(&color)}",
            aria_hidden: "true",
            CategoryIconGlyph { icon, class: "h-5 w-5".to_string() }
        }
    }
}
