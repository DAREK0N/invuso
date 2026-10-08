use dioxus::prelude::*;
use dioxus::router::Navigator;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdCamera, LdChevronRight, LdCircleAlert, LdImage, LdReceipt, LdSearchX},
};

use super::ReceiptAdjuster;
use crate::components::{
    BottomSheet, Button, ButtonVariant, EmptyState, MoneyText, SearchField, TopBar,
};
use crate::platform::ImageKind;
use crate::preferences::{default_home_currency, display_date};
use crate::services::ocr::{OcrJob, OcrJobs};
use crate::services::receipt_archive::{ArchiveEntry, receipt_archive};
use crate::services::receipts::{capture_receipt, file_url};
use crate::state::DataRevision;
use crate::storage::{Db, ReceiptFiles, ReceiptStatus};
use crate::{Route, clock};

/// `source` of [`Route::Scan`] that opens the camera.
pub const SOURCE_CAMERA: &str = "camera";
/// `source` of [`Route::Scan`] that opens the photo picker.
pub const SOURCE_GALLERY: &str = "gallery";

/// What the scan screen is doing.
#[derive(Debug, Clone, PartialEq)]
enum ScanState {
    /// Camera or picker open, or the image being archived.
    Working,
    /// Archived; the user turns and crops it (RCP-05).
    Adjusting(ReceiptFiles),
    Failed(String),
}

/// `/scan?source=…`: opens the camera or the photo picker right away,
/// archives the image, lets the user turn, crop and straighten it
/// (idee.md 7.2 steps 1–2, RCP-05) and continues to its review (step 5,
/// `ReceiptReview`), where text recognition runs (RCP-01..03). Backing out
/// of the picker or the adjusting returns to where the plus button was
/// tapped; the photo stays archived.
#[component]
pub fn Scan(source: String) -> Element {
    let db = use_context::<Db>();
    let nav = use_navigator();
    let mut state = use_signal(|| ScanState::Working);
    let kind = if source == SOURCE_CAMERA {
        ImageKind::Camera
    } else {
        ImageKind::Gallery
    };

    let start = use_callback(move |kind: ImageKind| {
        state.set(ScanState::Working);
        let db = db.clone();
        spawn(async move {
            match capture_receipt(db, kind).await {
                Ok(Some(receipt)) => state.set(ScanState::Adjusting(receipt)),
                Ok(None) if nav.can_go_back() => nav.go_back(),
                Ok(None) => {
                    nav.replace(Route::Home {});
                }
                Err(error) => state.set(ScanState::Failed(error.to_string())),
            }
        });
    });
    use_hook(move || start.call(kind));

    let leave = move || {
        if nav.can_go_back() {
            nav.go_back();
        } else {
            nav.replace(Route::Home {});
        }
    };

    rsx! {
        TopBar { title: t!("page.scan").to_string(), show_back: true }
        match state() {
            ScanState::Working => rsx! {
                div { class: "flex flex-col items-center gap-4 px-8 py-16 text-center", role: "status",
                    div { class: "h-10 w-10 animate-spin rounded-full border-4 border-jet-black-700 border-t-cerulean-400" }
                    p { class: "text-sm text-floral-white-300", {t!("scan.working").to_string()} }
                }
            },
            ScanState::Adjusting(receipt) => rsx! {
                ReceiptAdjuster {
                    receipt,
                    on_done: move |receipt: ReceiptFiles| {
                        nav.replace(Route::ReceiptReview {
                            receipt_id: receipt.id,
                        });
                    },
                    on_cancel: move |_| leave(),
                }
            },
            ScanState::Failed(message) => rsx! {
                EmptyState {
                    title: t!("scan.error_title").to_string(),
                    text: message,
                    Icon { icon: LdCircleAlert, class: "h-8 w-8" }
                }
                div { class: "mx-4 flex flex-col gap-3 safe-area-x",
                    Button {
                        class: "w-full",
                        onclick: move |_| start.call(kind),
                        if kind == ImageKind::Camera {
                            Icon { icon: LdCamera, class: "h-5 w-5" }
                        } else {
                            Icon { icon: LdImage, class: "h-5 w-5" }
                        }
                        {t!("scan.retry").to_string()}
                    }
                    Button {
                        variant: ButtonVariant::Secondary,
                        class: "w-full",
                        onclick: move |_| nav.go_back(),
                        {t!("common.back").to_string()}
                    }
                }
            },
        }
    }
}

/// `/receipts`: every receipt across groups (RCP-09), newest first, the
/// ones without expense on top: saved "for later" (RCP-07), left or
/// replaced. A search covers merchant, expense and recognized text. An open
/// receipt leads to its review, one with an expense to that expense.
#[component]
pub fn ReceiptArchive() -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let jobs = use_context::<OcrJobs>();
    let nav = use_navigator();
    let mut query = use_signal(String::new);
    let entries = use_archive(db, revision);

    let body = match &*entries.read() {
        None => rsx! { Loading {} },
        Some(Err(message)) => rsx! {
            EmptyState {
                title: t!("archive.load_error_title").to_string(),
                text: message.clone(),
                Icon { icon: LdCircleAlert, class: "h-8 w-8" }
            }
        },
        Some(Ok(entries)) if entries.is_empty() => rsx! {
            EmptyState {
                title: t!("archive.empty_title").to_string(),
                text: t!("archive.empty_text").to_string(),
                Icon { icon: LdReceipt, class: "h-8 w-8" }
            }
        },
        Some(Ok(entries)) => {
            let query_now = query();
            let (open, done): (Vec<ArchiveEntry>, Vec<ArchiveEntry>) = entries
                .iter()
                .filter(|entry| entry.matches(&query_now))
                .cloned()
                .partition(ArchiveEntry::is_open);
            let sections = [
                (t!("archive.open", count = open.len()).to_string(), open),
                (t!("archive.done", count = done.len()).to_string(), done),
            ];
            let no_hits = sections.iter().all(|(_, list)| list.is_empty());
            rsx! {
                div { class: "flex flex-col gap-5",
                    SearchField {
                        value: query_now.clone(),
                        placeholder: t!("archive.search").to_string(),
                        oninput: move |text| query.set(text),
                    }
                    if no_hits {
                        EmptyState {
                            title: t!("archive.no_hits_title").to_string(),
                            text: t!("archive.no_hits_text").to_string(),
                            Icon { icon: LdSearchX, class: "h-8 w-8" }
                        }
                    }
                    for (heading, list) in sections {
                        if !list.is_empty() {
                            section { key: "{heading}", class: "flex flex-col gap-2",
                                h2 { class: "px-1 text-sm font-medium text-floral-white-300", "{heading}" }
                                div { class: "overflow-hidden rounded-2xl border border-jet-black-700 bg-jet-black-900",
                                    for entry in list {
                                        ReceiptRow {
                                            key: "{entry.receipt.id}",
                                            job: jobs.get(&entry.receipt.id),
                                            onclick: {
                                                let entry = entry.clone();
                                                move |_| open_entry(nav, &entry)
                                            },
                                            entry,
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        TopBar { title: t!("page.receipts").to_string(), show_back: true }
        div { class: "mx-4 flex flex-col gap-5 pt-4 safe-area-x", {body} }
    }
}

/// An open receipt goes to its review, one with an expense to that
/// expense (AP-33 step 3).
fn open_entry(nav: Navigator, entry: &ArchiveEntry) {
    match &entry.receipt.expense {
        Some(expense) => {
            nav.push(Route::ExpenseDetail {
                id: expense.id.as_str().to_string(),
            });
        }
        None => {
            nav.push(Route::ReceiptReview {
                receipt_id: entry.receipt.id.clone(),
            });
        }
    }
}

/// Sheet listing the receipts no expense holds, to attach one to the
/// expense being edited (RCP-08).
#[component]
pub fn ArchivePickSheet(
    on_pick: EventHandler<ReceiptFiles>,
    on_close: EventHandler<()>,
) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let jobs = use_context::<OcrJobs>();
    let entries = use_archive(db.clone(), revision);
    let mut error = use_signal(|| None::<String>);

    let body = match &*entries.read() {
        None => rsx! { Loading {} },
        Some(Err(message)) => rsx! {
            p { class: "px-1 py-4 text-sm text-watermelon-300", role: "alert", "{message}" }
        },
        Some(Ok(entries)) => {
            let open: Vec<ArchiveEntry> = entries.iter().filter(|e| e.is_open()).cloned().collect();
            if open.is_empty() {
                rsx! {
                    EmptyState {
                        title: t!("archive.pick_empty_title").to_string(),
                        text: t!("archive.pick_empty_text").to_string(),
                        Icon { icon: LdReceipt, class: "h-8 w-8" }
                    }
                }
            } else {
                rsx! {
                    div { class: "overflow-hidden rounded-2xl border border-jet-black-700 bg-jet-black-900",
                        for entry in open {
                            ReceiptRow {
                                key: "{entry.receipt.id}",
                                job: jobs.get(&entry.receipt.id),
                                onclick: {
                                    let (db, id) = (db.clone(), entry.receipt.id.clone());
                                    move |_| match db.receipt(&id) {
                                        Ok(Some(files)) => on_pick.call(files),
                                        Ok(None) => error.set(Some(t!("archive.pick_gone").to_string())),
                                        Err(e) => error.set(Some(e.to_string())),
                                    }
                                },
                                entry,
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        BottomSheet {
            title: t!("archive.pick_title").to_string(),
            on_close: move |_| on_close.call(()),
            div { class: "flex max-h-[70vh] flex-col gap-2 overflow-y-auto overscroll-contain px-3 pt-2",
                if let Some(message) = error() {
                    p { class: "px-1 text-sm text-watermelon-300", role: "alert", "{message}" }
                }
                {body}
            }
        }
    }
}

/// The archive, read off the UI thread and again after every data change.
fn use_archive(db: Db, revision: DataRevision) -> Resource<Result<Vec<ArchiveEntry>, String>> {
    use_resource(move || {
        revision.track();
        let db = db.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                let fallback = db
                    .profile()?
                    .map_or_else(default_home_currency, |p| p.home_currency);
                receipt_archive(&db, fallback)
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
        }
    })
}

#[component]
fn Loading() -> Element {
    rsx! {
        div { class: "flex flex-col items-center gap-4 px-8 py-16 text-center", role: "status",
            div { class: "h-10 w-10 animate-spin rounded-full border-4 border-jet-black-700 border-t-cerulean-400" }
            p { class: "text-sm text-floral-white-300", {t!("archive.loading").to_string()} }
        }
    }
}

/// One receipt: thumbnail, what it is (expense or merchant), its day, its
/// status and the expense's amount.
#[component]
fn ReceiptRow(entry: ArchiveEntry, job: Option<OcrJob>, onclick: EventHandler<()>) -> Element {
    let receipt = &entry.receipt;
    let expense = receipt.expense.as_ref();
    let title = expense
        .map(|e| e.title.clone())
        .or_else(|| entry.merchant.clone())
        .unwrap_or_else(|| t!("archive.unnamed").to_string());
    let (day, place) = match expense {
        Some(e) => (
            e.occurred_at.get(0..10).unwrap_or_default().to_string(),
            e.group_name
                .clone()
                .unwrap_or_else(|| t!("expense.no_group").to_string()),
        ),
        None => (
            clock::local_date_of(receipt.created_at),
            t!("archive.no_expense").to_string(),
        ),
    };
    let subtitle = format!("{} · {place}", display_date(&day));
    let (status, tone) = match (&job, receipt.status) {
        (Some(OcrJob::Starting | OcrJob::Running(_)), _) => (
            t!("archive.status_reading"),
            "bg-cerulean-900 text-cerulean-200",
        ),
        (Some(OcrJob::Failed(_)), _) => (
            t!("archive.status_failed"),
            "bg-watermelon-900 text-watermelon-200",
        ),
        (None, ReceiptStatus::New) => (
            t!("archive.status_new"),
            "bg-jet-black-800 text-floral-white-300",
        ),
        (None, ReceiptStatus::Analyzed) => (
            t!("archive.status_analyzed"),
            "bg-pale-oak-900 text-pale-oak-200",
        ),
        (None, ReceiptStatus::Reviewed) => (
            t!("archive.status_reviewed"),
            "bg-muted-teal-900 text-muted-teal-200",
        ),
    };

    rsx! {
        button {
            class: "flex min-h-16 w-full items-center gap-3 border-b border-jet-black-800 px-4 py-2 text-left last:border-b-0 active:bg-jet-black-800 transition-colors ease-apple",
            r#type: "button",
            onclick: move |_| onclick.call(()),
            if let Some(path) = &receipt.thumbnail_path {
                img {
                    class: "h-12 w-12 shrink-0 rounded-xl bg-jet-black-800 object-cover object-top",
                    src: file_url(path),
                    alt: "",
                    loading: "lazy",
                }
            } else {
                span {
                    class: "flex h-12 w-12 shrink-0 items-center justify-center rounded-xl bg-jet-black-800 text-floral-white-300",
                    aria_hidden: "true",
                    Icon { icon: LdReceipt, class: "h-5 w-5" }
                }
            }
            span { class: "flex min-w-0 flex-1 flex-col gap-0.5",
                span { class: "truncate text-base text-floral-white-50", "{title}" }
                span { class: "truncate text-sm text-floral-white-400", "{subtitle}" }
                span { class: "self-start rounded-full px-2 py-0.5 text-xs font-medium {tone}", {status.to_string()} }
            }
            if let Some(e) = expense {
                MoneyText { amount: e.total_in_base, class: "shrink-0 text-base font-semibold text-floral-white-100" }
            }
            Icon { icon: LdChevronRight, class: "h-5 w-5 shrink-0 text-floral-white-500" }
        }
    }
}
