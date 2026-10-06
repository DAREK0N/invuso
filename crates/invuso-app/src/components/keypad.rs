use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdDelete, LdDivide, LdEqual, LdMinus, LdPlus, LdX},
};
use invuso_core::fx::expression::{Key, Operator};

/// Number pad with the four basic operations (FX-06). It replaces the
/// system keyboard, which offers no operators in its number layout.
/// `decimal` is false for currencies without minor unit (JPY), which
/// disables the separator key; `decimal_label` is the app language's one.
#[component]
pub fn Keypad(on_key: EventHandler<Key>, decimal: bool, decimal_label: String) -> Element {
    let digit = move |d: u8| {
        rsx! {
            KeyButton { label: d.to_string(), onclick: move |_| on_key.call(Key::Digit(d)), "{d}" }
        }
    };
    let operator = move |op: Operator| {
        rsx! {
            KeyButton {
                label: operator_label(op),
                kind: KeyKind::Operator,
                onclick: move |_| on_key.call(Key::Operator(op)),
                KeyIcon { glyph: Glyph::Operator(op) }
            }
        }
    };

    rsx! {
        div { class: "grid grid-cols-4 gap-2", role: "group", aria_label: t!("keypad.label").to_string(),
            KeyButton {
                label: t!("keypad.clear").to_string(),
                kind: KeyKind::Function,
                onclick: move |_| on_key.call(Key::Clear),
                "C"
            }
            KeyButton {
                label: t!("keypad.backspace").to_string(),
                kind: KeyKind::Function,
                onclick: move |_| on_key.call(Key::Backspace),
                KeyIcon { glyph: Glyph::Backspace }
            }
            {operator(Operator::Divide)}
            {operator(Operator::Multiply)}
            {digit(7)}
            {digit(8)}
            {digit(9)}
            {operator(Operator::Subtract)}
            {digit(4)}
            {digit(5)}
            {digit(6)}
            {operator(Operator::Add)}
            {digit(1)}
            {digit(2)}
            {digit(3)}
            KeyButton {
                label: t!("keypad.equals").to_string(),
                kind: KeyKind::Equals,
                class: "row-span-2",
                onclick: move |_| on_key.call(Key::Equals),
                KeyIcon { glyph: Glyph::Equals }
            }
            KeyButton {
                label: "0".to_string(),
                class: "col-span-2",
                onclick: move |_| on_key.call(Key::Digit(0)),
                "0"
            }
            KeyButton {
                label: t!("keypad.decimal").to_string(),
                disabled: !decimal,
                onclick: move |_| on_key.call(Key::Decimal),
                "{decimal_label}"
            }
        }
    }
}

fn operator_label(op: Operator) -> String {
    match op {
        Operator::Add => t!("keypad.add"),
        Operator::Subtract => t!("keypad.subtract"),
        Operator::Multiply => t!("keypad.multiply"),
        Operator::Divide => t!("keypad.divide"),
    }
    .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum KeyKind {
    #[default]
    Digit,
    Operator,
    Function,
    Equals,
}

#[component]
fn KeyButton(
    label: String,
    onclick: EventHandler<()>,
    #[props(default)] kind: KeyKind,
    #[props(default)] class: String,
    #[props(default)] disabled: bool,
    children: Element,
) -> Element {
    let colors = match kind {
        KeyKind::Digit => "bg-jet-black-800 text-floral-white-50 active:bg-jet-black-700",
        KeyKind::Operator => "bg-jet-black-900 text-cerulean-200 active:bg-jet-black-800",
        KeyKind::Function => "bg-jet-black-900 text-floral-white-300 active:bg-jet-black-800",
        KeyKind::Equals => "bg-cerulean-600 text-floral-white-50 active:bg-cerulean-700",
    };
    rsx! {
        button {
            class: "flex min-h-12 items-center justify-center rounded-2xl text-2xl font-medium tabular-nums transition ease-apple active:scale-95 disabled:opacity-30 {colors} {class}",
            r#type: "button",
            aria_label: "{label}",
            disabled,
            onclick: move |_| onclick.call(()),
            {children}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Glyph {
    Operator(Operator),
    Equals,
    Backspace,
}

#[component]
fn KeyIcon(glyph: Glyph) -> Element {
    let class = "h-6 w-6";
    match glyph {
        Glyph::Operator(Operator::Add) => rsx! { Icon { icon: LdPlus, class } },
        Glyph::Operator(Operator::Subtract) => rsx! { Icon { icon: LdMinus, class } },
        Glyph::Operator(Operator::Multiply) => rsx! { Icon { icon: LdX, class } },
        Glyph::Operator(Operator::Divide) => rsx! { Icon { icon: LdDivide, class } },
        Glyph::Equals => rsx! { Icon { icon: LdEqual, class } },
        Glyph::Backspace => rsx! { Icon { icon: LdDelete, class } },
    }
}
