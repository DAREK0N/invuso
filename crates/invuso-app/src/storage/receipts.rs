use rusqlite::{Connection, OptionalExtension, params};

use super::db::{new_id, now_ms};
use super::{Db, StorageError};

/// An archived receipt image as the app shows it (RCP-03, RCP-04). Paths
/// are relative to the data directory, so a backup can move them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptFiles {
    pub id: String,
    /// Originals in page order (several only with RCP-06).
    pub image_paths: Vec<String>,
    /// `None` if the image could not be decoded for a preview.
    pub thumbnail_path: Option<String>,
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
            thumbnail_path: thumbnail_path.map(str::to_string),
        })
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
                "SELECT path FROM receipt_image
                 WHERE receipt_id = ?1 AND deleted_at IS NULL
                 ORDER BY position, id",
            )?;
            let image_paths = statement
                .query_map([id], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            Ok(Some(ReceiptFiles {
                id: id.to_string(),
                image_paths,
                thumbnail_path,
            }))
        })
    }
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
