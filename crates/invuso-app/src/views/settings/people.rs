use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdCircleAlert, LdPencil, LdTrash2, LdUserPlus, LdUsers},
};
use invuso_core::domain::Person;

use crate::Route;
use crate::components::{
    Avatar, BottomSheet, Button, ColorPicker, ConfirmSheet, EmptyState, ErrorBanner, ListItem,
    TextField, TopBar,
};
use crate::preferences::suggested_person_color;
use crate::state::{DataRevision, ToastAction, Toaster};
use crate::storage::{Db, NewPerson};

/// What the person form sheet is doing.
#[derive(Debug, Clone, PartialEq)]
enum Form {
    New,
    Edit(Person),
}

/// `/settings/people`: everyone who can share expenses, "Ich" first
/// (PER-01, SET-06). Tap opens the detail, long-press a menu (UI-09).
#[component]
pub fn SettingsPeople() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let nav = use_navigator();
    let mut form = use_signal(|| None::<Form>);
    let mut menu = use_signal(|| None::<Person>);
    let mut confirm_delete = use_signal(|| None::<Person>);
    let mut delete_error = use_signal(|| None::<String>);

    let people = use_memo(move || {
        revision.track();
        db.people().map_err(|e| e.to_string())
    });

    rsx! {
        TopBar { title: t!("page.settings_people").to_string(), show_back: true }
        match &*people.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("people.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(people) => rsx! {
                div { class: "mx-4 flex flex-col gap-4 pt-4 safe-area-x",
                    Button { class: "w-full", onclick: move |_| form.set(Some(Form::New)),
                        Icon { icon: LdUserPlus, class: "h-5 w-5" }
                        {t!("people.add").to_string()}
                    }
                    if people.is_empty() {
                        EmptyState {
                            title: t!("people.empty_title").to_string(),
                            text: t!("people.empty_text").to_string(),
                            Icon { icon: LdUsers, class: "h-8 w-8" }
                        }
                    } else {
                        div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                            for person in people.iter().cloned() {
                                PersonRow {
                                    key: "{person.id.as_str()}",
                                    person: person.clone(),
                                    onclick: move |_| {
                                        nav.push(Route::PersonDetail { id: person.id.as_str().to_string() });
                                    },
                                    on_long_press: move |person| menu.set(Some(person)),
                                }
                            }
                        }
                        p { class: "px-1 text-sm text-floral-white-500", {t!("people.long_press_hint").to_string()} }
                    }
                }
            },
        }
        if let Some(person) = menu() {
            PersonMenu {
                person,
                on_edit: move |person| {
                    menu.set(None);
                    form.set(Some(Form::Edit(person)));
                },
                on_delete: move |person| {
                    menu.set(None);
                    delete_error.set(None);
                    confirm_delete.set(Some(person));
                },
                on_close: move |_| menu.set(None),
            }
        }
        if let Some(target) = form() {
            PersonFormSheet {
                person: match target {
                    Form::New => None,
                    Form::Edit(person) => Some(person),
                },
                on_saved: move |_| form.set(None),
                on_close: move |_| form.set(None),
            }
        }
        if let Some(person) = confirm_delete() {
            DeletePersonSheet {
                person,
                error: delete_error(),
                on_deleted: move |_| confirm_delete.set(None),
                on_error: move |message| delete_error.set(Some(message)),
                on_close: move |_| confirm_delete.set(None),
            }
        }
    }
}

#[component]
fn PersonRow(
    person: Person,
    onclick: EventHandler<()>,
    on_long_press: EventHandler<Person>,
) -> Element {
    let subtitle = person
        .note
        .as_deref()
        .and_then(|note| note.lines().next())
        .map(str::to_string);
    let badge = person.is_me.then(|| t!("people.me_badge").to_string());
    let (name, color) = (person.name.clone(), person.color.clone());

    rsx! {
        ListItem {
            title: name.clone(),
            subtitle,
            badge,
            onclick,
            on_long_press: move |_| on_long_press.call(person.clone()),
            Avatar { name, color }
        }
    }
}

/// Long-press menu of a person row: edit, and delete for everyone but "Ich".
#[component]
fn PersonMenu(
    person: Person,
    on_edit: EventHandler<Person>,
    on_delete: EventHandler<Person>,
    on_close: EventHandler<()>,
) -> Element {
    let edit_target = person.clone();
    let delete_target = person.clone();

    rsx! {
        BottomSheet { title: person.name.clone(), on_close,
            div { class: "flex flex-col gap-1 px-3 pt-2",
                MenuRow {
                    label: t!("common.edit").to_string(),
                    onclick: move |_| on_edit.call(edit_target.clone()),
                    Icon { icon: LdPencil, class: "h-5 w-5" }
                }
                if !person.is_me {
                    MenuRow {
                        label: t!("common.delete").to_string(),
                        danger: true,
                        onclick: move |_| on_delete.call(delete_target.clone()),
                        Icon { icon: LdTrash2, class: "h-5 w-5" }
                    }
                }
            }
        }
    }
}

/// One entry of [`PersonMenu`]; `children` is its icon.
#[component]
fn MenuRow(
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

/// Bottom sheet to create (`person: None`) or edit a person: name, color
/// and note (PER-01).
#[component]
pub(super) fn PersonFormSheet(
    person: Option<Person>,
    on_saved: EventHandler<Person>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let mut revision = use_context::<DataRevision>();
    let is_new = person.is_none();

    let initial = person.clone();
    let mut name = use_signal(|| initial.as_ref().map(|p| p.name.clone()).unwrap_or_default());
    let initial = person.clone();
    let suggest_db = db.clone();
    let mut color = use_signal(move || match &initial {
        Some(person) => person.color.clone(),
        None => {
            let people = suggest_db.people().unwrap_or_default();
            suggested_person_color(people.iter().map(|p| p.color.as_str())).to_string()
        }
    });
    let initial = person.clone();
    let mut note = use_signal(|| initial.and_then(|p| p.note).unwrap_or_default());
    let mut name_error = use_signal(|| None::<String>);
    let mut save_error = use_signal(|| None::<String>);

    let title = if is_new {
        t!("people.new_title").to_string()
    } else {
        t!("people.edit_title").to_string()
    };

    let save = move |_| {
        if name.read().trim().is_empty() {
            name_error.set(Some(t!("profile.name_required").to_string()));
            return;
        }
        let note_value = Some(note()).filter(|n| !n.trim().is_empty());
        let result = match &person {
            None => db.create_person(NewPerson {
                name: name(),
                color: color(),
                is_me: false,
                note: note_value,
            }),
            Some(existing) => {
                let updated = Person {
                    name: name().trim().to_string(),
                    color: color(),
                    note: note_value.map(|n| n.trim().to_string()),
                    ..existing.clone()
                };
                db.update_person(&updated).map(|()| updated)
            }
        };
        match result {
            Ok(saved) => {
                revision.bump();
                on_saved.call(saved);
            }
            Err(error) => save_error.set(Some(format!("{} {error}", t!("profile.save_error")))),
        }
    };

    rsx! {
        BottomSheet { title, on_close,
            div { class: "flex max-h-[75vh] flex-col gap-5 overflow-y-auto overscroll-contain px-5 pt-3",
                TextField {
                    id: "person-name",
                    label: t!("profile.name").to_string(),
                    value: name(),
                    placeholder: t!("people.name_placeholder").to_string(),
                    error: name_error(),
                    oninput: move |value| {
                        name.set(value);
                        name_error.set(None);
                    },
                }
                ColorPicker {
                    label: t!("people.color").to_string(),
                    selected: color(),
                    on_select: move |value| color.set(value),
                }
                TextField {
                    id: "person-note",
                    label: t!("people.note").to_string(),
                    value: note(),
                    placeholder: t!("people.note_placeholder").to_string(),
                    multiline: true,
                    oninput: move |value| note.set(value),
                }
                ErrorBanner { error: save_error() }
                Button { class: "w-full", onclick: save, {t!("common.save").to_string()} }
            }
        }
    }
}

/// Confirmation before deleting a person. Deleting is soft and shows a
/// toast with "Undo" (UI-11).
#[component]
pub(super) fn DeletePersonSheet(
    person: Person,
    error: Option<String>,
    on_deleted: EventHandler<()>,
    on_error: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let name = person.name.clone();

    rsx! {
        ConfirmSheet {
            title: t!("people.delete_title").to_string(),
            text: t!("people.delete_text", name = name).to_string(),
            confirm_label: t!("common.delete").to_string(),
            error,
            on_confirm: move |_| match delete_with_undo(&db, &person, revision, toaster) {
                Ok(()) => on_deleted.call(()),
                Err(message) => on_error.call(message),
            },
            on_close,
        }
    }
}

/// Soft-deletes the person and offers to restore them from a toast.
fn delete_with_undo(
    db: &Db,
    person: &Person,
    mut revision: DataRevision,
    mut toaster: Toaster,
) -> Result<(), String> {
    db.delete_person(&person.id)
        .map_err(|e| format!("{} {e}", t!("people.delete_error")))?;
    revision.bump();

    let db = db.clone();
    let id = person.id.clone();
    let undo = move || {
        let (mut revision, mut toaster) = (revision, toaster);
        match db.restore_person(&id) {
            Ok(()) => revision.bump(),
            Err(e) => toaster.show(format!("{} {e}", t!("people.restore_error")), None),
        }
    };
    toaster.show(
        t!("people.deleted", name = person.name).to_string(),
        Some(ToastAction {
            label: t!("common.undo").to_string(),
            run: Rc::new(undo),
        }),
    );
    Ok(())
}
