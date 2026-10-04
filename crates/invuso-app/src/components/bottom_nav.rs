use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdArrowLeftRight, LdHome, LdPlus, LdSettings, LdUsers},
};

use crate::Route;
use crate::components::AddActionSheet;

/// Floating glass tab bar (UI-01, idee.md 3.3). Left to right: Home, groups,
/// add expense (raised circle opening the action sheet), converter, settings.
#[component]
pub fn BottomNav() -> Element {
    let route = use_route::<Route>();
    let mut sheet_open = use_signal(|| false);

    let home_active = matches!(route, Route::Home {});
    let groups_active = matches!(
        route,
        Route::GroupList {}
            | Route::GroupNew {}
            | Route::GroupOverview { .. }
            | Route::GroupTimeline { .. }
            | Route::GroupMembers { .. }
            | Route::GroupSettle { .. }
            | Route::GroupEdit { .. }
            | Route::ExpenseDetail { .. }
    );
    let converter_active = matches!(route, Route::Converter {} | Route::RateHistory {});
    let settings_active = matches!(
        route,
        Route::Settings {}
            | Route::SettingsPeople {}
            | Route::PersonDetail { .. }
            | Route::SettingsPaymentMethods {}
            | Route::SettingsCategories {}
            | Route::SettingsAppearance {}
            | Route::SettingsLanguages {}
            | Route::SettingsData {}
    );

    rsx! {
        nav {
            class: "fixed app-chrome-bottom left-2 right-2 z-[1000] h-16 rounded-2xl glass shadow-xl safe-area-x",
            aria_label: t!("nav.label").to_string(),
            div { class: "grid h-full grid-cols-5 items-center",
                TabLink { to: Route::Home {}, label: t!("nav.home").to_string(), active: home_active,
                    Icon { icon: LdHome, class: "h-7 w-7" }
                }
                TabLink { to: Route::GroupList {}, label: t!("nav.groups").to_string(), active: groups_active,
                    Icon { icon: LdUsers, class: "h-7 w-7" }
                }
                div { class: "flex justify-center",
                    button {
                        class: "-mt-8 flex h-16 w-16 items-center justify-center rounded-full bg-cerulean-600 text-floral-white-50 shadow-lg shadow-black/40 ring-4 ring-jet-black-950 active:bg-cerulean-700 active:scale-95 transition ease-apple",
                        r#type: "button",
                        aria_label: t!("nav.add").to_string(),
                        onclick: move |_| sheet_open.set(true),
                        Icon { icon: LdPlus, class: "h-8 w-8" }
                    }
                }
                TabLink { to: Route::Converter {}, label: t!("nav.converter").to_string(), active: converter_active,
                    Icon { icon: LdArrowLeftRight, class: "h-7 w-7" }
                }
                TabLink { to: Route::Settings {}, label: t!("nav.settings").to_string(), active: settings_active,
                    Icon { icon: LdSettings, class: "h-7 w-7" }
                }
            }
        }
        if sheet_open() {
            AddActionSheet { on_close: move |_| sheet_open.set(false) }
        }
    }
}

/// One tab of the bar; `children` is its icon.
#[component]
fn TabLink(to: Route, label: String, active: bool, children: Element) -> Element {
    let color = if active {
        "text-floral-white-50"
    } else {
        "text-floral-white-400 active:text-cerulean-300"
    };

    rsx! {
        Link {
            class: "flex h-full items-center justify-center transition-colors ease-apple {color}",
            to,
            aria_label: label,
            aria_current: if active { "page" },
            {children}
        }
    }
}
