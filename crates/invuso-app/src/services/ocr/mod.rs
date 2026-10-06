//! On-device text recognition of receipts (OCR-01, OCR-02, OCR-04;
//! decision 10.1, idee.md 7.2 step 2).
//!
//! [`OcrEngine`] hides the engine; [`PaddleOcr`] is the one the app ships.
//! [`analyze_receipt`] prepares the archived photo, runs the engine and
//! stores the recognized boxes with the receipt; the parser in
//! `invuso-core` reads them from there. [`OcrJobs`] runs this in the
//! background and reports progress to the UI.

mod jobs;
mod paddle;
pub(crate) mod preprocess;

use std::path::Path;

use image::RgbImage;
use thiserror::Error;

pub use jobs::{OcrJob, OcrJobs};
pub use paddle::PaddleOcr;

use crate::storage::{Db, OcrFragment, ReceiptText, StorageError};

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("the text recognition models could not be loaded: {0}")]
    Model(String),
    #[error("the receipt image could not be read: {0}")]
    Image(String),
    #[error("the receipt does not exist")]
    NoReceipt,
    #[error("{0}")]
    Platform(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// How far a recognition is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcrProgress {
    /// Looking for text regions.
    Detecting,
    /// Reading region `done + 1` of `total`.
    Reading { done: usize, total: usize },
}

/// What an engine found in one image.
#[derive(Debug, Clone, PartialEq)]
pub struct Recognition {
    /// Boxes are in the levelled image: turned back by `skew_degrees`
    /// around the image centre, so printed rows are horizontal.
    pub fragments: Vec<OcrFragment>,
    /// How far the photo's lines slope down to the right.
    pub skew_degrees: f32,
}

/// A text recognition engine running on the device.
pub trait OcrEngine: Send + Sync {
    /// Stored with each result (`receipt.ocr_engine`).
    fn name(&self) -> &'static str;

    /// Recognizes all text in an upright image. Takes a while: call it off
    /// the UI thread.
    fn recognize(
        &self,
        image: &RgbImage,
        progress: &mut dyn FnMut(OcrProgress),
    ) -> Result<Recognition, OcrError>;
}

/// Recognizes the text of an archived receipt and stores it with the
/// receipt (status `analyzed`). Blocking; `data_dir` holds the image paths.
pub fn analyze_receipt(
    db: &Db,
    data_dir: &Path,
    engine: &dyn OcrEngine,
    receipt_id: &str,
    progress: &mut dyn FnMut(OcrProgress),
) -> Result<ReceiptText, OcrError> {
    let receipt = db.receipt(receipt_id)?.ok_or(OcrError::NoReceipt)?;
    // Several pages come with RCP-06; until then a receipt has one image.
    // A corrected copy (RCP-05) is what the user wants read.
    let path = receipt.page(0).ok_or(OcrError::NoReceipt)?;
    let bytes = std::fs::read(data_dir.join(path)).map_err(|e| OcrError::Image(e.to_string()))?;
    let image = preprocess::prepare(&bytes)?;
    let recognition = engine.recognize(&image, progress)?;
    let text = ReceiptText::new(
        engine.name(),
        recognition.fragments,
        recognition.skew_degrees,
    );
    db.save_receipt_text(receipt_id, &text)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Mutex;

    use image::{DynamicImage, ImageFormat};
    use invuso_core::receipt::BoundingBox;

    use super::*;

    /// Reports one fixed line and remembers the image size it was given.
    struct FakeEngine {
        seen: Mutex<Option<(u32, u32)>>,
    }

    impl OcrEngine for FakeEngine {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn recognize(
            &self,
            image: &RgbImage,
            progress: &mut dyn FnMut(OcrProgress),
        ) -> Result<Recognition, OcrError> {
            *self.seen.lock().unwrap() = Some(image.dimensions());
            progress(OcrProgress::Detecting);
            progress(OcrProgress::Reading { done: 0, total: 1 });
            let fragments = vec![OcrFragment {
                text: "SUMME 1,99".to_string(),
                bbox: BoundingBox {
                    left: 1,
                    top: 2,
                    right: 30,
                    bottom: 8,
                },
                confidence: 0.5,
            }];
            Ok(Recognition {
                fragments,
                skew_degrees: 1.5,
            })
        }
    }

    #[test]
    fn stores_the_recognized_text_with_the_receipt() {
        let db = Db::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("invuso-ocr-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(dir.join("receipts")).unwrap();
        let mut png = Vec::new();
        DynamicImage::ImageRgb8(RgbImage::new(40, 20))
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        std::fs::write(dir.join("receipts/a.png"), png).unwrap();
        let receipt = db.create_receipt("receipts/a.png", None).unwrap();
        assert_eq!(db.receipt_text(&receipt.id).unwrap(), None);

        let engine = FakeEngine {
            seen: Mutex::new(None),
        };
        let mut steps = Vec::new();
        let text =
            analyze_receipt(&db, &dir, &engine, &receipt.id, &mut |p| steps.push(p)).unwrap();
        assert_eq!(*engine.seen.lock().unwrap(), Some((40, 20)));
        assert_eq!(steps.len(), 2);
        assert_eq!(text.raw_text, "SUMME 1,99");
        assert_eq!(text.skew_degrees, 1.5);
        assert_eq!(db.receipt_text(&receipt.id).unwrap(), Some(text));

        let missing = analyze_receipt(&db, &dir, &engine, "missing", &mut |_| {});
        assert!(matches!(missing, Err(OcrError::NoReceipt)));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// End to end with the shipped models on the AP-S1 photos, which are
    /// not in the repository (`spikes/ocr/README.md`). Writes the rows to
    /// `spikes/ocr/results/app/` for `score.py`; `OCR_CONTRAST=off` skips
    /// the contrast stretch (results in `app-raw/`) to compare both.
    #[test]
    #[ignore = "needs the AP-S1 sample photos in spikes/ocr/samples"]
    fn recognizes_the_sample_receipts() {
        use invuso_core::domain::Currency;
        use invuso_core::receipt::{TotalCheck, parse_receipt, text_rows};

        let app = Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |name: &str| std::fs::read(app.join("assets/ocr").join(name)).unwrap();
        let dictionary = String::from_utf8(read("ppocrv6_dict.txt")).unwrap();
        let engine = PaddleOcr::load(
            read("PP-OCRv6_det_small.onnx"),
            read("PP-OCRv6_rec_small.onnx"),
            &dictionary,
        )
        .unwrap();
        let stretch = std::env::var("OCR_CONTRAST").as_deref() != Ok("off");
        let spike = app.join("../../spikes/ocr");
        // `OCR_SAMPLES=samples_tilted` runs another folder of the spike.
        let samples = std::env::var("OCR_SAMPLES").unwrap_or_else(|_| "samples".to_string());
        let out = spike
            .join("results")
            .join(format!("app{}", if stretch { "" } else { "-raw" }))
            .join(if samples == "samples" {
                ""
            } else {
                samples.as_str()
            });
        std::fs::create_dir_all(&out).unwrap();

        let mut paths: Vec<_> = std::fs::read_dir(spike.join(&samples))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("jpg" | "png")))
            .collect();
        paths.sort();
        let mut german = 0;
        let mut japanese_matched = 0;
        for path in paths {
            let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
            let mut image = preprocess::decode(&std::fs::read(&path).unwrap()).unwrap();
            if stretch {
                preprocess::stretch_contrast(&mut image);
            }
            let started = std::time::Instant::now();
            let recognition = engine.recognize(&image, &mut |_| {}).unwrap();
            let text = ReceiptText::new(
                engine.name(),
                recognition.fragments,
                recognition.skew_degrees,
            );
            let elapsed = started.elapsed().as_millis();
            let rows = text_rows(&text.recognized());
            std::fs::write(out.join(format!("{stem}.txt")), rows.join("\n") + "\n").unwrap();
            let boxes: Vec<String> = text
                .fragments
                .iter()
                .map(|f| {
                    let b = f.bbox;
                    format!(
                        "{}\t{}\t{}\t{}\t{:.2}\t{}",
                        b.left, b.top, b.right, b.bottom, f.confidence, f.text
                    )
                })
                .collect();
            std::fs::write(
                out.join(format!("{stem}.boxes.tsv")),
                boxes.join("\n") + "\n",
            )
            .unwrap();
            println!(
                "{stem:22} {elapsed:>6} ms {:>4} rows, skew {:.1}°",
                rows.len(),
                text.skew_degrees
            );
            // `own_*` are the user's own photos: no expected result yet, so
            // only the parsed items are written for comparing by eye.
            let own = stem.starts_with("own_");
            let japanese = stem.starts_with("ja_");
            if stem.starts_with("de_") || own || japanese {
                let code = if japanese { "JPY" } else { "EUR" };
                let parsed =
                    parse_receipt(&text.recognized(), Currency::from_code(code).unwrap()).unwrap();
                println!("    {} items, check {:?}", parsed.items.len(), parsed.check);
                let items: Vec<String> = parsed
                    .items
                    .iter()
                    .map(|i| format!("{} | {} | {:?}", i.text, i.quantity, i.total_price))
                    .collect();
                std::fs::write(
                    out.join(format!("{stem}.items.txt")),
                    items.join("\n") + "\n",
                )
                .unwrap();
                if stem.starts_with("de_") {
                    german += 1;
                    assert_eq!(parsed.check, TotalCheck::Matches, "{stem}");
                }
                // Japanese photos that are no till receipt (`donki` header
                // only, `receipt_jpy` handwritten) or whose set parts read
                // `1コ` as `13` (McDonald's) are only written out.
                let unparseable = [
                    "ja_donki",
                    "ja_receipt_jpy",
                    "ja_mcd_kanayama",
                    "ja_mcd_yabacho",
                ];
                if japanese && !unparseable.contains(&stem.as_str()) {
                    japanese_matched += 1;
                    assert_eq!(parsed.check, TotalCheck::Matches, "{stem}");
                }
            }
        }
        assert!(german > 0, "no German samples");
        println!("{japanese_matched} Japanese receipts match their total");
    }

    /// AP-34: a receipt photographed at an angle is read better once its
    /// four corners are straightened (RCP-05). The German AP-S1 photos are
    /// put into a strong perspective, as a tilted phone sees them; the
    /// corners given back are a little off, as a finger sets them.
    #[test]
    #[ignore = "needs the AP-S1 sample photos in spikes/ocr/samples"]
    fn straightening_a_tilted_receipt_reads_more() {
        use invuso_core::domain::Currency;
        use invuso_core::receipt::{TotalCheck, parse_receipt};

        use crate::services::receipt_edit::{
            ImageEdit, Point, detect_corners, photograph_at_an_angle,
        };

        let app = Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |name: &str| std::fs::read(app.join("assets/ocr").join(name)).unwrap();
        let dictionary = String::from_utf8(read("ppocrv6_dict.txt")).unwrap();
        let engine = PaddleOcr::load(
            read("PP-OCRv6_det_small.onnx"),
            read("PP-OCRv6_rec_small.onnx"),
            &dictionary,
        )
        .unwrap();
        let euro = Currency::from_code("EUR").unwrap();
        // Items found (printed only: a tilted photo can yield wrong ones)
        // and whether they add up to the printed total.
        let read_receipt = |image: &mut RgbImage| {
            preprocess::stretch_contrast(image);
            let recognition = engine.recognize(image, &mut |_| {}).unwrap();
            let text = ReceiptText::new(engine.name(), recognition.fragments, 0.0);
            let parsed = parse_receipt(&text.recognized(), euro).unwrap();
            (parsed.items.len(), parsed.check == TotalCheck::Matches)
        };

        // Narrow at the top and turned a little; the photo has the
        // receipt's proportions, so straightening gives them back.
        let corners = [
            Point::new(0.30, 0.06),
            Point::new(0.74, 0.12),
            Point::new(0.94, 0.95),
            Point::new(0.06, 0.90),
        ];
        let set_by_finger = [
            Point::new(0.29, 0.055),
            Point::new(0.75, 0.115),
            Point::new(0.945, 0.955),
            Point::new(0.055, 0.905),
        ];
        let samples = app.join("../../spikes/ocr/samples");
        let mut paths: Vec<_> = std::fs::read_dir(&samples)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("de_"))
            })
            .collect();
        paths.sort();
        // Matching totals per variant: tilted, straightened by finger, the
        // same with each filter, and with the corners found automatically.
        let names = ["tilted", "finger", "+contrast", "+sharpen", "auto"];
        let mut matched = [0usize; 5];
        for path in &paths {
            let receipt = preprocess::decode(&std::fs::read(path).unwrap()).unwrap();
            let scale = 4000.0 / f64::from(receipt.width().max(receipt.height()));
            let (width, height) = (
                (f64::from(receipt.width()) * scale) as u32,
                (f64::from(receipt.height()) * scale) as u32,
            );
            let tilted = photograph_at_an_angle(&receipt, &corners, width, height);
            let finger = ImageEdit::default().with_upright_corners(set_by_finger);
            let detected = detect_corners(&tilted);
            let variants = [
                Some(tilted.clone()),
                finger.apply(&tilted),
                ImageEdit {
                    contrast: true,
                    ..finger
                }
                .apply(&tilted),
                ImageEdit {
                    sharpen: true,
                    ..finger
                }
                .apply(&tilted),
                detected.and_then(|c| ImageEdit::default().with_upright_corners(c).apply(&tilted)),
            ];
            print!("{:24}", path.file_name().unwrap().to_string_lossy());
            for (index, variant) in variants.into_iter().enumerate() {
                match variant {
                    Some(mut image) => {
                        let (items, total) = read_receipt(&mut image);
                        matched[index] += usize::from(total);
                        print!(
                            "  {} {items:>2} {}",
                            names[index],
                            if total { "ok" } else { "--" }
                        );
                    }
                    None => print!("  {} none", names[index]),
                }
            }
            println!();
        }
        println!("matching totals: {names:?} {matched:?}");
        let (matched_before, matched_after) = (matched[0], matched[1]);
        assert!(!paths.is_empty(), "no German samples");
        // 2026-10-06: 2 of 5 tilted, 5 of 5 straightened by finger, with
        // either filter and with the corners found automatically.
        assert!(
            matched_after > matched_before,
            "{matched_before} → {matched_after}"
        );
        assert_eq!(matched_after, paths.len());
        assert_eq!(matched[4], paths.len(), "automatic corners");
    }
}
