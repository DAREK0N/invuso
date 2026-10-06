use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdChevronRight, LdCircleAlert, LdCircleUser},
};

use crate::Route;
use crate::appearance::{AppLanguage, Appearance, UiLanguage};
use crate::components::{
    AvatarEntry, AvatarStack, BottomSheet, Button, CurrencyPicker, EmptyState, ErrorBanner,
    LanguagePicker, LinkRow, RadioRow, SwitchRow, TextField, TopBar,
};
use crate::preferences::language_name;
use crate::services::receipt_edit;
use crate::state::DataRevision;
use crate::storage::{Db, Profile};

/// Which profile value a bottom sheet is editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Editing {
    Name,
    HomeCurrency,
    TargetLanguage,
}

/// Settings start page: the profile (name of "Ich", home currency, target
/// language; SET-01..03), app language and appearance (SET-04, SET-05) and
/// links to the management subpages (SET-06).
#[component]
pub fn Settings() -> Element {
    let db = use_context::<Db>();
    let mut profile = use_signal(|| db.profile().map_err(|e| e.to_string()));
    let mut editing = use_signal(|| None::<Editing>);
    let mut revision = use_context::<DataRevision>();

    let on_saved = move |saved: Profile| {
        profile.set(Ok(Some(saved)));
        editing.set(None);
        // The name of "Ich" also shows in the people list.
        revision.bump();
    };
    let on_close = move |_| editing.set(None);

    rsx! {
        TopBar { title: t!("page.settings").to_string() }
        match &*profile.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("profile.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            // Unreachable after onboarding; shown rather than an empty page.
            Ok(None) => rsx! {
                EmptyState {
                    title: t!("profile.missing_title").to_string(),
                    text: t!("profile.missing_text").to_string(),
                    Icon { icon: LdCircleUser, class: "h-8 w-8" }
                }
            },
            Ok(Some(current)) => rsx! {
                section { class: "mx-4 flex flex-col gap-2 pt-4 safe-area-x",
                    h2 { class: "px-1 text-xs font-semibold uppercase tracking-wide text-floral-white-400",
                        {t!("profile.section").to_string()}
                    }
                    div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                        SettingsRow {
                            label: t!("profile.name").to_string(),
                            value: current.name.clone(),
                            onclick: move |_| editing.set(Some(Editing::Name)),
                        }
                        SettingsRow {
                            label: t!("profile.home_currency").to_string(),
                            value: format!("{} · {}", current.home_currency.code(), current.home_currency.name()),
                            onclick: move |_| editing.set(Some(Editing::HomeCurrency)),
                        }
                        SettingsRow {
                            label: t!("profile.target_language").to_string(),
                            value: language_name(&current.target_language),
                            onclick: move |_| editing.set(Some(Editing::TargetLanguage)),
                        }
                    }
                }
                match editing() {
                    Some(Editing::Name) => rsx! {
                        NameSheet { profile: current.clone(), on_saved, on_close }
                    },
                    Some(Editing::HomeCurrency) => rsx! {
                        HomeCurrencySheet { profile: current.clone(), on_saved, on_close }
                    },
                    Some(Editing::TargetLanguage) => rsx! {
                        TargetLanguageSheet { profile: current.clone(), on_saved, on_close }
                    },
                    None => rsx! {},
                }
            },
        }
        AppSection {}
        ManageSection {}
    }
}

/// App language (SET-04) and the link to the appearance page (SET-05).
#[component]
fn AppSection() -> Element {
    let db = use_context::<Db>();
    let look = use_context::<Signal<Appearance>>();
    let nav = use_navigator();
    let language = use_signal(|| AppLanguage::load(&db));
    let mut choosing = use_signal(|| false);

    rsx! {
        section { class: "mx-4 flex flex-col gap-2 pt-6 safe-area-x",
            h2 { class: "px-1 text-xs font-semibold uppercase tracking-wide text-floral-white-400",
                {t!("settings.app_section").to_string()}
            }
            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                SettingsRow {
                    label: t!("settings.app_language").to_string(),
                    value: language().label(),
                    onclick: move |_| choosing.set(true),
                }
                LinkRow {
                    label: t!("page.settings_appearance").to_string(),
                    onclick: move |_| {
                        nav.push(Route::SettingsAppearance {});
                    },
                    span { class: "text-base text-floral-white-400", {look().theme.label()} }
                }
            }
        }
        if choosing() {
            AppLanguageSheet { language, on_close: move |_| choosing.set(false) }
        }
    }
}

/// Picks the app language; a different locale rebuilds the screens on this
/// page.
#[component]
fn AppLanguageSheet(mut language: Signal<AppLanguage>, on_close: EventHandler<()>) -> Element {
    let db = use_context::<Db>();
    let mut ui_language = use_context::<UiLanguage>();
    let mut error = use_signal(|| None::<String>);

    rsx! {
        BottomSheet { title: t!("settings.app_language").to_string(), on_close,
            div { class: "flex flex-col gap-2 px-3 pt-2",
                ErrorBanner { error: error() }
                div {
                    class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                    role: "radiogroup",
                    aria_label: t!("settings.app_language").to_string(),
                    for option in AppLanguage::ALL {
                        RadioRow {
                            key: "{option.code()}",
                            label: option.label(),
                            hint: (option == AppLanguage::System)
                                .then(|| t!("app_language.system_hint").to_string()),
                            selected: language() == option,
                            onclick: {
                                let db = db.clone();
                                move |_| match ui_language.switch(&db, option, Route::Settings {}) {
                                    Ok(()) => {
                                        language.set(option);
                                        on_close.call(());
                                    }
                                    Err(e) => {
                                        error.set(Some(format!("{} {e}", t!("app_language.save_error"))));
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

/// Links to the management subpages (SET-06): people, payment methods,
/// categories.
#[component]
fn ManageSection() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let nav = use_navigator();
    let people_db = db.clone();
    // The subpages show load errors; here the rows just stay bare.
    let people = use_memo(move || {
        revision.track();
        people_db
            .people()
            .map(|people| {
                people
                    .into_iter()
                    .map(|person| AvatarEntry {
                        name: person.name,
                        color: person.color,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let methods_db = db.clone();
    let active_methods = use_memo(move || {
        revision.track();
        methods_db
            .payment_methods()
            .map(|methods| methods.iter().filter(|m| !m.archived).count())
            .unwrap_or_default()
    });
    let categories = use_memo(move || {
        revision.track();
        db.categories()
            .map(|categories| categories.len())
            .unwrap_or_default()
    });

    rsx! {
        section { class: "mx-4 flex flex-col gap-2 pt-6 safe-area-x",
            h2 { class: "px-1 text-xs font-semibold uppercase tracking-wide text-floral-white-400",
                {t!("settings.manage_section").to_string()}
            }
            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                LinkRow {
                    label: t!("page.settings_people").to_string(),
                    onclick: move |_| {
                        nav.push(Route::SettingsPeople {});
                    },
                    AvatarStack { people: people() }
                }
                LinkRow {
                    label: t!("page.settings_payment_methods").to_string(),
                    onclick: move |_| {
                        nav.push(Route::SettingsPaymentMethods {});
                    },
                    if active_methods() > 0 {
                        span { class: "text-base tabular-nums text-floral-white-400", "{active_methods}" }
                    }
                }
                LinkRow {
                    label: t!("page.settings_categories").to_string(),
                    onclick: move |_| {
                        nav.push(Route::SettingsCategories {});
                    },
                    if categories() > 0 {
                        span { class: "text-base tabular-nums text-floral-white-400", "{categories}" }
                    }
                }
                LinkRow {
                    label: t!("page.settings_languages").to_string(),
                    onclick: move |_| {
                        nav.push(Route::SettingsLanguages {});
                    },
                }
            }
        }
        ReceiptSection {}
        section { class: "mx-4 flex flex-col gap-2 pt-6 safe-area-x",
            h2 { class: "px-1 text-xs font-semibold uppercase tracking-wide text-floral-white-400",
                {t!("settings.data_section").to_string()}
            }
            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                LinkRow {
                    label: t!("page.settings_data").to_string(),
                    onclick: move |_| {
                        nav.push(Route::SettingsData {});
                    },
                }
            }
        }
        section { class: "mx-4 flex flex-col gap-2 pt-6 safe-area-x",
            h2 { class: "px-1 text-xs font-semibold uppercase tracking-wide text-floral-white-400",
                {t!("settings.about_section").to_string()}
            }
            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                LinkRow {
                    label: t!("page.settings_licenses").to_string(),
                    onclick: move |_| {
                        nav.push(Route::SettingsLicenses {});
                    },
                    span { class: "text-base tabular-nums text-floral-white-400", {env!("CARGO_PKG_VERSION")} }
                }
            }
        }
    }
}

/// Tappable row showing a setting and its current value.
#[component]
fn SettingsRow(label: String, value: String, onclick: EventHandler<()>) -> Element {
    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "text-sm text-floral-white-400", "{label}" }
                span { class: "truncate text-base text-floral-white-50", "{value}" }
            }
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
        }
    }
}

/// Saves the edited profile; the error text is shown in the open sheet.
fn save(db: &Db, profile: Profile, on_saved: EventHandler<Profile>) -> Result<(), String> {
    db.save_profile(&profile)
        .map_err(|e| format!("{} {e}", t!("profile.save_error")))?;
    on_saved.call(profile);
    Ok(())
}

#[component]
fn NameSheet(
    profile: Profile,
    on_saved: EventHandler<Profile>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let mut name = use_signal(|| profile.name.clone());
    let mut error = use_signal(|| None::<String>);

    rsx! {
        BottomSheet { title: t!("profile.name").to_string(), on_close,
            div { class: "flex flex-col gap-4 px-5 pt-3",
                TextField {
                    id: "profile-name",
                    label: t!("profile.name").to_string(),
                    value: name(),
                    placeholder: t!("profile.name_placeholder").to_string(),
                    error: error(),
                    oninput: move |value| {
                        name.set(value);
                        error.set(None);
                    },
                }
                Button {
                    class: "w-full",
                    onclick: move |_| {
                        if name.read().trim().is_empty() {
                            error.set(Some(t!("profile.name_required").to_string()));
                            return;
                        }
                        let updated = Profile { name: name().trim().to_string(), ..profile.clone() };
                        if let Err(message) = save(&db, updated, on_saved) {
                            error.set(Some(message));
                        }
                    },
                    {t!("common.save").to_string()}
                }
            }
        }
    }
}

#[component]
fn HomeCurrencySheet(
    profile: Profile,
    on_saved: EventHandler<Profile>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let mut error = use_signal(|| None::<String>);

    rsx! {
        BottomSheet { title: t!("profile.home_currency").to_string(), on_close,
            div { class: "flex max-h-[70vh] flex-col gap-2 overflow-y-auto overscroll-contain px-3 pt-2",
                ErrorBanner { error: error() }
                CurrencyPicker {
                    selected: profile.home_currency,
                    on_select: move |home_currency| {
                        let updated = Profile { home_currency, ..profile.clone() };
                        if let Err(message) = save(&db, updated, on_saved) {
                            error.set(Some(message));
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn TargetLanguageSheet(
    profile: Profile,
    on_saved: EventHandler<Profile>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let mut error = use_signal(|| None::<String>);

    rsx! {
        BottomSheet { title: t!("profile.target_language").to_string(), on_close,
            div { class: "flex max-h-[70vh] flex-col gap-2 overflow-y-auto overscroll-contain px-3 pt-2",
                ErrorBanner { error: error() }
                LanguagePicker {
                    selected: profile.target_language.clone(),
                    on_select: move |target_language| {
                        let updated = Profile { target_language, ..profile.clone() };
                        if let Err(message) = save(&db, updated, on_saved) {
                            error.set(Some(message));
                        }
                    },
                }
            }
        }
    }
}

/// Receipt settings: suggest the corners of a new photo (RCP-05).
#[component]
fn ReceiptSection() -> Element {
    let db = use_context::<Db>();
    let mut auto = use_signal({
        let db = db.clone();
        move || receipt_edit::auto_corners(&db).map_err(|e| e.to_string())
    });
    let mut error = use_signal(|| None::<String>);

    rsx! {
        section { class: "mx-4 flex flex-col gap-2 pt-6 safe-area-x",
            h2 { class: "px-1 text-xs font-semibold uppercase tracking-wide text-floral-white-400",
                {t!("settings.receipt_section").to_string()}
            }
            match auto() {
                Ok(on) => rsx! {
                    div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                        SwitchRow {
                            label: t!("settings.auto_corners").to_string(),
                            hint: t!("settings.auto_corners_hint").to_string(),
                            checked: on,
                            onchange: move |on: bool| match receipt_edit::set_auto_corners(&db, on) {
                                Ok(()) => {
                                    error.set(None);
                                    auto.set(Ok(on));
                                }
                                Err(e) => error.set(Some(e.to_string())),
                            },
                        }
                    }
                },
                Err(message) => rsx! { ErrorBanner { error: Some(message) } },
            }
            ErrorBanner { error: error() }
        }
    }
}
