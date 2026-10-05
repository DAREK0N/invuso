use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdChevronRight, LdCircleAlert, LdPencil, LdPin, LdPinOff, LdPlus, LdTrash2, LdUsers,
    },
};
use invuso_core::domain::{Group, Money};

use super::{DeleteGroupSheet, mark_active};
use crate::Route;
use crate::components::{
    AvatarEntry, AvatarStack, BottomSheet, Button, EmptyState, GroupIcon, MenuRow, MoneyText,
    TopBar,
};
use crate::preferences::period_text;
use crate::services::summary::group_summary;
use crate::state::{DataRevision, Toaster};
use crate::storage::{Db, StorageError};

/// A group with its total, the own balance and the avatars of its
/// members, as the list shows it.
#[derive(Debug, Clone, PartialEq)]
struct GroupEntry {
    group: Group,
    members: Vec<AvatarEntry>,
    /// Total spent in the base currency (GRP-10).
    total: Money,
    /// Balance of "Ich", if a member or still part of an expense (PER-02).
    own: Option<Money>,
    /// Marked as the active group (GRP-05).
    active: bool,
}

/// `/groups`: every group with icon, color, total, own balance and member
/// avatars (GRP-01, GRP-02). Tap opens the group, long-press a menu
/// (UI-09); the active group is marked (GRP-05).
#[component]
pub fn GroupList() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let nav = use_navigator();
    let mut menu = use_signal(|| None::<GroupEntry>);
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
                                    entry: entry.clone(),
                                    on_long_press: move |_| menu.set(Some(entry.clone())),
                                }
                            }
                        }
                        p { class: "px-1 text-sm text-floral-white-500", {t!("group.long_press_hint").to_string()} }
                    }
                }
            },
        }
        if let Some(entry) = menu() {
            GroupMenu {
                group: entry.group,
                active: entry.active,
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
    let me = db.me()?.map(|person| person.id);
    let active = db.active_group()?.map(|group| group.id);
    db.groups()?
        .into_iter()
        .map(|group| {
            let summary = group_summary(db, &group)?;
            let own = me
                .as_ref()
                .and_then(|me| summary.people.get(me))
                .map(|totals| Money::new(totals.balance, group.base_currency));
            let members = db
                .group_members(&group.id)?
                .into_iter()
                .map(|member| AvatarEntry {
                    name: member.person.name,
                    color: member.person.color,
                })
                .collect();
            Ok(GroupEntry {
                active: active.as_ref() == Some(&group.id),
                group,
                members,
                total: summary.total,
                own,
            })
        })
        .collect()
}

/// List row: group icon, name, total and period, own balance and member
/// avatars.
#[component]
fn GroupRow(entry: GroupEntry, on_long_press: EventHandler<Group>) -> Element {
    let nav = use_navigator();
    let GroupEntry {
        group,
        members,
        total,
        own,
        active,
    } = entry;
    let period = period_text(group.start_date.as_deref(), group.end_date.as_deref());
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
                span { class: "flex min-w-0 items-center gap-2",
                    span { class: "truncate text-base text-floral-white-50", "{group.name}" }
                    if active {
                        span { class: "flex shrink-0 items-center gap-1 rounded-full bg-cerulean-800 px-2 py-0.5 text-xs font-medium text-cerulean-100",
                            Icon { icon: LdPin, class: "h-3 w-3" }
                            {t!("group.active").to_string()}
                        }
                    }
                }
                span { class: "flex min-w-0 items-center gap-1 text-sm text-floral-white-400",
                    MoneyText { amount: total }
                    if let Some(period) = period {
                        span { class: "truncate", "· {period}" }
                    }
                }
            }
            span { class: "flex shrink-0 flex-col items-end gap-1",
                if let Some(own) = own {
                    MoneyText { amount: own, signed: true, class: "text-sm font-semibold" }
                }
                AvatarStack { people: members, max: 3 }
            }
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
        }
    }
}

/// Long-press menu of a group: mark as active or not (GRP-05), edit,
/// members, delete.
#[component]
fn GroupMenu(
    group: Group,
    active: bool,
    on_delete: EventHandler<Group>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let nav = use_navigator();
    let active_target = group.clone();
    let edit_id = group.id.as_str().to_string();
    let members_id = edit_id.clone();
    let delete_target = group.clone();

    rsx! {
        BottomSheet { title: group.name.clone(), on_close,
            div { class: "flex flex-col gap-1 px-3 pt-2",
                MenuRow {
                    label: if active { t!("group.unmark_active").to_string() } else { t!("group.mark_active").to_string() },
                    onclick: move |_| {
                        on_close.call(());
                        let target = (!active).then_some(&active_target);
                        mark_active(&db, target, revision, toaster);
                    },
                    if active {
                        Icon { icon: LdPinOff, class: "h-5 w-5" }
                    } else {
                        Icon { icon: LdPin, class: "h-5 w-5" }
                    }
                }
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
