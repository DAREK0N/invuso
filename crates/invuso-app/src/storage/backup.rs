//! Copying the whole database out and back in (DATA-01, DATA-02). The
//! archive around it, with the receipt images, is built in
//! `services::backup`.

use std::path::Path;

use rusqlite::{Connection, MAIN_DB, OpenFlags};

use super::{Db, StorageError, migrations, settings};

impl Db {
    /// Writes a consistent, compacted copy of the database to `dest`
    /// (DATA-01). An existing file at `dest` is replaced.
    pub fn snapshot_to(&self, dest: &Path) -> Result<(), StorageError> {
        let dest_text = dest
            .to_str()
            .ok_or(StorageError::InvalidInput("backup path is not UTF-8"))?;
        // VACUUM INTO refuses to overwrite.
        match std::fs::remove_file(dest) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        self.with(|conn| {
            conn.execute("VACUUM INTO ?1", [dest_text])?;
            Ok(())
        })
    }

    /// Replaces every record with the database at `source` (DATA-02, user
    /// decision in AP-26: restoring replaces, it does not merge). An older
    /// backup is migrated; a newer one or a file that is no Invuso database
    /// is refused before anything changes. This installation keeps its own
    /// device id (CORE-12), whichever device made the backup.
    pub fn replace_with(&self, source: &Path) -> Result<(), StorageError> {
        check_backup(source)?;
        let mut conn = self.conn.lock().map_err(|_| StorageError::Poisoned)?;
        conn.restore(MAIN_DB, source, None::<fn(rusqlite::backup::Progress)>)?;
        migrations::run(&mut conn)?;
        settings::set(&conn, settings::DEVICE_ID, &self.device_id)?;
        Ok(())
    }
}

impl Db {
    /// Whether `source` is an intact Invuso database this app can read,
    /// without touching the open database.
    pub fn check_backup(source: &Path) -> Result<(), StorageError> {
        check_backup(source)
    }
}

fn check_backup(source: &Path) -> Result<(), StorageError> {
    let conn = Connection::open_with_flags(
        source,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| StorageError::NotABackup)?;
    // A file that is no SQLite database at all fails on the first read.
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| StorageError::NotABackup)?;
    let has_expenses: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'expense')",
            [],
            |row| row.get(0),
        )
        .map_err(|_| StorageError::NotABackup)?;
    if version == 0 || !has_expenses {
        return Err(StorageError::NotABackup);
    }
    let supported = migrations::supported();
    if version > supported {
        return Err(StorageError::SchemaTooNew {
            found: version,
            supported,
        });
    }
    let check: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if check != "ok" {
        return Err(StorageError::NotABackup);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::storage::NewPerson;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("invuso-backup-{}", uuid::Uuid::now_v7()));
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
    fn a_snapshot_restores_the_data_and_keeps_the_device_id() {
        let dir = temp_dir();
        let source = Db::open(&dir.join("a.sqlite3")).unwrap();
        person(&source, "Anna");
        source.snapshot_to(&dir.join("snapshot.sqlite3")).unwrap();
        // A second snapshot overwrites the first.
        person(&source, "Ben");
        source.snapshot_to(&dir.join("snapshot.sqlite3")).unwrap();

        let target = Db::open(&dir.join("b.sqlite3")).unwrap();
        person(&target, "Clara");
        let device = target.device_id().to_string();
        assert_ne!(device, source.device_id());

        target.replace_with(&dir.join("snapshot.sqlite3")).unwrap();
        assert_eq!(names(&target), ["Anna", "Ben"]);
        assert_eq!(target.setting(settings::DEVICE_ID).unwrap(), Some(device));

        // Still a working WAL database after reopening.
        drop(target);
        let reopened = Db::open(&dir.join("b.sqlite3")).unwrap();
        assert_eq!(names(&reopened), ["Anna", "Ben"]);
        person(&reopened, "Dora");
        assert_eq!(names(&reopened), ["Anna", "Ben", "Dora"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_older_backup_is_migrated() {
        let dir = temp_dir();
        let old = dir.join("old.sqlite3");
        {
            let conn = Connection::open(&old).unwrap();
            conn.execute_batch(include_str!("../../migrations/0001_initial.sql"))
                .unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        let db = Db::open_in_memory().unwrap();
        db.replace_with(&old).unwrap();
        let version: u32 = db
            .with(|conn| Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?))
            .unwrap();
        assert_eq!(version, migrations::supported());
        // Migration 0002 added the default categories.
        assert!(!db.categories().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn foreign_and_newer_files_change_nothing() {
        let dir = temp_dir();
        let db = Db::open_in_memory().unwrap();
        person(&db, "Anna");

        let text = dir.join("notes.txt");
        std::fs::write(&text, "not a database").unwrap();
        assert!(matches!(
            db.replace_with(&text),
            Err(StorageError::NotABackup)
        ));

        let other = dir.join("other.sqlite3");
        Connection::open(&other)
            .unwrap()
            .execute_batch("CREATE TABLE notes (text TEXT); PRAGMA user_version = 3;")
            .unwrap();
        assert!(matches!(
            db.replace_with(&other),
            Err(StorageError::NotABackup)
        ));

        let newer = dir.join("newer.sqlite3");
        db.snapshot_to(&newer).unwrap();
        Connection::open(&newer)
            .unwrap()
            .pragma_update(None, "user_version", 999)
            .unwrap();
        assert!(matches!(
            db.replace_with(&newer),
            Err(StorageError::SchemaTooNew { found: 999, .. })
        ));

        assert_eq!(names(&db), ["Anna"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
