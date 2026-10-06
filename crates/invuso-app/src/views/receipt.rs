use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdCamera, LdCircleAlert, LdImage},
};

use super::{PlaceholderPage, ReceiptAdjuster};
use crate::Route;
use crate::components::{Button, ButtonVariant, EmptyState, TopBar};
use crate::platform::ImageKind;
use crate::services::receipts::capture_receipt;
use crate::storage::{Db, ReceiptFiles};

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

#[component]
pub fn ReceiptArchive() -> Element {
    rsx! { PlaceholderPage { title: t!("page.receipts").to_string(), show_back: true } }
}
