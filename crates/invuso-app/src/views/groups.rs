use std::rc::Rc;

use dioxus::prelude::*;
use invuso_core::domain::Group;

use super::PlaceholderPage;
use crate::components::ConfirmSheet;
use crate::preferences::period_text;
use crate::state::{DataRevision, ToastAction, Toaster};
use crate::storage::Db;

mod form;
mod list;
mod members;
mod overview;
mod timeline;

pub use form::{GroupEdit, GroupNew};
pub use list::GroupList;
pub use members::GroupMembers;
pub use overview::GroupOverview;
pub use timeline::GroupTimeline;

#[component]
pub fn GroupSettle(id: String) -> Element {
    rsx! { PlaceholderPage { title: t!("page.group_settle").to_string(), show_back: true } }
}

/// "EUR · 01.03.2026 – 14.03.2026": base currency and, if set, the period.
fn group_subtitle(group: &Group) -> String {
    let mut parts = vec![group.base_currency.code().to_string()];
    if let Some(period) = period_text(group.start_date.as_deref(), group.end_date.as_deref()) {
        parts.push(period);
    }
    parts.join(" · ")
}

/// Confirmation before deleting a group. Deleting is soft and shows a
/// toast with "Undo" (UI-11).
#[component]
fn DeleteGroupSheet(
    group: Group,
    error: Option<String>,
    on_deleted: EventHandler<()>,
    on_error: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let toaster = use_context::<Toaster>();
    let name = group.name.clone();

    rsx! {
        ConfirmSheet {
            title: t!("group.delete_title").to_string(),
            text: t!("group.delete_text", name = name).to_string(),
            confirm_label: t!("common.delete").to_string(),
            error,
            on_confirm: move |_| match delete_with_undo(&db, &group, revision, toaster) {
                Ok(()) => on_deleted.call(()),
                Err(message) => on_error.call(message),
            },
            on_close,
        }
    }
}

/// Soft-deletes the group and offers to restore it from a toast.
fn delete_with_undo(
    db: &Db,
    group: &Group,
    mut revision: DataRevision,
    mut toaster: Toaster,
) -> Result<(), String> {
    db.delete_group(&group.id)
        .map_err(|e| format!("{} {e}", t!("group.delete_error")))?;
    revision.bump();

    let db = db.clone();
    let id = group.id.clone();
    let undo = move || {
        let (mut revision, mut toaster) = (revision, toaster);
        match db.restore_group(&id) {
            Ok(()) => revision.bump(),
            Err(e) => toaster.show(format!("{} {e}", t!("group.restore_error")), None),
        }
    };
    toaster.show(
        t!("group.deleted", name = group.name).to_string(),
        Some(ToastAction {
            label: t!("common.undo").to_string(),
            run: Rc::new(undo),
        }),
    );
    Ok(())
}

/// Marks the group as active (GRP-05), or removes the mark with `None`,
/// and confirms it in a toast.
fn mark_active(db: &Db, group: Option<&Group>, mut revision: DataRevision, mut toaster: Toaster) {
    match db.set_active_group(group.map(|g| &g.id)) {
        Ok(()) => {
            revision.bump();
            let message = match group {
                Some(group) => t!("group.marked_active", name = group.name),
                None => t!("group.unmarked_active"),
            };
            toaster.show(message.to_string(), None);
        }
        Err(e) => toaster.show(format!("{} {e}", t!("group.active_error")), None),
    }
}
