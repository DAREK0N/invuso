use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{LdCircleAlert, LdRotateCcw, LdScanText, LdTriangleAlert},
};
use invuso_core::domain::Currency;
use invuso_core::receipt::{ParsedReceipt, TotalCheck, parse_receipt};

use crate::format::{NumberFormat, format_money};
use crate::services::ocr::{OcrJob, OcrJobs, OcrProgress};
use crate::state::DataRevision;
use crate::storage::Db;

/// Text recognition of the attached receipt (OCR-01, idee.md 7.2 steps
/// 2–3): starts it unless it ran before, shows its progress while the form
/// stays usable, then what the parser read. Hands what was read to the
/// form once through `on_read` (idee.md 7.2 step 3), so it can fill an
/// empty amount and the line items.
#[component]
pub(super) fn ReceiptRecognition(
    receipt_id: String,
    currency: ReadSignal<Currency>,
    on_read: EventHandler<ParsedReceipt>,
) -> Element {
    let db = use_context::<Db>();
    let revision = use_context::<DataRevision>();
    let jobs = use_context::<OcrJobs>();
    // The parent keys this component by receipt, so the id never changes.
    let id = use_hook(|| receipt_id.clone());

    let stored = use_memo({
        let (db, id) = (db.clone(), id.clone());
        move || {
            revision.track();
            db.receipt_text(&id).map_err(|e| e.to_string())
        }
    });
    let parsed = use_memo(move || match &*stored.read() {
        // Parsing a few hundred boxes takes microseconds; redone when the
        // currency changes, since it decides how amounts are read.
        Ok(Some(text)) => Some(parse_receipt(&text.recognized(), currency()).ok()),
        _ => None,
    });

    use_effect({
        let (db, id) = (db.clone(), id.clone());
        move || {
            // A failed run waits for the retry button instead of looping.
            if matches!(*stored.read(), Ok(None)) && jobs.peek(&id).is_none() {
                jobs.start(db.clone(), id.clone(), revision);
            }
        }
    });

    let mut offered = use_signal(|| false);
    use_effect(move || {
        let receipt = parsed.read().clone().flatten();
        if let Some(receipt) = receipt
            && !*offered.peek()
        {
            offered.set(true);
            on_read.call(receipt);
        }
    });

    let retry_db = db.clone();
    let retry_id = id.clone();
    let retry = move |_| jobs.start(retry_db.clone(), retry_id.clone(), revision);

    let body = match (jobs.get(&id), &*stored.read()) {
        (Some(OcrJob::Failed(message)), _) => rsx! {
            div { class: "flex items-start gap-3", role: "alert",
                Icon { icon: LdCircleAlert, class: "mt-0.5 h-5 w-5 shrink-0 text-watermelon-300" }
                div { class: "flex min-w-0 flex-1 flex-col gap-1",
                    p { class: "text-sm text-floral-white-50", {t!("ocr.failed").to_string()} }
                    p { class: "text-xs break-words text-floral-white-400", "{message}" }
                }
            }
            button {
                class: "flex min-h-11 items-center gap-2 self-start rounded-full px-3 text-sm font-medium text-cerulean-300 active:bg-jet-black-800 transition-colors",
                r#type: "button",
                onclick: retry,
                Icon { icon: LdRotateCcw, class: "h-4 w-4" }
                {t!("ocr.retry").to_string()}
            }
        },
        (Some(job), _) => rsx! { Progress { job } },
        (None, Err(message)) => rsx! {
            p { class: "text-sm text-watermelon-300", role: "alert", "{message}" }
        },
        (None, Ok(None)) => rsx! { Progress { job: OcrJob::Starting } },
        (None, Ok(Some(_))) => match parsed() {
            Some(Some(receipt)) => rsx! { Outcome { receipt } },
            _ => rsx! { Line { icon_alert: true, text: t!("ocr.unreadable").to_string() } },
        },
    };

    rsx! {
        div { class: "flex flex-col gap-2 rounded-2xl border border-jet-black-800 bg-jet-black-900 px-4 py-3",
            {body}
        }
    }
}

#[component]
fn Progress(job: OcrJob) -> Element {
    let (text, share) = match job {
        OcrJob::Running(OcrProgress::Reading { done, total }) => (
            t!("ocr.reading", done = done + 1, total = total).to_string(),
            Some((done * 100 / total.max(1)).min(100)),
        ),
        OcrJob::Running(OcrProgress::Detecting) => (t!("ocr.detecting").to_string(), None),
        _ => (t!("ocr.starting").to_string(), None),
    };
    rsx! {
        div { class: "flex flex-col gap-2", role: "status",
            div { class: "flex items-center gap-3",
                Icon { icon: LdScanText, class: "h-5 w-5 shrink-0 animate-pulse text-cerulean-300" }
                p { class: "text-sm text-floral-white-200", "{text}" }
            }
            div { class: "h-1.5 w-full overflow-hidden rounded-full bg-jet-black-700",
                match share {
                    Some(percent) => rsx! {
                        div {
                            class: "h-full rounded-full bg-cerulean-400 transition-[width] ease-apple",
                            style: "width: {percent}%",
                        }
                    },
                    None => rsx! { div { class: "h-full w-1/3 animate-pulse rounded-full bg-cerulean-400" } },
                }
            }
        }
    }
}

/// What the parser read: number of positions and the total, or why the
/// two do not agree (OCR-14).
#[component]
fn Outcome(receipt: ParsedReceipt) -> Element {
    let format = NumberFormat::current();
    let count = receipt.items.len();
    let items = if count == 1 {
        t!("ocr.items_one").to_string()
    } else {
        t!("ocr.items_other", count = count).to_string()
    };
    let summary = match receipt.total {
        Some(total) => t!(
            "ocr.found_total",
            items = items,
            total = format_money(total, format)
        )
        .to_string(),
        None => items,
    };
    let warning = match receipt.check {
        TotalCheck::Matches => None,
        TotalCheck::NoTotal => Some(t!("ocr.no_total").to_string()),
        TotalCheck::Differs { items_sum, .. } => {
            Some(t!("ocr.differs", sum = format_money(items_sum, format)).to_string())
        }
    };
    rsx! {
        Line { icon_alert: false, text: summary }
        if let Some(warning) = warning {
            div { class: "flex items-start gap-3",
                Icon { icon: LdTriangleAlert, class: "mt-0.5 h-5 w-5 shrink-0 text-pale-oak-300" }
                p { class: "text-sm text-pale-oak-200", "{warning}" }
            }
        }
    }
}

#[component]
fn Line(icon_alert: bool, text: String) -> Element {
    rsx! {
        div { class: "flex items-start gap-3", role: "status",
            if icon_alert {
                Icon { icon: LdTriangleAlert, class: "mt-0.5 h-5 w-5 shrink-0 text-pale-oak-300" }
            } else {
                Icon { icon: LdScanText, class: "mt-0.5 h-5 w-5 shrink-0 text-cerulean-300" }
            }
            p { class: "text-sm text-floral-white-50", "{text}" }
        }
    }
}
