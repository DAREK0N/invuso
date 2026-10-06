use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdMoon, LdMoonStar, LdSmartphone, LdSun, LdSunMoon},
};

use crate::appearance::{Appearance, Size, Theme};
use crate::components::{Chip, ErrorBanner, RadioRow, SwitchRow, TopBar};
use crate::storage::Db;

/// `/settings/appearance`: theme, corner radius, UI scale and shadows
/// (UI-18, SET-05). Every choice is saved and shown at once.
#[component]
pub fn SettingsAppearance() -> Element {
    let db = use_context::<Db>();
    let mut look = use_context::<Signal<Appearance>>();
    let mut error = use_signal(|| None::<String>);

    let change = use_callback(move |updated: Appearance| match updated.save(&db) {
        Ok(()) => {
            look.set(updated);
            error.set(None);
        }
        Err(e) => error.set(Some(format!("{} {e}", t!("appearance.save_error")))),
    });
    let current = look();

    rsx! {
        TopBar { title: t!("page.settings_appearance").to_string(), show_back: true }
        div { class: "mx-4 flex flex-col gap-6 pt-4 safe-area-x",
            ErrorBanner { error: error() }
            section { class: "flex flex-col gap-2",
                SectionTitle { title: t!("appearance.theme").to_string() }
                div {
                    class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                    role: "radiogroup",
                    aria_label: t!("appearance.theme").to_string(),
                    for theme in Theme::ALL {
                        RadioRow {
                            key: "{theme.code()}",
                            label: theme.label(),
                            hint: theme_hint(theme),
                            selected: current.theme == theme,
                            onclick: move |_| change.call(Appearance { theme, ..current }),
                            ThemeIcon { theme }
                        }
                    }
                }
            }
            SizeChoice {
                title: t!("appearance.radius").to_string(),
                selected: current.radius,
                on_select: move |radius| change.call(Appearance { radius, ..current }),
            }
            SizeChoice {
                title: t!("appearance.scale").to_string(),
                selected: current.scale,
                on_select: move |scale| change.call(Appearance { scale, ..current }),
            }
            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                SwitchRow {
                    label: t!("appearance.shadows").to_string(),
                    hint: t!("appearance.shadows_hint").to_string(),
                    checked: current.shadows,
                    onchange: move |shadows| change.call(Appearance { shadows, ..current }),
                }
            }
        }
    }
}

#[component]
fn SectionTitle(title: String) -> Element {
    rsx! {
        h2 { class: "px-1 text-xs font-semibold uppercase tracking-wide text-floral-white-400",
            "{title}"
        }
    }
}

/// Small, Normal, Large as a row of chips.
#[component]
fn SizeChoice(title: String, selected: Size, on_select: EventHandler<Size>) -> Element {
    rsx! {
        section { class: "flex flex-col gap-2",
            SectionTitle { title: title.clone() }
            div { class: "flex flex-wrap gap-2", role: "radiogroup", aria_label: title,
                for size in Size::ALL {
                    Chip {
                        key: "{size.code()}",
                        label: size.label(),
                        selected: selected == size,
                        onclick: move |_| on_select.call(size),
                    }
                }
            }
        }
    }
}

#[component]
fn ThemeIcon(theme: Theme) -> Element {
    let class = "h-5 w-5 shrink-0 text-cerulean-300";
    match theme {
        Theme::System => rsx! { Icon { icon: LdSunMoon, class } },
        Theme::Dark => rsx! { Icon { icon: LdMoon, class } },
        Theme::Night => rsx! { Icon { icon: LdMoonStar, class } },
        Theme::Oled => rsx! { Icon { icon: LdSmartphone, class } },
        Theme::Light => rsx! { Icon { icon: LdSun, class } },
    }
}

fn theme_hint(theme: Theme) -> Option<String> {
    match theme {
        Theme::System => Some(t!("appearance.theme_system_hint").to_string()),
        Theme::Oled => Some(t!("appearance.theme_oled_hint").to_string()),
        _ => None,
    }
}
