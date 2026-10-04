use dioxus::prelude::*;
use dioxus_free_icons::{Icon, icons::ld_icons::LdChevronRight};

/// Tappable list row with a leading element (`children`, e.g. an avatar),
/// title, optional subtitle and an optional long-press action (UI-09).
///
/// Long-press uses the `contextmenu` event, which the Android WebView fires
/// on a long touch; that is sturdier than our own touch timers and does not
/// fight with scrolling like swipe gestures would.
#[component]
pub fn ListItem(
    title: String,
    #[props(default)] subtitle: Option<String>,
    #[props(default)] badge: Option<String>,
    onclick: EventHandler<()>,
    #[props(default)] on_long_press: Option<EventHandler<()>>,
    children: Element,
) -> Element {
    rsx! {
        button {
            class: "no-callout flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            oncontextmenu: move |event| {
                if let Some(on_long_press) = on_long_press {
                    event.prevent_default();
                    on_long_press.call(());
                }
            },
            {children}
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "flex items-center gap-2",
                    span { class: "truncate text-base text-floral-white-50", "{title}" }
                    if let Some(badge) = badge {
                        span { class: "shrink-0 rounded-full bg-cerulean-800 px-2 py-0.5 text-xs font-medium text-cerulean-200",
                            "{badge}"
                        }
                    }
                }
                if let Some(subtitle) = subtitle {
                    span { class: "truncate text-sm text-floral-white-400", "{subtitle}" }
                }
            }
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
        }
    }
}
