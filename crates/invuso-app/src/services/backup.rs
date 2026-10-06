//! Full backup as one ZIP file and restoring it (DATA-01, DATA-02), and
//! handing exports to the system's "save as" dialog (DATA-03).
//!
//! The archive holds the database and every receipt image; translation
//! packs are left out, they can be downloaded again (user decision in
//! AP-26). Files are built in the app's private `transfer` folder and only
//! copied out (or in) by the platform's document dialog.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::clock;
use crate::platform::{self, DocumentFiles, DocumentOutcome};
use crate::services::receipts::RECEIPTS_DIR;
use crate::storage::{Db, StorageError, now_ms};

/// Private folder for files on their way out of or into the app.
const TRANSFER_DIR: &str = "transfer";
/// Names inside the archive.
const MANIFEST_ENTRY: &str = "invuso-backup.json";
const DATABASE_ENTRY: &str = "invuso.sqlite3";
/// Version of the archive layout; a newer one is refused.
const FORMAT: u32 = 1;

/// Media types the open dialog offers for a backup; providers label a zip
/// differently.
const BACKUP_TYPES: [&str; 3] = [
    "application/zip",
    "application/x-zip-compressed",
    "application/octet-stream",
];

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("{0}")]
    Platform(String),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("the file is not an Invuso backup")]
    NotABackup,
    /// Made by a newer app version.
    #[error("backup format {0} is newer than this app supports")]
    TooNew(u32),
}

impl BackupError {
    /// Whether the chosen file is no backup at all, as opposed to one this
    /// app is too old for.
    pub fn is_not_a_backup(&self) -> bool {
        matches!(
            self,
            Self::NotABackup | Self::Zip(_) | Self::Storage(StorageError::NotABackup)
        )
    }

    pub fn is_too_new(&self) -> bool {
        matches!(
            self,
            Self::TooNew(_) | Self::Storage(StorageError::SchemaTooNew { .. })
        )
    }
}

/// What the archive says about itself.
#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    format: u32,
    app_version: String,
    /// Unix milliseconds.
    created_at: i64,
}

/// Builds a backup and lets the user save it. `false` if the user backed
/// out of the dialog.
pub async fn save_backup(db: Db) -> Result<bool, BackupError> {
    let data_dir = platform::data_dir().map_err(BackupError::Platform)?;
    let name = format!("invuso-backup-{}.zip", clock::local_now().0);
    let archive = fresh_transfer_file(&data_dir, &name)?;
    let written = archive.clone();
    // Compressing the database and copying the photos takes a moment.
    tokio::task::spawn_blocking(move || write_archive(&db, &data_dir, &written))
        .await
        .map_err(|e| BackupError::Platform(e.to_string()))??;
    save_document(&archive, &name, "application/zip").await
}

/// Lets the user pick a backup and replaces all data with it. `false` if
/// the user backed out of the dialog; then nothing changed.
pub async fn restore_backup(db: Db) -> Result<bool, BackupError> {
    let data_dir = platform::data_dir().map_err(BackupError::Platform)?;
    let archive = fresh_transfer_file(&data_dir, "restore.zip")?;
    let outcome = platform::document_files()
        .open(&archive, &BACKUP_TYPES)
        .await
        .map_err(BackupError::Platform);
    if !matches!(outcome, Ok(DocumentOutcome::Done)) {
        let _ = std::fs::remove_file(&archive);
        return outcome.map(|_| false);
    }
    let read = archive.clone();
    let restored = tokio::task::spawn_blocking(move || restore_archive(&db, &data_dir, &read))
        .await
        .map_err(|e| BackupError::Platform(e.to_string()));
    let _ = std::fs::remove_file(&archive);
    restored??;
    Ok(true)
}

/// Writes `contents` to a file called `name` and lets the user save it,
/// e.g. a group's CSV (DATA-03). `false` if the user backed out.
pub async fn save_text(name: String, mime: &str, contents: String) -> Result<bool, BackupError> {
    let data_dir = platform::data_dir().map_err(BackupError::Platform)?;
    let file = fresh_transfer_file(&data_dir, &name)?;
    std::fs::write(&file, contents)?;
    save_document(&file, &name, mime).await
}

/// Whether the platform has the dialogs the backup screen needs.
pub fn supported() -> bool {
    platform::document_files().supported()
}

/// Hands `file` to the save dialog and removes it afterwards, saved or not.
async fn save_document(file: &Path, name: &str, mime: &str) -> Result<bool, BackupError> {
    let outcome = platform::document_files()
        .save(file, name, mime)
        .await
        .map_err(BackupError::Platform);
    let _ = std::fs::remove_file(file);
    Ok(outcome? == DocumentOutcome::Done)
}

/// `transfer/<name>`, with leftovers of earlier runs removed: they only
/// ever hold copies.
fn fresh_transfer_file(data_dir: &Path, name: &str) -> Result<PathBuf, BackupError> {
    let dir = data_dir.join(TRANSFER_DIR);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(name))
}

/// Writes the backup archive to `dest`: manifest, database snapshot and the
/// receipt images. Returns the number of images.
fn write_archive(db: &Db, data_dir: &Path, dest: &Path) -> Result<usize, BackupError> {
    let snapshot = dest.with_extension("sqlite3");
    db.snapshot_to(&snapshot)?;
    let written = write_entries(data_dir, &snapshot, dest);
    let _ = std::fs::remove_file(&snapshot);
    written
}

fn write_entries(data_dir: &Path, snapshot: &Path, dest: &Path) -> Result<usize, BackupError> {
    let mut zip = ZipWriter::new(File::create(dest)?);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    // Photos are compressed already; deflating them again only costs time.
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    let manifest = Manifest {
        format: FORMAT,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: now_ms(),
    };
    zip.start_file(MANIFEST_ENTRY, deflated)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest).map_err(std::io::Error::other)?)?;

    zip.start_file(DATABASE_ENTRY, deflated)?;
    std::io::copy(&mut File::open(snapshot)?, &mut zip)?;

    let mut images = 0;
    let receipts = data_dir.join(RECEIPTS_DIR);
    if receipts.is_dir() {
        let mut names: Vec<String> = std::fs::read_dir(&receipts)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_image_name(name))
            .collect();
        names.sort();
        for name in names {
            zip.start_file(format!("{RECEIPTS_DIR}/{name}"), stored)?;
            std::io::copy(&mut File::open(receipts.join(&name))?, &mut zip)?;
            images += 1;
        }
    }
    zip.finish()?;
    Ok(images)
}

/// Replaces the data with the backup at `source`: the database is checked
/// first, then the images are added, then the records are replaced.
/// Images already on the device stay (idee.md 1.4: originals are never
/// thrown away automatically); an image with the same name is the same
/// receipt and is kept as it is.
fn restore_archive(db: &Db, data_dir: &Path, source: &Path) -> Result<(), BackupError> {
    let mut zip = ZipArchive::new(File::open(source)?)?;

    let manifest: Manifest = {
        let mut entry = zip
            .by_name(MANIFEST_ENTRY)
            .map_err(|_| BackupError::NotABackup)?;
        let mut text = Vec::new();
        entry.read_to_end(&mut text)?;
        serde_json::from_slice(&text).map_err(|_| BackupError::NotABackup)?
    };
    if manifest.format > FORMAT {
        return Err(BackupError::TooNew(manifest.format));
    }

    let database = source.with_extension("sqlite3");
    let result = (|| {
        {
            let mut entry = zip
                .by_name(DATABASE_ENTRY)
                .map_err(|_| BackupError::NotABackup)?;
            std::io::copy(&mut entry, &mut File::create(&database)?)?;
        }
        Db::check_backup(&database)?;
        restore_images(&mut zip, &data_dir.join(RECEIPTS_DIR))?;
        db.replace_with(&database)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&database);
    result
}

fn restore_images(zip: &mut ZipArchive<File>, receipts: &Path) -> Result<(), BackupError> {
    std::fs::create_dir_all(receipts)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        if !entry.is_file() {
            continue;
        }
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let Some(name) = image_entry_name(&path) else {
            continue;
        };
        let target = receipts.join(&name);
        if target.exists() {
            continue;
        }
        // Written beside the target first, so an interrupted restore never
        // leaves a truncated image under a receipt's name.
        let staged = receipts.join(format!("{name}.part"));
        std::io::copy(&mut entry, &mut File::create(&staged)?)?;
        std::fs::rename(&staged, &target)?;
    }
    Ok(())
}

/// The file name of an archive entry `receipts/<name>`; anything else (other
/// folders, nested paths, half-written files) is not restored.
fn image_entry_name(path: &Path) -> Option<String> {
    let mut parts = path.components();
    let folder = parts.next()?.as_os_str().to_str()?;
    let name = parts.next()?.as_os_str().to_str()?;
    (folder == RECEIPTS_DIR && parts.next().is_none() && is_image_name(name))
        .then(|| name.to_string())
}

/// Receipt files are `<uuid>.<ext>` and `<uuid>_thumb.jpg`; `.part` files
/// are captures still being written.
fn is_image_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.ends_with(".part")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::NewPerson;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("invuso-archive-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn person(db: &Db, name: &str) {
        db.create_person(NewPerson {
            name: name.into(),
            color: "thistle".into(),
            is_me: false,
            note: None,
        })
        .unwrap();
    }

    fn names(db: &Db) -> Vec<String> {
        let mut names: Vec<String> = db.people().unwrap().into_iter().map(|p| p.name).collect();
        names.sort();
        names
    }

    #[test]
    fn a_backup_restores_records_and_images_on_another_device() {
        let phone = temp_dir();
        let db = Db::open(&phone.join("invuso.sqlite3")).unwrap();
        person(&db, "Anna");
        std::fs::create_dir_all(phone.join("receipts")).unwrap();
        std::fs::write(phone.join("receipts/r1.jpg"), b"photo").unwrap();
        std::fs::write(phone.join("receipts/r1_thumb.jpg"), b"thumb").unwrap();
        std::fs::write(phone.join("receipts/r2.part"), b"half").unwrap();
        let archive = phone.join("backup.zip");
        assert_eq!(write_archive(&db, &phone, &archive).unwrap(), 2);
        assert!(!phone.join("backup.sqlite3").exists());

        let other = temp_dir();
        let target = Db::open(&other.join("invuso.sqlite3")).unwrap();
        person(&target, "Ben");
        std::fs::create_dir_all(other.join("receipts")).unwrap();
        std::fs::write(other.join("receipts/old.jpg"), b"kept").unwrap();
        restore_archive(&target, &other, &archive).unwrap();

        assert_eq!(names(&target), ["Anna"]);
        assert_eq!(
            std::fs::read(other.join("receipts/r1.jpg")).unwrap(),
            b"photo"
        );
        assert_eq!(
            std::fs::read(other.join("receipts/r1_thumb.jpg")).unwrap(),
            b"thumb"
        );
        assert!(!other.join("receipts/r2.part").exists());
        assert_eq!(
            std::fs::read(other.join("receipts/old.jpg")).unwrap(),
            b"kept"
        );
        assert!(!other.join("backup.sqlite3").exists());
        let _ = std::fs::remove_dir_all(phone);
        let _ = std::fs::remove_dir_all(other);
    }

    #[test]
    fn other_files_change_nothing() {
        let dir = temp_dir();
        let db = Db::open(&dir.join("invuso.sqlite3")).unwrap();
        person(&db, "Anna");

        let text = dir.join("notes.zip");
        std::fs::write(&text, "no zip").unwrap();
        assert!(
            restore_archive(&db, &dir, &text)
                .unwrap_err()
                .is_not_a_backup()
        );

        // A zip without manifest.
        let plain = dir.join("plain.zip");
        let mut zip = ZipWriter::new(File::create(&plain).unwrap());
        zip.start_file("receipts/x.jpg", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
        assert!(
            restore_archive(&db, &dir, &plain)
                .unwrap_err()
                .is_not_a_backup()
        );

        // A manifest from a newer app.
        let newer = dir.join("newer.zip");
        let mut zip = ZipWriter::new(File::create(&newer).unwrap());
        zip.start_file(MANIFEST_ENTRY, SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"format": 2, "app_version": "9", "created_at": 0}"#)
            .unwrap();
        zip.finish().unwrap();
        assert!(restore_archive(&db, &dir, &newer).unwrap_err().is_too_new());

        // A database that is no Invuso database: no image is added.
        let foreign = dir.join("foreign.zip");
        let mut zip = ZipWriter::new(File::create(&foreign).unwrap());
        zip.start_file(MANIFEST_ENTRY, SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"format": 1, "app_version": "1", "created_at": 0}"#)
            .unwrap();
        zip.start_file(DATABASE_ENTRY, SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"not sqlite").unwrap();
        zip.start_file("receipts/x.jpg", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
        assert!(
            restore_archive(&db, &dir, &foreign)
                .unwrap_err()
                .is_not_a_backup()
        );
        assert!(!dir.join("receipts/x.jpg").exists());

        assert_eq!(names(&db), ["Anna"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_flat_receipt_entries_are_restored() {
        let name = |path: &str| image_entry_name(Path::new(path));
        assert_eq!(name("receipts/a.jpg").as_deref(), Some("a.jpg"));
        assert_eq!(name("receipts/a_thumb.jpg").as_deref(), Some("a_thumb.jpg"));
        assert_eq!(name("receipts/a.jpg.part"), None);
        assert_eq!(name("receipts/sub/a.jpg"), None);
        assert_eq!(name("translation/model.rten"), None);
        assert_eq!(name("invuso.sqlite3"), None);
    }
}
