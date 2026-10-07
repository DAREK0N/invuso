use invuso_core::receipt::{BoundingBox, RecognizedText, text_rows};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::db::{new_id, now_ms};
use super::{Db, StorageError};

/// An archived receipt image as the app shows it (RCP-03, RCP-04). Paths
/// are relative to the data directory, so a backup can move them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptFiles {
    pub id: String,
    /// Originals in page order (several only with RCP-06); never changed.
    pub image_paths: Vec<String>,
    /// Turned, cropped and straightened copy of each page (RCP-05), `None`
    /// where the original was taken as it is.
    pub edited_paths: Vec<Option<String>>,
    /// `None` if the image could not be decoded for a preview.
    pub thumbnail_path: Option<String>,
}

impl ReceiptFiles {
    /// The image of a page that is shown and recognized: the corrected
    /// copy if there is one, else the original.
    pub fn page(&self, index: usize) -> Option<&str> {
        let edited = self.edited_paths.get(index).and_then(Option::as_deref);
        edited.or_else(|| self.image_paths.get(index).map(String::as_str))
    }
}

/// One piece of recognized text with its box (OCR-01).
#[derive(Debug, Clone, PartialEq)]
pub struct OcrFragment {
    pub text: String,
    /// In pixels of the image the engine read, turned back by
    /// [`ReceiptText::skew_degrees`] (rows level).
    pub bbox: BoundingBox,
    /// Mean probability of its characters, 0–1.
    pub confidence: f32,
}

/// The recognized text of a receipt, as stored with it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReceiptText {
    pub engine: String,
    pub fragments: Vec<OcrFragment>,
    /// The fragments as printed rows, one per line (`ocr_raw_text`).
    pub raw_text: String,
    /// Mean confidence of all fragments; `None` if nothing was found.
    pub confidence: Option<f32>,
    /// The fragments' boxes are in the photo turned back by this angle
    /// around its centre (rows level); needed to mark them in the photo.
    pub skew_degrees: f32,
    /// Width and height of the image the engine read, which may be smaller
    /// than the photo; `None` for results stored before AP-36, which
    /// cannot be marked in the photo then (OCR-37).
    pub image_size: Option<(u32, u32)>,
}

impl ReceiptText {
    pub fn new(engine: &str, fragments: Vec<OcrFragment>, skew_degrees: f32) -> Self {
        let raw_text = text_rows(&recognized(&fragments)).join("\n");
        let confidence = (!fragments.is_empty())
            .then(|| fragments.iter().map(|f| f.confidence).sum::<f32>() / fragments.len() as f32);
        Self {
            engine: engine.to_string(),
            fragments,
            raw_text,
            confidence,
            skew_degrees,
            image_size: None,
        }
    }

    /// The same result, read from an image of this size.
    pub fn with_image_size(self, width: u32, height: u32) -> Self {
        Self {
            image_size: Some((width, height)),
            ..self
        }
    }

    /// The fragments as the receipt parser takes them.
    pub fn recognized(&self) -> Vec<RecognizedText> {
        recognized(&self.fragments)
    }
}

fn recognized(fragments: &[OcrFragment]) -> Vec<RecognizedText> {
    fragments
        .iter()
        .map(|f| RecognizedText {
            text: f.text.clone(),
            bbox: f.bbox,
        })
        .collect()
}

/// Stored form of the boxes in `receipt.ocr_boxes`.
#[derive(Serialize, Deserialize)]
struct StoredBoxes {
    skew_degrees: f32,
    fragments: Vec<StoredFragment>,
    /// `[width, height]`; added in AP-36 without a migration, older rows
    /// lack it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image_size: Option<[u32; 2]>,
}

#[derive(Serialize, Deserialize)]
struct StoredFragment {
    text: String,
    #[serde(rename = "box")]
    bbox: [i32; 4],
    confidence: f32,
}

impl Db {
    /// Archives a new receipt with one image (status `new`) and returns its
    /// id. It stays even if no expense ever uses it: originals are never
    /// discarded on their own (AGENTS.md 7.4).
    pub fn create_receipt(
        &self,
        image_path: &str,
        thumbnail_path: Option<&str>,
    ) -> Result<ReceiptFiles, StorageError> {
        let id = new_id();
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            let now = now_ms();
            tx.execute(
                "INSERT INTO receipt
                     (id, thumbnail_path, status, created_at, updated_at, origin_device_id)
                 VALUES (?1, ?2, 'new', ?3, ?3, ?4)",
                params![id, thumbnail_path, now, self.device_id()],
            )?;
            tx.execute(
                "INSERT INTO receipt_image
                     (id, receipt_id, position, path, created_at, updated_at, origin_device_id)
                 VALUES (?1, ?2, 0, ?3, ?4, ?4, ?5)",
                params![new_id(), id, image_path, now, self.device_id()],
            )?;
            tx.commit()?;
            Ok(())
        })?;
        Ok(ReceiptFiles {
            id,
            image_paths: vec![image_path.to_string()],
            edited_paths: vec![None],
            thumbnail_path: thumbnail_path.map(str::to_string),
        })
    }

    /// Records the corrected copy of the receipt's first page and its new
    /// thumbnail (RCP-05). A recognition of the old image no longer fits,
    /// so the receipt goes back to `new`; the original stays untouched.
    pub fn save_receipt_edit(
        &self,
        id: &str,
        edited_path: &str,
        thumbnail_path: Option<&str>,
    ) -> Result<ReceiptFiles, StorageError> {
        self.with(|conn| {
            let tx = conn.unchecked_transaction()?;
            let now = now_ms();
            let changed = tx.execute(
                "UPDATE receipt
                 SET thumbnail_path = ?2, ocr_raw_text = NULL, ocr_engine = NULL,
                     ocr_confidence = NULL, ocr_boxes = NULL, parsed_at = NULL,
                     status = 'new', updated_at = ?3
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id, thumbnail_path, now],
            )?;
            if changed == 0 {
                return Err(StorageError::NotFound);
            }
            // Several pages come with RCP-06; until then the first one.
            tx.execute(
                "UPDATE receipt_image SET edited_path = ?2, updated_at = ?3
                 WHERE id = (SELECT id FROM receipt_image
                             WHERE receipt_id = ?1 AND deleted_at IS NULL
                             ORDER BY position, id LIMIT 1)",
                params![id, edited_path, now],
            )?;
            tx.commit()?;
            Ok(())
        })?;
        self.receipt(id)?.ok_or(StorageError::NotFound)
    }

    /// The receipt's files, `None` if it does not exist or is deleted.
    pub fn receipt(&self, id: &str) -> Result<Option<ReceiptFiles>, StorageError> {
        self.with(|conn| {
            let Some(thumbnail_path) = conn
                .query_row(
                    "SELECT thumbnail_path FROM receipt WHERE id = ?1 AND deleted_at IS NULL",
                    [id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
            else {
                return Ok(None);
            };
            let mut statement = conn.prepare(
                "SELECT path, edited_path FROM receipt_image
                 WHERE receipt_id = ?1 AND deleted_at IS NULL
                 ORDER BY position, id",
            )?;
            let (image_paths, edited_paths) = statement
                .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<(String, Option<String>)>, _>>()?
                .into_iter()
                .unzip();
            Ok(Some(ReceiptFiles {
                id: id.to_string(),
                image_paths,
                edited_paths,
                thumbnail_path,
            }))
        })
    }
}

impl Db {
    /// Stores the recognized text of a receipt and marks it `analyzed`.
    /// Earlier results are replaced; the image itself stays untouched.
    pub fn save_receipt_text(&self, id: &str, text: &ReceiptText) -> Result<(), StorageError> {
        let boxes = StoredBoxes {
            skew_degrees: text.skew_degrees,
            fragments: text
                .fragments
                .iter()
                .map(|f| StoredFragment {
                    text: f.text.clone(),
                    bbox: [f.bbox.left, f.bbox.top, f.bbox.right, f.bbox.bottom],
                    confidence: f.confidence,
                })
                .collect(),
            image_size: text.image_size.map(|(w, h)| [w, h]),
        };
        let boxes = serde_json::to_string(&boxes)
            .map_err(|_| StorageError::InvalidInput("unserializable OCR result"))?;
        self.with(|conn| {
            let now = now_ms();
            let changed = conn.execute(
                "UPDATE receipt
                 SET ocr_raw_text = ?2, ocr_engine = ?3, ocr_confidence = ?4, ocr_boxes = ?5,
                     parsed_at = ?6, status = 'analyzed', updated_at = ?6
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![
                    id,
                    text.raw_text,
                    text.engine,
                    text.confidence.map(f64::from),
                    boxes,
                    now
                ],
            )?;
            if changed == 0 {
                return Err(StorageError::NotFound);
            }
            Ok(())
        })
    }

    /// The recognized text of a receipt; `None` until it was analyzed.
    pub fn receipt_text(&self, id: &str) -> Result<Option<ReceiptText>, StorageError> {
        let row = self.with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT ocr_engine, ocr_boxes, ocr_raw_text, ocr_confidence FROM receipt
                     WHERE id = ?1 AND deleted_at IS NULL AND ocr_boxes IS NOT NULL",
                    [id],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, Option<f64>>(3)?,
                        ))
                    },
                )
                .optional()?)
        })?;
        let Some((engine, boxes, raw_text, confidence)) = row else {
            return Ok(None);
        };
        let stored: StoredBoxes = serde_json::from_str(&boxes)
            .map_err(|_| StorageError::InvalidInput("unreadable OCR result"))?;
        Ok(Some(ReceiptText {
            engine: engine.unwrap_or_default(),
            fragments: stored
                .fragments
                .into_iter()
                .map(|f| OcrFragment {
                    text: f.text,
                    bbox: BoundingBox {
                        left: f.bbox[0],
                        top: f.bbox[1],
                        right: f.bbox[2],
                        bottom: f.bbox[3],
                    },
                    confidence: f.confidence,
                })
                .collect(),
            raw_text: raw_text.unwrap_or_default(),
            confidence: confidence.map(|c| c as f32),
            skew_degrees: stored.skew_degrees,
            image_size: stored.image_size.map(|[w, h]| (w, h)),
        }))
    }
}

/// Marks an analyzed receipt as checked by the user (status `reviewed`,
/// idee.md 4.1 "geprüft"); a receipt never analyzed stays as it is.
pub(super) fn mark_reviewed(conn: &Connection, id: &str, now: i64) -> Result<(), StorageError> {
    conn.execute(
        "UPDATE receipt SET status = 'reviewed', updated_at = ?2
         WHERE id = ?1 AND status = 'analyzed' AND deleted_at IS NULL",
        params![id, now],
    )?;
    Ok(())
}

/// A new expense may only take a receipt that exists and no other expense
/// holds; one receipt belongs to one expense (idee.md 4.2).
pub(super) fn check_unattached(conn: &Connection, id: &str) -> Result<(), StorageError> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM receipt WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(StorageError::NotFound);
    }
    let taken = conn
        .query_row(
            "SELECT 1 FROM expense WHERE receipt_id = ?1 AND deleted_at IS NULL",
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if taken {
        return Err(StorageError::InvalidInput(
            "the receipt belongs to another expense",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_a_receipt_with_its_image() {
        let db = Db::open_in_memory().unwrap();
        let receipt = db
            .create_receipt("receipts/a.jpg", Some("receipts/a_thumb.jpg"))
            .unwrap();
        assert_eq!(db.receipt(&receipt.id).unwrap(), Some(receipt.clone()));
        assert_eq!(receipt.image_paths, ["receipts/a.jpg"]);

        let without_preview = db.create_receipt("receipts/b.heic", None).unwrap();
        assert_eq!(
            db.receipt(&without_preview.id)
                .unwrap()
                .unwrap()
                .thumbnail_path,
            None
        );
        assert_eq!(db.receipt("missing").unwrap(), None);
    }

    #[test]
    fn stores_and_reads_the_recognized_text() {
        let db = Db::open_in_memory().unwrap();
        let receipt = db.create_receipt("receipts/a.jpg", None).unwrap();
        let fragment = |text: &str, left, top| OcrFragment {
            text: text.to_string(),
            bbox: BoundingBox {
                left,
                top,
                right: left + 100,
                bottom: top + 20,
            },
            confidence: 0.75,
        };
        let text = ReceiptText::new(
            "test",
            vec![
                fragment("1,99", 300, 102),
                fragment("Milch", 10, 100),
                fragment("SUMME", 10, 150),
            ],
            -2.5,
        )
        .with_image_size(400, 900);
        assert_eq!(text.raw_text, "Milch 1,99\nSUMME");
        assert_eq!(text.confidence, Some(0.75));
        db.save_receipt_text(&receipt.id, &text).unwrap();
        assert_eq!(db.receipt_text(&receipt.id).unwrap(), Some(text));
        let status: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT status FROM receipt WHERE id = ?1",
                    [&receipt.id],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(status, "analyzed");

        // Nothing recognized is a result too, unlike "not analyzed yet".
        let blank = db.create_receipt("receipts/b.jpg", None).unwrap();
        assert_eq!(db.receipt_text(&blank.id).unwrap(), None);
        let empty = ReceiptText::new("test", Vec::new(), 0.0);
        assert_eq!(empty.confidence, None);
        db.save_receipt_text(&blank.id, &empty).unwrap();
        assert_eq!(db.receipt_text(&blank.id).unwrap(), Some(empty.clone()));

        // Results stored before AP-36 know no image size.
        db.with(|conn| {
            Ok(conn.execute(
                "UPDATE receipt SET ocr_boxes = '{\"skew_degrees\":0.0,\"fragments\":[]}'
                 WHERE id = ?1",
                [&blank.id],
            )?)
        })
        .unwrap();
        assert_eq!(db.receipt_text(&blank.id).unwrap(), Some(empty.clone()));
        assert!(matches!(
            db.save_receipt_text("missing", &empty),
            Err(StorageError::NotFound)
        ));
    }

    #[test]
    fn an_edit_keeps_the_original_and_drops_the_old_recognition() {
        let db = Db::open_in_memory().unwrap();
        let receipt = db
            .create_receipt("receipts/a.jpg", Some("receipts/a_thumb.jpg"))
            .unwrap();
        assert_eq!(receipt.page(0), Some("receipts/a.jpg"));
        db.save_receipt_text(&receipt.id, &ReceiptText::new("test", Vec::new(), 0.0))
            .unwrap();

        let edited = db
            .save_receipt_edit(
                &receipt.id,
                "receipts/a_edited.jpg",
                Some("receipts/a_edited_thumb.jpg"),
            )
            .unwrap();
        assert_eq!(edited.image_paths, ["receipts/a.jpg"]);
        assert_eq!(
            edited.edited_paths,
            [Some("receipts/a_edited.jpg".to_string())]
        );
        assert_eq!(edited.page(0), Some("receipts/a_edited.jpg"));
        assert_eq!(
            edited.thumbnail_path.as_deref(),
            Some("receipts/a_edited_thumb.jpg")
        );
        assert_eq!(db.receipt(&receipt.id).unwrap(), Some(edited));
        assert_eq!(db.receipt_text(&receipt.id).unwrap(), None);
        assert!(matches!(
            db.save_receipt_edit("missing", "receipts/x.jpg", None),
            Err(StorageError::NotFound)
        ));
    }

    #[test]
    fn only_unattached_receipts_can_be_attached() {
        let db = Db::open_in_memory().unwrap();
        let receipt = db.create_receipt("receipts/a.jpg", None).unwrap();
        db.with(|conn| check_unattached(conn, &receipt.id)).unwrap();
        assert!(matches!(
            db.with(|conn| check_unattached(conn, "missing")),
            Err(StorageError::NotFound)
        ));
    }
}
