use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdBriefcase, LdCar, LdHeart, LdHome, LdLuggage, LdMountain, LdPartyPopper, LdPlane, LdTent,
        LdTreePalm, LdUsers, LdUtensils,
    },
};

use crate::components::color_classes;

/// Lucide glyph for an icon key from `preferences::GROUP_ICONS`.
#[component]
pub fn GroupIconGlyph(
    icon: String,
    #[props(default = "h-5 w-5".to_string())] class: String,
) -> Element {
    match icon.as_str() {
        "plane" => rsx! { Icon { icon: LdPlane, class } },
        "luggage" => rsx! { Icon { icon: LdLuggage, class } },
        "tree-palm" => rsx! { Icon { icon: LdTreePalm, class } },
        "mountain" => rsx! { Icon { icon: LdMountain, class } },
        "tent" => rsx! { Icon { icon: LdTent, class } },
        "home" => rsx! { Icon { icon: LdHome, class } },
        "utensils" => rsx! { Icon { icon: LdUtensils, class } },
        "car" => rsx! { Icon { icon: LdCar, class } },
        "briefcase" => rsx! { Icon { icon: LdBriefcase, class } },
        "party-popper" => rsx! { Icon { icon: LdPartyPopper, class } },
        "heart" => rsx! { Icon { icon: LdHeart, class } },
        // "users" and keys this version does not know.
        _ => rsx! { Icon { icon: LdUsers, class } },
    }
}

/// Rounded square with a group's icon on its color; the square shape tells
/// groups apart from the round avatars of people.
#[component]
pub fn GroupIcon(icon: String, color: String, #[props(default)] large: bool) -> Element {
    let (box_size, glyph) = if large {
        ("h-20 w-20 rounded-3xl", "h-10 w-10")
    } else {
        ("h-11 w-11 rounded-2xl", "h-5 w-5")
    };

    rsx! {
        span {
            class: "flex shrink-0 items-center justify-center {box_size} {color_classes(&color)}",
            aria_hidden: "true",
            GroupIconGlyph { icon, class: glyph.to_string() }
        }
    }
}
