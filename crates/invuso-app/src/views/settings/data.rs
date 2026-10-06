use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdArchive, LdArchiveRestore, LdFileSpreadsheet, LdUsers},
};
use invuso_core::domain::{Group, GroupId};

use crate::Route;
use crate::appearance::{AppLanguage, Appearance, UiLanguage};
use crate::clock;
use crate::components::{ConfirmSheet, EmptyState, ErrorBanner, GroupIcon, TopBar};
use crate::services::backup::{self, BackupError};
use crate::services::export::{CsvStyle, csv_file_name, group_csv};
use crate::state::{DataRevision, Toaster};
use crate::storage::Db;

/// What is running; one thing at a time, since each opens a file dialog.
#[derive(Debug, Clone, PartialEq)]
enum Busy {
    Backup,
    Restore,
    Export(GroupId),
}

/// `/settings/data`: full backup as a ZIP file, restoring it (DATA-01,
/// DATA-02) and a group as CSV (DATA-03).
#[component]
pub fn SettingsData() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let mut toaster = use_context::<Toaster>();
    let mut look = use_context::<Signal<Appearance>>();
    let mut ui_language = use_context::<UiLanguage>();
    let mut busy = use_signal(|| None::<Busy>);
    let mut error = use_signal(|| None::<String>);
    let mut confirming = use_signal(|| false);
    let supported = backup::supported();

    let groups_db = db.clone();
    let groups = use_memo(move || {
        revision.track();
        groups_db.groups().map_err(|e| e.to_string())
    });

    let start = move |job: Busy| begin(busy, error, supported, job);

    let backup_db = db.clone();
    let create_backup = move |_| {
        if !start(Busy::Backup) {
            return;
        }
        let db = backup_db.clone();
        spawn(async move {
            match backup::save_backup(db).await {
                Ok(true) => toaster.show(t!("data.backup_saved").to_string(), None),
                Ok(false) => {}
                Err(e) => error.set(Some(format!("{} {e}", t!("data.backup_error")))),
            }
            busy.set(None);
        });
    };

    let restore_db = db.clone();
    let restore = move |_| {
        confirming.set(false);
        if !start(Busy::Restore) {
            return;
        }
        let db = restore_db.clone();
        spawn(async move {
            match backup::restore_backup(db.clone()).await {
                Ok(true) => {
                    // Everything changed: settings above the router are
                    // read again, screens reload through the revision.
                    look.set(Appearance::load(&db));
                    let language = AppLanguage::load(&db);
                    if let Err(e) = ui_language.switch(&db, language, Route::SettingsData {}) {
                        warn!("applying the restored app language failed: {e}");
                    }
                    let mut revision = revision;
                    revision.bump();
                    toaster.show(t!("data.restored").to_string(), None);
                }
                Ok(false) => {}
                Err(e) => error.set(Some(restore_error_text(&e))),
            }
            busy.set(None);
        });
    };

    let export_db = db.clone();
    let export = use_callback(move |group: Group| {
        if !start(Busy::Export(group.id.clone())) {
            return;
        }
        let db = export_db.clone();
        spawn(async move {
            let saved = match group_csv(&db, &group, CsvStyle::current()) {
                Ok(csv) => {
                    let name = csv_file_name(&group, &clock::local_now().0);
                    backup::save_text(name, "text/csv", csv)
                        .await
                        .map_err(|e| e.to_string())
                }
                Err(e) => Err(e.to_string()),
            };
            match saved {
                Ok(true) => toaster.show(t!("data.export_saved").to_string(), None),
                Ok(false) => {}
                Err(e) => error.set(Some(format!("{} {e}", t!("data.export_error")))),
            }
            busy.set(None);
        });
    });

    let running = busy();
    rsx! {
        TopBar { title: t!("page.settings_data").to_string(), show_back: true }
        div { class: "mx-4 flex flex-col gap-2 pt-4 safe-area-x",
            ErrorBanner { error: error() }
        }
        section { class: "mx-4 flex flex-col gap-2 pt-2 safe-area-x",
            h2 { class: "px-1 text-xs font-semibold uppercase tracking-wide text-floral-white-400",
                {t!("data.backup_section").to_string()}
            }
            div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                ActionRow {
                    label: t!("data.backup_create").to_string(),
                    hint: t!("data.backup_create_hint").to_string(),
                    working: (running == Some(Busy::Backup)).then(|| t!("data.working_backup").to_string()),
                    disabled: running.is_some(),
                    onclick: create_backup,
                    Icon { icon: LdArchive, class: "h-5 w-5" }
                }
                ActionRow {
                    label: t!("data.backup_restore").to_string(),
                    hint: t!("data.backup_restore_hint").to_string(),
                    working: (running == Some(Busy::Restore)).then(|| t!("data.working_restore").to_string()),
                    disabled: running.is_some(),
                    onclick: move |_| confirming.set(true),
                    Icon { icon: LdArchiveRestore, class: "h-5 w-5" }
                }
            }
        }
        section { class: "mx-4 flex flex-col gap-2 pt-6 pb-6 safe-area-x",
            h2 { class: "px-1 text-xs font-semibold uppercase tracking-wide text-floral-white-400",
                {t!("data.export_section").to_string()}
            }
            p { class: "px-1 text-sm text-floral-white-400", {t!("data.export_hint").to_string()} }
            match &*groups.read() {
                Err(message) => rsx! {
                    ErrorBanner { error: Some(format!("{} {message}", t!("data.groups_error"))) }
                },
                Ok(list) if list.is_empty() => rsx! {
                    EmptyState {
                        title: t!("data.no_groups").to_string(),
                        text: t!("data.no_groups_text").to_string(),
                        Icon { icon: LdUsers, class: "h-8 w-8" }
                    }
                },
                Ok(list) => rsx! {
                    div { class: "flex flex-col overflow-hidden rounded-2xl border border-jet-black-800 bg-jet-black-900",
                        for group in list.iter().cloned() {
                            GroupExportRow {
                                key: "{group.id.as_str()}",
                                working: running == Some(Busy::Export(group.id.clone())),
                                disabled: running.is_some(),
                                group: group.clone(),
                                onclick: move |_| export.call(group.clone()),
                            }
                        }
                    }
                },
            }
        }
        if confirming() {
            ConfirmSheet {
                title: t!("data.restore_confirm_title").to_string(),
                text: t!("data.restore_confirm_text").to_string(),
                confirm_label: t!("data.restore_confirm").to_string(),
                on_confirm: restore,
                on_close: move |_| confirming.set(false),
            }
        }
    }
}

/// Marks `job` as running unless something else is; `false` if it must
/// not start.
fn begin(
    mut busy: Signal<Option<Busy>>,
    mut error: Signal<Option<String>>,
    supported: bool,
    job: Busy,
) -> bool {
    if busy.peek().is_some() {
        return false;
    }
    if !supported {
        error.set(Some(t!("data.unsupported").to_string()));
        return false;
    }
    error.set(None);
    busy.set(Some(job));
    true
}

/// Text for a failed restore: why the file was refused, or what broke.
fn restore_error_text(error: &BackupError) -> String {
    if error.is_too_new() {
        t!("data.too_new").to_string()
    } else if error.is_not_a_backup() {
        t!("data.not_a_backup").to_string()
    } else {
        format!("{} {error}", t!("data.restore_error"))
    }
}

/// Row starting a backup action: icon, label and a hint, which turns into
/// a progress text while it runs.
#[component]
fn ActionRow(
    label: String,
    hint: String,
    working: Option<String>,
    disabled: bool,
    onclick: EventHandler<()>,
    children: Element,
) -> Element {
    let is_working = working.is_some();
    let detail = working.unwrap_or(hint);
    rsx! {
        button {
            class: "flex min-h-16 w-full items-center gap-4 border-b border-jet-black-800 px-4 py-3 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple disabled:opacity-60",
            r#type: "button",
            disabled,
            aria_busy: if is_working { "true" } else { "false" },
            onclick: move |_| onclick.call(()),
            span { class: "flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-cerulean-800 text-cerulean-200",
                if is_working {
                    Spinner {}
                } else {
                    {children}
                }
            }
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "text-base font-medium text-floral-white-50", "{label}" }
                span { class: "text-sm text-floral-white-400", "{detail}" }
            }
        }
    }
}

/// A group to export as CSV.
#[component]
fn GroupExportRow(
    group: Group,
    working: bool,
    disabled: bool,
    onclick: EventHandler<()>,
) -> Element {
    rsx! {
        button {
            class: "flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple disabled:opacity-60",
            r#type: "button",
            disabled,
            aria_busy: if working { "true" } else { "false" },
            onclick: move |_| onclick.call(()),
            GroupIcon { icon: group.icon.clone(), color: group.color.clone() }
            span { class: "flex min-w-0 flex-1 flex-col",
                span { class: "truncate text-base text-floral-white-50", "{group.name}" }
                if working {
                    span { class: "text-sm text-floral-white-400", {t!("data.working_export").to_string()} }
                }
            }
            span { class: "flex h-10 w-10 shrink-0 items-center justify-center text-floral-white-400",
                if working {
                    Spinner {}
                } else {
                    Icon { icon: LdFileSpreadsheet, class: "h-5 w-5" }
                }
            }
        }
    }
}

#[component]
fn Spinner() -> Element {
    rsx! {
        span { class: "h-5 w-5 animate-spin rounded-full border-2 border-jet-black-700 border-t-cerulean-300" }
    }
}
