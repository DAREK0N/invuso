use dioxus::prelude::*;

/// Visual weight of a [`Button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// The one main action of a screen (accent color).
    #[default]
    Primary,
    /// Secondary actions next to a primary one.
    Secondary,
}

/// Large, touch-friendly button (UI-03, so far Primary and Secondary).
/// `class` adds layout classes such as `w-full` or `flex-1`.
#[component]
pub fn Button(
    #[props(default)] variant: ButtonVariant,
    #[props(default)] disabled: bool,
    #[props(default)] class: String,
    onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let colors = match variant {
        ButtonVariant::Primary => "bg-cerulean-600 text-floral-white-50 active:bg-cerulean-700",
        ButtonVariant::Secondary => {
            "bg-jet-black-800 text-floral-white-100 active:bg-jet-black-700"
        }
    };

    rsx! {
        button {
            class: "flex min-h-12 items-center justify-center gap-2 rounded-2xl px-5 text-base font-semibold transition ease-apple active:scale-[0.98] disabled:opacity-50 {colors} {class}",
            r#type: "button",
            disabled,
            onclick: move |event| onclick.call(event),
            {children}
        }
    }
}
