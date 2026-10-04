use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdChevronRight, LdCircleAlert, LdTrash2},
};
use invuso_core::domain::{GroupId, GroupMember, local_date};

use super::form::GroupNotFound;
use super::{DeleteGroupSheet, group_subtitle};
use crate::Route;
use crate::components::{
    AvatarEntry, AvatarStack, Button, ButtonVariant, EmptyState, GroupIcon, LinkRow, MoneyText,
    TopBar,
};
use crate::preferences::display_date;
use crate::state::DataRevision;
use crate::storage::{Db, ExpenseListEntry};

/// `/groups/:id`: for now the head of the group (name, members) with links
/// to members and editing, and a plain list of its expenses to edit them
/// (EXP-05); totals and balances follow with GRP-10..14, the timeline
/// with GRP-20..23.
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
        let expenses = db.group_expenses(&id).map_err(|e| e.to_string())?;
        Ok::<_, String>(Some((group, members, expenses)))
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
            Ok(Some((group, members, expenses))) => rsx! {
                div { class: "mx-4 flex flex-col gap-4 pt-6 safe-area-x",
                    div { class: "flex flex-col items-center gap-3 text-center",
                        GroupIcon { icon: group.icon.clone(), color: group.color.clone(), large: true }
                        h2 { class: "text-2xl font-semibold break-words text-floral-white-50", "{group.name}" }
                        p { class: "text-sm text-floral-white-400", {group_subtitle(group)} }
                    }
                    div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
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
                    section { class: "flex flex-col gap-2",
                        h2 { class: "px-1 text-sm font-medium text-floral-white-300", {t!("group.expenses").to_string()} }
                        if expenses.is_empty() {
                            p { class: "px-1 text-sm text-floral-white-400", {t!("group.no_expenses").to_string()} }
                        } else {
                            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                                for entry in expenses.iter().cloned() {
                                    ExpenseRow {
                                        key: "{entry.id.as_str()}",
                                        onclick: {
                                            let id = entry.id.as_str().to_string();
                                            move |_| {
                                                nav.push(Route::ExpenseEdit { id: id.clone() });
                                            }
                                        },
                                        entry,
                                    }
                                }
                            }
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

/// One expense of the interim list: title and day, amount in the base
/// currency and, if different, in its own currency.
#[component]
fn ExpenseRow(entry: ExpenseListEntry, onclick: EventHandler<()>) -> Element {
    let foreign = entry.total.currency() != entry.total_in_base.currency();

    rsx! {
        button {
            class: "flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "truncate text-base text-floral-white-50", "{entry.title}" }
                span { class: "text-sm text-floral-white-400", {display_date(local_date(&entry.occurred_at))} }
            }
            span { class: "flex shrink-0 flex-col items-end",
                MoneyText { amount: entry.total_in_base, class: "text-base font-semibold text-floral-white-100" }
                if foreign {
                    MoneyText { amount: entry.total, class: "text-sm text-floral-white-400" }
                }
            }
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
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
