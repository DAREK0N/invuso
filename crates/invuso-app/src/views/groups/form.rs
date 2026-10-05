use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdChevronRight, LdCircleAlert, LdSearchX},
};
use invuso_core::domain::{Currency, Group, GroupError, GroupId, validate_period};

use crate::Route;
use crate::components::LanguagePicker;
use crate::components::{
    BottomSheet, Button, ColorPicker, CurrencyPicker, DateField, EmptyState, ErrorBanner,
    IconPicker, IconSet, TextField, TopBar,
};
use crate::preferences::{
    DEFAULT_GROUP_ICON, default_home_currency, language_name, suggested_person_color,
};
use crate::state::DataRevision;
use crate::storage::{Db, NewGroup, StorageError};

/// `/groups/new`: creates a group, then continues to its members (GRP-01).
#[component]
pub fn GroupNew() -> Element {
    rsx! {
        TopBar { title: t!("page.group_new").to_string(), show_back: true }
        GroupForm { group: None }
    }
}

/// `/groups/:id/edit`: name, icon, color, base currency, period and target
/// language of a group (GRP-01, TRL-05).
#[component]
pub fn GroupEdit(id: String) -> Element {
    let db = use_context::<Db>();
    // Read once: the form keeps its own state while it is open.
    let group = use_hook(|| db.group(&GroupId::new(id)).map_err(|e| e.to_string()));

    rsx! {
        TopBar { title: t!("page.group_edit").to_string(), show_back: true }
        match group {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("group.load_error_title").to_string(),
                    text: message,
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(None) => rsx! { GroupNotFound {} },
            Ok(Some(group)) => rsx! { GroupForm { group: Some(group) } },
        }
    }
}

/// Shown for a group id that does not exist (any more).
#[component]
pub(super) fn GroupNotFound() -> Element {
    rsx! {
        EmptyState {
            title: t!("group.not_found_title").to_string(),
            text: t!("group.not_found_text").to_string(),
            Icon { icon: LdSearchX, class: "h-8 w-8" }
        }
    }
}

/// Form for a new (`group: None`) or an existing group.
#[component]
fn GroupForm(group: Option<Group>) -> Element {
    let db = use_context::<Db>();
    let mut revision = use_context::<DataRevision>();
    let nav = use_navigator();

    let initial = group.clone();
    let mut name = use_signal(|| initial.map(|g| g.name).unwrap_or_default());
    let initial = group.clone();
    let mut icon = use_signal(|| initial.map_or(DEFAULT_GROUP_ICON.to_string(), |g| g.icon));
    let initial = group.clone();
    let suggest_db = db.clone();
    let mut color = use_signal(move || match initial {
        Some(group) => group.color,
        None => {
            let groups = suggest_db.groups().unwrap_or_default();
            suggested_person_color(groups.iter().map(|g| g.color.as_str())).to_string()
        }
    });
    let initial = group.clone();
    let currency_db = db.clone();
    let mut base_currency = use_signal(move || match initial {
        Some(group) => group.base_currency,
        // Suggest the home currency (AP-08); EUR if the profile is unreadable.
        None => currency_db
            .profile()
            .ok()
            .flatten()
            .map_or_else(default_home_currency, |p| p.home_currency),
    });
    let initial = group.clone();
    let mut start_date = use_signal(|| initial.and_then(|g| g.start_date).unwrap_or_default());
    let initial = group.clone();
    let mut end_date = use_signal(|| initial.and_then(|g| g.end_date).unwrap_or_default());
    let initial = group.clone();
    // `""` follows the global target language (TRL-05).
    let mut target_language =
        use_signal(|| initial.and_then(|g| g.target_language).unwrap_or_default());
    let language_db = db.clone();
    let global_language = use_hook(move || {
        language_db
            .profile()
            .ok()
            .flatten()
            .map(|p| p.target_language)
    });
    let mut picking_currency = use_signal(|| false);
    let mut picking_language = use_signal(|| false);
    let mut name_error = use_signal(|| None::<String>);
    let mut start_error = use_signal(|| None::<String>);
    let mut end_error = use_signal(|| None::<String>);
    let mut save_error = use_signal(|| None::<String>);

    let mut show_period_error = move |error: GroupError| match error {
        GroupError::EndBeforeStart => end_error.set(Some(t!("group.end_before_start").to_string())),
        GroupError::InvalidDate(date) => {
            let message = Some(t!("group.date_invalid").to_string());
            if date == start_date.read().trim() {
                start_error.set(message);
            } else {
                end_error.set(message);
            }
        }
    };

    let save = move |_| {
        let mut valid = true;
        if name.read().trim().is_empty() {
            name_error.set(Some(t!("group.name_required").to_string()));
            valid = false;
        }
        if let Err(error) = validate_period(Some(&start_date()), Some(&end_date())) {
            show_period_error(error);
            valid = false;
        }
        if !valid {
            return;
        }
        let blank_to_none = |date: String| Some(date).filter(|d| !d.trim().is_empty());
        let result = match &group {
            None => db.create_group(NewGroup {
                name: name(),
                icon: icon(),
                color: color(),
                base_currency: base_currency(),
                start_date: blank_to_none(start_date()),
                end_date: blank_to_none(end_date()),
                target_language: Some(target_language()),
            }),
            Some(existing) => {
                let updated = Group {
                    name: name().trim().to_string(),
                    icon: icon(),
                    color: color(),
                    base_currency: base_currency(),
                    start_date: blank_to_none(start_date()).map(|d| d.trim().to_string()),
                    end_date: blank_to_none(end_date()).map(|d| d.trim().to_string()),
                    target_language: Some(target_language()).filter(|l| !l.is_empty()),
                    ..existing.clone()
                };
                db.update_group(&updated).map(|()| updated)
            }
        };
        match result {
            Ok(saved) => {
                revision.bump();
                let id = saved.id.as_str().to_string();
                if group.is_none() {
                    // Adding people is the next step; back then leads to the list.
                    nav.replace(Route::GroupMembers { id, setup: true });
                } else if nav.can_go_back() {
                    nav.go_back();
                } else {
                    nav.replace(Route::GroupOverview { id });
                }
            }
            Err(StorageError::Group(error)) => show_period_error(error),
            Err(error) => save_error.set(Some(format!("{} {error}", t!("profile.save_error")))),
        }
    };

    let currency: Currency = base_currency();
    let end_min = Some(start_date()).filter(|d| !d.is_empty());
    let follow_global = match &global_language {
        Some(code) => t!("group.language_global", language = language_name(code)).to_string(),
        None => t!("group.language_global_unknown").to_string(),
    };
    let language_code = target_language();
    let (shown_code, shown_language) = if language_code.is_empty() {
        ("–".to_string(), follow_global.clone())
    } else {
        (language_code.clone(), language_name(&language_code))
    };

    rsx! {
        div { class: "mx-4 flex flex-col gap-5 pt-4 safe-area-x",
            TextField {
                id: "group-name",
                label: t!("group.name").to_string(),
                value: name(),
                placeholder: t!("group.name_placeholder").to_string(),
                error: name_error(),
                oninput: move |value| {
                    name.set(value);
                    name_error.set(None);
                },
            }
            IconPicker {
                label: t!("group.icon").to_string(),
                selected: icon(),
                color: color(),
                set: IconSet::Group,
                on_select: move |value| icon.set(value),
            }
            ColorPicker {
                label: t!("group.color").to_string(),
                selected: color(),
                on_select: move |value| color.set(value),
            }
            div { class: "flex flex-col gap-2",
                span { class: "text-sm font-medium text-floral-white-300", {t!("group.base_currency").to_string()} }
                button {
                    class: "flex min-h-12 w-full items-center gap-3 rounded-2xl border border-jet-black-700 bg-jet-black-900 px-4 text-left active:bg-jet-black-800 transition-colors ease-apple",
                    r#type: "button",
                    onclick: move |_| picking_currency.set(true),
                    span { class: "w-12 shrink-0 text-sm font-semibold tabular-nums text-cerulean-300", "{currency.code()}" }
                    span { class: "flex-1 truncate text-base text-floral-white-50", "{currency.name()}" }
                    Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
                }
                p { class: "px-1 text-sm text-floral-white-500", {t!("group.base_currency_hint").to_string()} }
            }
            DateField {
                id: "group-start",
                label: t!("group.start_date").to_string(),
                value: start_date(),
                error: start_error(),
                oninput: move |value| {
                    start_date.set(value);
                    start_error.set(None);
                    end_error.set(None);
                },
            }
            DateField {
                id: "group-end",
                label: t!("group.end_date").to_string(),
                value: end_date(),
                min: end_min,
                error: end_error(),
                oninput: move |value| {
                    end_date.set(value);
                    end_error.set(None);
                },
            }
            div { class: "flex flex-col gap-2",
                span { class: "text-sm font-medium text-floral-white-300", {t!("group.target_language").to_string()} }
                button {
                    class: "flex min-h-12 w-full items-center gap-3 rounded-2xl border border-jet-black-700 bg-jet-black-900 px-4 text-left active:bg-jet-black-800 transition-colors ease-apple",
                    r#type: "button",
                    onclick: move |_| picking_language.set(true),
                    span { class: "w-12 shrink-0 text-sm font-semibold text-cerulean-300", "{shown_code}" }
                    span { class: "flex-1 truncate text-base text-floral-white-50", "{shown_language}" }
                    Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
                }
                p { class: "px-1 text-sm text-floral-white-500", {t!("group.target_language_hint").to_string()} }
            }
            ErrorBanner { error: save_error() }
            Button { class: "w-full", onclick: save, {t!("common.save").to_string()} }
        }
        if picking_language() {
            BottomSheet {
                title: t!("group.target_language").to_string(),
                on_close: move |_| picking_language.set(false),
                div { class: "flex max-h-[70vh] flex-col gap-2 overflow-y-auto overscroll-contain px-3 pt-2",
                    LanguagePicker {
                        selected: language_code.clone(),
                        follow_global: Some(follow_global.clone()),
                        on_select: move |code| {
                            target_language.set(code);
                            picking_language.set(false);
                        },
                    }
                }
            }
        }
        if picking_currency() {
            BottomSheet {
                title: t!("group.base_currency").to_string(),
                on_close: move |_| picking_currency.set(false),
                div { class: "flex max-h-[70vh] flex-col gap-2 overflow-y-auto overscroll-contain px-3 pt-2",
                    CurrencyPicker {
                        selected: currency,
                        on_select: move |value| {
                            base_currency.set(value);
                            picking_currency.set(false);
                        },
                    }
                }
            }
        }
    }
}
