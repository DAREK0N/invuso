use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use thiserror::Error;

use super::{migrations, settings};

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("cannot create the data directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("database schema version {found} is newer than this app supports ({supported})")]
    SchemaTooNew { found: u32, supported: u32 },
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),
    #[error("invalid payment method: {0}")]
    PaymentMethod(#[from] invuso_core::domain::PaymentMethodError),
    #[error("invalid expense: {0}")]
    Expense(#[from] invuso_core::domain::ExpenseError),
    #[error("invalid group: {0}")]
    Group(#[from] invuso_core::domain::GroupError),
    #[error("invalid settlement: {0}")]
    Settlement(#[from] invuso_core::domain::SettlementError),
    #[error("invalid cash movement: {0}")]
    Cash(#[from] invuso_core::domain::CashError),
    #[error("the person is already a member of the group")]
    AlreadyMember,
    #[error("record not found")]
    NotFound,
    #[error("\"Ich\" cannot be deleted")]
    CannotDeleteMe,
    #[error("\"Ich\" cannot leave a group")]
    CannotRemoveMe,
    #[error("database lock poisoned")]
    Poisoned,
}

/// Handle to the app database, shared through the Dioxus context.
///
/// One connection behind a mutex: queries are short and the UI is the only
/// writer; background work (rates, OCR) can still use it from other threads.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    device_id: Arc<str>,
}

impl PartialEq for Db {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.conn, &other.conn)
    }
}

impl Db {
    /// Opens (or creates) the database file and brings the schema up to date.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    /// Fresh in-memory database, for tests.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, StorageError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self, StorageError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrations::run(&mut conn)?;
        let device_id = settings::ensure_device_id(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            device_id: device_id.into(),
        })
    }

    /// Stable id of this installation; stored on every record it creates
    /// (`origin_device_id`, CORE-12).
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub(super) fn with<T>(
        &self,
        op: impl FnOnce(&Connection) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        let conn = self.conn.lock().map_err(|_| StorageError::Poisoned)?;
        op(&conn)
    }
}

/// New record id: UUIDv7, so ids sort by creation time.
pub(super) fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Current time as Unix milliseconds (`created_at`, `updated_at`, `deleted_at`).
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
