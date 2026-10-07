use rusqlite::Connection;

use super::StorageError;

/// Schema migrations in order; migration `n` (1-based) brings the database
/// to `PRAGMA user_version = n`. Existing entries must never be edited —
/// schema changes always get a new file (AGENTS.md 7.4).
const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_initial.sql"),
    include_str!("../../migrations/0002_default_categories.sql"),
    include_str!("../../migrations/0003_receipt_ocr_boxes.sql"),
    include_str!("../../migrations/0004_cash_movement_card.sql"),
    include_str!("../../migrations/0005_expense_coordinates.sql"),
    include_str!("../../migrations/0006_receipt_image_edited.sql"),
    include_str!("../../migrations/0007_line_item_belongs_to.sql"),
];

/// Schema version of a fully migrated database.
pub(super) fn supported() -> u32 {
    MIGRATIONS.len() as u32
}

/// Applies every migration the database has not seen yet, each in its own
/// transaction.
pub(super) fn run(conn: &mut Connection) -> Result<(), StorageError> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let supported = supported();
    if current > supported {
        return Err(StorageError::SchemaTooNew {
            found: current,
            supported,
        });
    }

    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", index as u32 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_version(conn: &Connection) -> u32 {
        conn.pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn migrates_fresh_database_and_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len() as u32);
        run(&mut conn).unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len() as u32);
    }

    #[test]
    fn refuses_newer_schema() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        assert!(matches!(
            run(&mut conn),
            Err(StorageError::SchemaTooNew { found: 999, .. })
        ));
    }

    #[test]
    fn every_synced_table_has_sync_columns() {
        let mut conn = Connection::open_in_memory().unwrap();
        run(&mut conn).unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT IN ('settings', 'translation_cache')")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(!tables.is_empty());
        for table in tables {
            let columns: Vec<String> = conn
                .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            for required in [
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "origin_device_id",
                "created_by",
                "updated_by",
            ] {
                assert!(
                    columns.iter().any(|c| c == required),
                    "{table} lacks {required}"
                );
            }
        }
    }
}
