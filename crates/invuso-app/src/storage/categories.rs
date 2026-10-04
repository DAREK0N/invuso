use invuso_core::domain::{Category, CategoryId};
use rusqlite::{Connection, OptionalExtension};

use super::{Db, StorageError};

impl Db {
    /// All categories in display order (EXP-01); the defaults come from
    /// migration 0002.
    pub fn categories(&self) -> Result<Vec<Category>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(
                "SELECT id, name, icon, color, is_default FROM category
                 WHERE deleted_at IS NULL ORDER BY sort_order, name COLLATE NOCASE, id",
            )?;
            let categories = statement
                .query_map([], |row| {
                    Ok(Category {
                        id: CategoryId::new(row.get::<_, String>(0)?),
                        name: row.get(1)?,
                        icon: row.get(2)?,
                        color: row.get(3)?,
                        is_default: row.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(categories)
        })
    }
}

/// The foreign key accepts deleted categories; a new expense needs a live one.
pub(super) fn check_category(conn: &Connection, id: &CategoryId) -> Result<(), StorageError> {
    conn.query_row(
        "SELECT 1 FROM category WHERE id = ?1 AND deleted_at IS NULL",
        [id.as_str()],
        |_| Ok(()),
    )
    .optional()?
    .ok_or(StorageError::InvalidInput("category does not exist"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_seeded_in_order() {
        let db = Db::open_in_memory().unwrap();
        let categories = db.categories().unwrap();
        let names: Vec<_> = categories.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "food",
                "groceries",
                "transport",
                "lodging",
                "activities",
                "shopping",
                "health",
                "other"
            ]
        );
        assert!(categories.iter().all(|c| c.is_default));
        assert_eq!(categories[0].id, CategoryId::new("default-food"));
    }
}
