//! Background recognition with progress for the UI (idee.md 7.2 step 2:
//! "OCR läuft im Hintergrund, App bleibt bedienbar").

use std::collections::HashMap;
use std::sync::Arc;

use dioxus::prelude::*;
use tokio::sync::OnceCell;

use super::{OcrError, OcrProgress, PaddleOcr, analyze_receipt};
use crate::platform;
use crate::state::DataRevision;
use crate::storage::Db;

static DETECTOR: Asset = asset!("/assets/ocr/PP-OCRv6_det_small.onnx");
static RECOGNIZER: Asset = asset!("/assets/ocr/PP-OCRv6_rec_small.onnx");
const DICTIONARY: &str = include_str!("../../../assets/ocr/ppocrv6_dict.txt");

/// The models, loaded on first use and kept for the app's life: loading
/// takes about a second, and a trip usually brings many receipts.
static ENGINE: OnceCell<Arc<PaddleOcr>> = OnceCell::const_new();

async fn engine() -> Result<Arc<PaddleOcr>, OcrError> {
    ENGINE
        .get_or_try_init(|| async {
            let model =
                |e: dioxus::asset_resolver::AssetResolveError| OcrError::Model(e.to_string());
            let detector = dioxus::asset_resolver::read_asset_bytes(&DETECTOR)
                .await
                .map_err(model)?;
            let recognizer = dioxus::asset_resolver::read_asset_bytes(&RECOGNIZER)
                .await
                .map_err(model)?;
            tokio::task::spawn_blocking(move || PaddleOcr::load(detector, recognizer, DICTIONARY))
                .await
                .map_err(|e| OcrError::Platform(e.to_string()))?
                .map(Arc::new)
        })
        .await
        .cloned()
}

/// State of a recognition that has not finished successfully. A finished
/// one leaves no job: its text is stored with the receipt.
#[derive(Debug, Clone, PartialEq)]
pub enum OcrJob {
    /// Loading the models (first receipt after app start) or the image.
    Starting,
    Running(OcrProgress),
    Failed(String),
}

/// Recognitions of this app run, by receipt id. Provided above the router,
/// so a job outlives the screen that started it.
#[derive(Clone, Copy, PartialEq)]
pub struct OcrJobs(Signal<HashMap<String, OcrJob>>);

impl OcrJobs {
    /// Must be called inside a component, like any `Signal::new`.
    pub fn new() -> Self {
        Self(Signal::new(HashMap::new()))
    }

    /// The job of a receipt; subscribes the caller to its changes.
    pub fn get(&self, receipt_id: &str) -> Option<OcrJob> {
        self.0.read().get(receipt_id).cloned()
    }

    /// Like [`OcrJobs::get`], without subscribing.
    pub fn peek(&self, receipt_id: &str) -> Option<OcrJob> {
        self.0.peek().get(receipt_id).cloned()
    }

    /// Recognizes a receipt in the background, unless that is already
    /// running. When done, the text is stored and `revision` bumped.
    pub fn start(&self, db: Db, receipt_id: String, mut revision: DataRevision) {
        let mut jobs = self.0;
        if matches!(
            jobs.peek().get(&receipt_id),
            Some(OcrJob::Starting | OcrJob::Running(_))
        ) {
            return;
        }
        jobs.write().insert(receipt_id.clone(), OcrJob::Starting);
        // Not tied to the calling screen: leaving the form must not cancel
        // the work.
        dioxus::core::spawn_forever(async move {
            let outcome = run(db, receipt_id.clone(), jobs).await;
            match outcome {
                Ok(()) => {
                    jobs.write().remove(&receipt_id);
                    revision.bump();
                }
                Err(error) => {
                    jobs.write()
                        .insert(receipt_id, OcrJob::Failed(error.to_string()));
                }
            }
        });
    }
}

impl Default for OcrJobs {
    fn default() -> Self {
        Self::new()
    }
}

/// Loads the engine, runs [`analyze_receipt`] on a blocking thread and
/// forwards its progress.
async fn run(
    db: Db,
    receipt_id: String,
    mut jobs: Signal<HashMap<String, OcrJob>>,
) -> Result<(), OcrError> {
    let data_dir = platform::data_dir().map_err(OcrError::Platform)?;
    let engine = engine().await?;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let id = receipt_id.clone();
    let worker = tokio::task::spawn_blocking(move || {
        analyze_receipt(&db, &data_dir, engine.as_ref(), &id, &mut |progress| {
            // The receiver only goes away with the app.
            let _ = sender.send(progress);
        })
    });
    // Ends when the worker drops the sender.
    while let Some(progress) = receiver.recv().await {
        jobs.write()
            .insert(receipt_id.clone(), OcrJob::Running(progress));
    }
    worker
        .await
        .map_err(|e| OcrError::Platform(e.to_string()))?
        .map(|_| ())
}
