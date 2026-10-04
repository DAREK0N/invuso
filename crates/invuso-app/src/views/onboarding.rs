use dioxus::prelude::*;

use crate::Route;
use crate::components::{Button, ButtonVariant, CurrencyPicker, LanguagePicker, TextField, TopBar};
use crate::platform;
use crate::preferences::{default_home_currency, suggested_target_language};
use crate::storage::{Db, Profile};

const STEP_COUNT: usize = 3;

/// First start (idee.md 7.5, steps 1–2): name of "Ich", home currency and
/// target language. Saving creates "Ich" and replaces this screen with Home,
/// so back never returns here.
#[component]
pub fn Onboarding() -> Element {
    let db = use_context::<Db>();
    let nav = use_navigator();

    let mut step = use_signal(|| 0_usize);
    let mut name = use_signal(String::new);
    let mut name_error = use_signal(|| None::<String>);
    let mut home_currency = use_signal(default_home_currency);
    let mut target_language =
        use_signal(|| suggested_target_language(platform::system_locale().as_deref()).to_string());
    let mut save_error = use_signal(|| None::<String>);

    let current = step();
    let is_last = current + 1 == STEP_COUNT;
    let (title, text) = match current {
        0 => (t!("onboarding.name_title"), t!("onboarding.name_text")),
        1 => (
            t!("onboarding.currency_title"),
            t!("onboarding.currency_text"),
        ),
        _ => (
            t!("onboarding.language_title"),
            t!("onboarding.language_text"),
        ),
    };

    let next = move |_| {
        if current == 0 && name.read().trim().is_empty() {
            name_error.set(Some(t!("profile.name_required").to_string()));
            return;
        }
        if !is_last {
            step.set(current + 1);
            return;
        }
        let profile = Profile {
            name: name(),
            home_currency: home_currency(),
            target_language: target_language(),
        };
        match db.save_profile(&profile) {
            Ok(()) => {
                nav.replace(Route::Home {});
            }
            Err(error) => save_error.set(Some(error.to_string())),
        }
    };

    rsx! {
        TopBar { title: t!("page.onboarding").to_string() }
        div { class: "mx-4 flex flex-col gap-5 pt-4 pb-28 safe-area-x",
            div { class: "flex flex-col gap-3",
                StepDots { current }
                p { class: "text-sm text-floral-white-400",
                    {t!("onboarding.step", current = current + 1, total = STEP_COUNT).to_string()}
                }
                h2 { class: "text-2xl font-semibold text-floral-white-50", "{title}" }
                p { class: "text-base text-floral-white-300", "{text}" }
            }
            match current {
                0 => rsx! {
                    TextField {
                        id: "onboarding-name",
                        label: t!("profile.name").to_string(),
                        value: name(),
                        placeholder: t!("profile.name_placeholder").to_string(),
                        error: name_error(),
                        oninput: move |value| {
                            name.set(value);
                            name_error.set(None);
                        },
                    }
                },
                1 => rsx! {
                    CurrencyPicker {
                        selected: home_currency(),
                        on_select: move |currency| home_currency.set(currency),
                    }
                },
                _ => rsx! {
                    LanguagePicker {
                        selected: target_language(),
                        on_select: move |code| target_language.set(code),
                    }
                },
            }
            if let Some(error) = save_error() {
                p { class: "rounded-2xl bg-watermelon-900 px-4 py-3 text-sm text-watermelon-200", role: "alert",
                    "{t!(\"profile.save_error\")} {error}"
                }
            }
        }
        div {
            class: "fixed inset-x-0 bottom-0 z-40 border-t border-jet-black-800 bg-jet-black-950 pt-3 safe-area-x",
            style: "padding-bottom: calc(0.75rem + var(--safe-area-bottom));",
            div { class: "mx-4 flex gap-3",
                if current > 0 {
                    Button {
                        variant: ButtonVariant::Secondary,
                        class: "flex-1",
                        onclick: move |_| step.set(current - 1),
                        {t!("common.back").to_string()}
                    }
                }
                Button { class: "flex-1", onclick: next,
                    if is_last {
                        {t!("onboarding.finish").to_string()}
                    } else {
                        {t!("common.next").to_string()}
                    }
                }
            }
        }
    }
}

/// Progress indicator: one dot per step, the current one wide.
#[component]
fn StepDots(current: usize) -> Element {
    rsx! {
        div { class: "flex gap-2", aria_hidden: "true",
            for index in 0..STEP_COUNT {
                div {
                    key: "{index}",
                    class: "h-1.5 rounded-full transition-all ease-apple",
                    class: if index == current { "w-8 bg-cerulean-400" } else { "w-4 bg-jet-black-700" },
                }
            }
        }
    }
}
