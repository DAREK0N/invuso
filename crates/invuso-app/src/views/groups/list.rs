use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdChevronRight, LdCircleAlert, LdPencil, LdPlus, LdTrash2, LdUsers},
};
use invuso_core::domain::Group;

use super::{DeleteGroupSheet, group_subtitle};
use crate::Route;
use crate::components::{
    AvatarEntry, AvatarStack, BottomSheet, Button, EmptyState, GroupIcon, MenuRow, TopBar,
};
use crate::state::DataRevision;
use crate::storage::{Db, StorageError};

/// A group with the avatars of its members, as the list shows it.
#[derive(Debug, Clone, PartialEq)]
struct GroupEntry {
    group: Group,
    members: Vec<AvatarEntry>,
}

/// `/groups`: every group with icon, color and member avatars (GRP-01,
/// GRP-02 without totals and balance). Tap opens the group, long-press a
/// menu (UI-09).
#[component]
pub fn GroupList() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let nav = use_navigator();
    let mut menu = use_signal(|| None::<Group>);
    let mut confirm_delete = use_signal(|| None::<Group>);
    let mut delete_error = use_signal(|| None::<String>);

    let entries = use_memo(move || {
        revision.track();
        load(&db).map_err(|e| e.to_string())
    });

    rsx! {
        TopBar { title: t!("page.groups").to_string() }
        match &*entries.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("group.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(entries) => rsx! {
                div { class: "mx-4 flex flex-col gap-4 pt-4 safe-area-x",
                    Button {
                        class: "w-full",
                        onclick: move |_| {
                            nav.push(Route::GroupNew {});
                        },
                        Icon { icon: LdPlus, class: "h-5 w-5" }
                        {t!("group.add").to_string()}
                    }
                    if entries.is_empty() {
                        EmptyState {
                            title: t!("group.empty_title").to_string(),
                            text: t!("group.empty_text").to_string(),
                            Icon { icon: LdUsers, class: "h-8 w-8" }
                        }
                    } else {
                        div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                            for entry in entries.iter().cloned() {
                                GroupRow {
                                    key: "{entry.group.id.as_str()}",
                                    entry,
                                    on_long_press: move |group| menu.set(Some(group)),
                                }
                            }
                        }
                        p { class: "px-1 text-sm text-floral-white-500", {t!("group.long_press_hint").to_string()} }
                    }
                }
            },
        }
        if let Some(group) = menu() {
            GroupMenu {
                group,
                on_delete: move |group| {
                    menu.set(None);
                    delete_error.set(None);
                    confirm_delete.set(Some(group));
                },
                on_close: move |_| menu.set(None),
            }
        }
        if let Some(group) = confirm_delete() {
            DeleteGroupSheet {
                group,
                error: delete_error(),
                on_deleted: move |_| confirm_delete.set(None),
                on_error: move |message| delete_error.set(Some(message)),
                on_close: move |_| confirm_delete.set(None),
            }
        }
    }
}

fn load(db: &Db) -> Result<Vec<GroupEntry>, StorageError> {
    db.groups()?
        .into_iter()
        .map(|group| {
            let members = db
                .group_members(&group.id)?
                .into_iter()
                .map(|member| AvatarEntry {
                    name: member.person.name,
                    color: member.person.color,
                })
                .collect();
            Ok(GroupEntry { group, members })
        })
        .collect()
}

/// List row: group icon, name, base currency and period, member avatars.
#[component]
fn GroupRow(entry: GroupEntry, on_long_press: EventHandler<Group>) -> Element {
    let nav = use_navigator();
    let GroupEntry { group, members } = entry;
    let subtitle = group_subtitle(&group);
    let id = group.id.as_str().to_string();

    rsx! {
        button {
            class: "no-callout flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| {
                nav.push(Route::GroupOverview { id: id.clone() });
            },
            oncontextmenu: {
                let group = group.clone();
                move |event: Event<MouseData>| {
                    event.prevent_default();
                    on_long_press.call(group.clone());
                }
            },
            GroupIcon { icon: group.icon.clone(), color: group.color.clone() }
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "truncate text-base text-floral-white-50", "{group.name}" }
                span { class: "truncate text-sm text-floral-white-400", "{subtitle}" }
            }
            AvatarStack { people: members, max: 3 }
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
        }
    }
}

/// Long-press menu of a group: edit, members, delete.
#[component]
fn GroupMenu(group: Group, on_delete: EventHandler<Group>, on_close: EventHandler<()>) -> Element {
    let nav = use_navigator();
    let edit_id = group.id.as_str().to_string();
    let members_id = edit_id.clone();
    let delete_target = group.clone();

    rsx! {
        BottomSheet { title: group.name.clone(), on_close,
            div { class: "flex flex-col gap-1 px-3 pt-2",
                MenuRow {
                    label: t!("common.edit").to_string(),
                    onclick: move |_| {
                        on_close.call(());
                        nav.push(Route::GroupEdit { id: edit_id.clone() });
                    },
                    Icon { icon: LdPencil, class: "h-5 w-5" }
                }
                MenuRow {
                    label: t!("page.group_members").to_string(),
                    onclick: move |_| {
                        on_close.call(());
                        nav.push(Route::GroupMembers {
                            id: members_id.clone(),
                            setup: false,
                        });
                    },
                    Icon { icon: LdUsers, class: "h-5 w-5" }
                }
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
