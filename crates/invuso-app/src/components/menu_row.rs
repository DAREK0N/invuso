use dioxus::prelude::*;

/// One entry of a long-press menu sheet; `children` is its icon.
#[component]
pub fn MenuRow(
    label: String,
    #[props(default)] danger: bool,
    onclick: EventHandler<()>,
    children: Element,
) -> Element {
    let (icon_colors, text_color) = if danger {
        (
            "bg-watermelon-900 text-watermelon-300",
            "text-watermelon-300",
        )
    } else {
        ("bg-cerulean-800 text-cerulean-200", "text-floral-white-100")
    };

    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-4 rounded-2xl px-3 text-left active:bg-jet-black-800 transition-colors ease-apple {text_color}",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            span { class: "flex h-10 w-10 shrink-0 items-center justify-center rounded-full {icon_colors}",
                {children}
            }
            span { class: "text-base font-medium", "{label}" }
        }
    }
}
