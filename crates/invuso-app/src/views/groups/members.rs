use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdCheck, LdCircleAlert, LdUserMinus, LdUserPlus},
};
use invuso_core::domain::{Group, GroupId, GroupMember, Person};

use super::form::GroupNotFound;
use crate::Route;
use crate::components::{
    Avatar, BottomSheet, Button, ButtonVariant, EmptyState, ErrorBanner, GroupIcon, ListItem,
    MenuRow, TopBar,
};
use crate::state::{DataRevision, ToastAction, Toaster};
use crate::storage::Db;
use crate::views::PersonFormSheet;

/// Everything the members page shows.
#[derive(Debug, Clone, PartialEq)]
struct Members {
    group: Group,
    members: Vec<GroupMember>,
    /// People who are not in the group yet, in the global people order.
    candidates: Vec<Person>,
}

/// Which sheet the members page shows.
#[derive(Debug, Clone, PartialEq)]
enum Sheet {
    /// Pick existing people or start creating a new one.
    Add,
    NewPerson,
    Menu(Person),
}

/// `/groups/:id/members`: who is in the group; add existing or new people,
/// remove anyone but "Ich" (GRP-03). Right after creating the group
/// (`setup`), "Done" leads on to the group.
#[component]
pub fn GroupMembers(id: String, setup: bool) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let nav = use_navigator();
    let mut sheet = use_signal(|| None::<Sheet>);
    let mut add_error = use_signal(|| None::<String>);

    let group_id = use_memo(use_reactive!(|id| GroupId::new(id)));
    let load_db = db.clone();
    let data = use_memo(move || {
        revision.track();
        load(&load_db, &group_id()).map_err(|e| e.to_string())
    });

    rsx! {
        TopBar { title: t!("page.group_members").to_string(), show_back: true }
        match &*data.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("group.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(None) => rsx! { GroupNotFound {} },
            Ok(Some(data)) => rsx! {
                div { class: "mx-4 flex flex-col gap-4 pt-4 safe-area-x",
                    div { class: "flex items-center gap-3 px-1",
                        GroupIcon { icon: data.group.icon.clone(), color: data.group.color.clone() }
                        h2 { class: "min-w-0 flex-1 truncate text-lg font-semibold text-floral-white-50", "{data.group.name}" }
                    }
                    Button {
                        variant: if setup { ButtonVariant::Secondary } else { ButtonVariant::Primary },
                        class: "w-full",
                        onclick: move |_| {
                            add_error.set(None);
                            sheet.set(Some(Sheet::Add));
                        },
                        Icon { icon: LdUserPlus, class: "h-5 w-5" }
                        {t!("member.add").to_string()}
                    }
                    div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                        for member in data.members.iter().cloned() {
                            MemberRow {
                                key: "{member.person.id.as_str()}",
                                person: member.person.clone(),
                                onclick: move |_| {
                                    nav.push(Route::PersonDetail { id: member.person.id.as_str().to_string() });
                                },
                                on_long_press: move |person| sheet.set(Some(Sheet::Menu(person))),
                            }
                        }
                    }
                    p { class: "px-1 text-sm text-floral-white-500", {t!("member.long_press_hint").to_string()} }
                    if setup {
                        Button {
                            class: "w-full",
                            onclick: {
                                let id = data.group.id.as_str().to_string();
                                // Replaces this step, so back from the group leads to the list.
                                move |_| {
                                    nav.replace(Route::GroupOverview { id: id.clone() });
                                }
                            },
                            Icon { icon: LdCheck, class: "h-5 w-5" }
                            {t!("member.done").to_string()}
                        }
                    }
                }
                match sheet() {
                    Some(Sheet::Add) => rsx! {
                        AddMemberSheet {
                            candidates: data.candidates.clone(),
                            error: add_error(),
                            on_pick: {
                                let db = db.clone();
                                move |person: Person| add_member(&db, &group_id(), &person, revision, add_error)
                            },
                            on_new_person: move |_| sheet.set(Some(Sheet::NewPerson)),
                            on_close: move |_| sheet.set(None),
                        }
                    },
                    Some(Sheet::NewPerson) => rsx! {
                        PersonFormSheet {
                            person: None,
                            on_saved: {
                                let db = db.clone();
                                move |person: Person| {
                                    add_member(&db, &group_id(), &person, revision, add_error);
                                    sheet.set(Some(Sheet::Add));
                                }
                            },
                            on_close: move |_| sheet.set(Some(Sheet::Add)),
                        }
                    },
                    Some(Sheet::Menu(person)) => rsx! {
                        MemberMenu {
                            group: data.group.clone(),
                            person,
                            on_close: move |_| sheet.set(None),
                        }
                    },
                    None => rsx! {},
                }
            },
        }
    }
}

fn load(db: &Db, id: &GroupId) -> Result<Option<Members>, crate::storage::StorageError> {
    let Some(group) = db.group(id)? else {
        return Ok(None);
    };
    let members = db.group_members(id)?;
    let candidates = db
        .people()?
        .into_iter()
        .filter(|person| !members.iter().any(|m| m.person.id == person.id))
        .collect();
    Ok(Some(Members {
        group,
        members,
        candidates,
    }))
}

/// Adds the person; a failure shows in the add sheet.
fn add_member(
    db: &Db,
    group: &GroupId,
    person: &Person,
    mut revision: DataRevision,
    mut error: Signal<Option<String>>,
) {
    match db.add_group_member(group, &person.id) {
        Ok(()) => {
            error.set(None);
            revision.bump();
        }
        Err(e) => error.set(Some(format!("{} {e}", t!("member.add_error")))),
    }
}

#[component]
fn MemberRow(
    person: Person,
    onclick: EventHandler<()>,
    on_long_press: EventHandler<Person>,
) -> Element {
    let badge = person.is_me.then(|| t!("people.me_badge").to_string());
    let (name, color) = (person.name.clone(), person.color.clone());

    rsx! {
        ListItem {
            title: name.clone(),
            badge,
            onclick,
            on_long_press: move |_| on_long_press.call(person.clone()),
            Avatar { name, color }
        }
    }
}

/// Picks people who are not in the group yet; each tap adds one and the
/// sheet stays open for the next. Also leads to creating a new person.
#[component]
fn AddMemberSheet(
    candidates: Vec<Person>,
    error: Option<String>,
    on_pick: EventHandler<Person>,
    on_new_person: EventHandler<()>,
    on_close: EventHandler<()>,
) -> Element {
    rsx! {
        BottomSheet { title: t!("member.add").to_string(), on_close,
            div { class: "flex max-h-[70vh] flex-col gap-2 overflow-y-auto overscroll-contain px-3 pt-2",
                MenuRow {
                    label: t!("member.new_person").to_string(),
                    onclick: move |_| on_new_person.call(()),
                    Icon { icon: LdUserPlus, class: "h-5 w-5" }
                }
                ErrorBanner { error }
                if candidates.is_empty() {
                    p { class: "px-3 py-4 text-center text-sm text-floral-white-400",
                        {t!("member.all_added").to_string()}
                    }
                } else {
                    div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-950",
                        for person in candidates {
                            CandidateRow { key: "{person.id.as_str()}", person, on_pick }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn CandidateRow(person: Person, on_pick: EventHandler<Person>) -> Element {
    let (name, color) = (person.name.clone(), person.color.clone());
    rsx! {
        button {
            class: "flex min-h-14 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| on_pick.call(person.clone()),
            Avatar { name: name.clone(), color }
            span { class: "min-w-0 flex-1 truncate text-base text-floral-white-50", "{name}" }
            Icon { icon: LdUserPlus, class: "h-5 w-5 shrink-0 text-cerulean-300" }
        }
    }
}

/// Long-press menu of a member: remove, or a note that "Ich" always stays.
#[component]
fn MemberMenu(group: Group, person: Person, on_close: EventHandler<()>) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let mut error = use_signal(|| None::<String>);
    let target = person.clone();

    rsx! {
        BottomSheet { title: person.name.clone(), on_close,
            div { class: "flex flex-col gap-1 px-3 pt-2",
                if person.is_me {
                    p { class: "px-3 py-4 text-center text-sm text-floral-white-400",
                        {t!("member.me_stays").to_string()}
                    }
                } else {
                    ErrorBanner { error: error() }
                    MenuRow {
                        label: t!("member.remove").to_string(),
                        danger: true,
                        onclick: move |_| match remove_with_undo(&db, &group, &target, revision, toaster) {
                            Ok(()) => on_close.call(()),
                            Err(message) => error.set(Some(message)),
                        },
                        Icon { icon: LdUserMinus, class: "h-5 w-5" }
                    }
                }
            }
        }
    }
}

/// Removes the person from the group and offers to add them back from a
/// toast; adding back restores the old membership.
fn remove_with_undo(
    db: &Db,
    group: &Group,
    person: &Person,
    mut revision: DataRevision,
    mut toaster: Toaster,
) -> Result<(), String> {
    db.remove_group_member(&group.id, &person.id)
        .map_err(|e| format!("{} {e}", t!("member.remove_error")))?;
    revision.bump();

    let db = db.clone();
    let (group_id, person_id) = (group.id.clone(), person.id.clone());
    let undo = move || {
        let (mut revision, mut toaster) = (revision, toaster);
        match db.add_group_member(&group_id, &person_id) {
            Ok(()) => revision.bump(),
            Err(e) => toaster.show(format!("{} {e}", t!("member.add_error")), None),
        }
    };
    toaster.show(
        t!("member.removed", name = person.name).to_string(),
        Some(ToastAction {
            label: t!("common.undo").to_string(),
            run: Rc::new(undo),
        }),
    );
    Ok(())
}
