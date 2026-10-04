use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdCircleAlert, LdTrash2},
};
use invuso_core::domain::{GroupId, GroupMember};

use super::form::GroupNotFound;
use super::{DeleteGroupSheet, group_subtitle};
use crate::Route;
use crate::components::{
    AvatarEntry, AvatarStack, Button, ButtonVariant, EmptyState, GroupIcon, LinkRow, TopBar,
};
use crate::state::DataRevision;
use crate::storage::Db;

/// `/groups/:id`: for now the head of the group (name, members) with links
/// to its expenses (timeline, GRP-20), members and editing; totals and
/// balances follow with GRP-10..14.
#[component]
pub fn GroupOverview(id: String) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let nav = use_navigator();
    let mut confirm_delete = use_signal(|| false);
    let mut delete_error = use_signal(|| None::<String>);

    let group_id = use_memo(use_reactive!(|id| GroupId::new(id)));
    let data = use_memo(move || {
        revision.track();
        let id = group_id();
        let Some(group) = db.group(&id).map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        let members = db.group_members(&id).map_err(|e| e.to_string())?;
        let expense_count = db.group_expense_count(&id).map_err(|e| e.to_string())?;
        Ok::<_, String>(Some((group, members, expense_count)))
    });

    rsx! {
        TopBar { title: t!("page.group_overview").to_string(), show_back: true }
        match &*data.read() {
            Err(message) => rsx! {
                EmptyState {
                    title: t!("group.load_error_title").to_string(),
                    text: message.clone(),
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
            },
            Ok(None) => rsx! { GroupNotFound {} },
            Ok(Some((group, members, expense_count))) => rsx! {
                div { class: "mx-4 flex flex-col gap-4 pt-6 safe-area-x",
                    div { class: "flex flex-col items-center gap-3 text-center",
                        GroupIcon { icon: group.icon.clone(), color: group.color.clone(), large: true }
                        h2 { class: "text-2xl font-semibold break-words text-floral-white-50", "{group.name}" }
                        p { class: "text-sm text-floral-white-400", {group_subtitle(group)} }
                    }
                    div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                        LinkRow {
                            label: t!("group.expenses").to_string(),
                            onclick: {
                                let id = group.id.as_str().to_string();
                                move |_| {
                                    nav.push(Route::GroupTimeline { id: id.clone() });
                                }
                            },
                            span { class: "text-sm tabular-nums text-floral-white-400", "{expense_count}" }
                        }
                        LinkRow {
                            label: t!("page.group_members").to_string(),
                            onclick: {
                                let id = group.id.as_str().to_string();
                                move |_| {
                                    nav.push(Route::GroupMembers {
                                        id: id.clone(),
                                        setup: false,
                                    });
                                }
                            },
                            AvatarStack { people: avatars(members), max: 3 }
                        }
                        LinkRow {
                            label: t!("common.edit").to_string(),
                            onclick: {
                                let id = group.id.as_str().to_string();
                                move |_| {
                                    nav.push(Route::GroupEdit { id: id.clone() });
                                }
                            },
                        }
                    }
                    Button {
                        variant: ButtonVariant::Danger,
                        class: "w-full",
                        onclick: move |_| {
                            delete_error.set(None);
                            confirm_delete.set(true);
                        },
                        Icon { icon: LdTrash2, class: "h-5 w-5" }
                        {t!("group.delete").to_string()}
                    }
                }
                if confirm_delete() {
                    DeleteGroupSheet {
                        group: group.clone(),
                        error: delete_error(),
                        on_deleted: move |_| {
                            confirm_delete.set(false);
                            // The undo toast stays visible on the list.
                            if nav.can_go_back() {
                                nav.go_back();
                            } else {
                                nav.replace(Route::GroupList {});
                            }
                        },
                        on_error: move |message| delete_error.set(Some(message)),
                        on_close: move |_| confirm_delete.set(false),
                    }
                }
            },
        }
    }
}

fn avatars(members: &[GroupMember]) -> Vec<AvatarEntry> {
    members
        .iter()
        .map(|member| AvatarEntry {
            name: member.person.name.clone(),
            color: member.person.color.clone(),
        })
        .collect()
}
