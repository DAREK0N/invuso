use invuso_core::domain::{Category, CategoryId};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::db::{new_id, now_ms};
use super::{Db, StorageError, people};

/// Input for creating a category of the user's own (EXP-09).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCategory {
    pub name: String,
    pub icon: String,
    pub color: String,
}

const COLUMNS: &str = "id, name, icon, color, is_default";
const ORDER: &str = "ORDER BY sort_order, name COLLATE NOCASE, id";

impl Db {
    /// All categories in display order (EXP-01); the defaults come from
    /// migration 0002.
    pub fn categories(&self) -> Result<Vec<Category>, StorageError> {
        self.query_categories(&format!(
            "SELECT {COLUMNS} FROM category WHERE deleted_at IS NULL {ORDER}"
        ))
    }

    /// Every category including deleted ones: expenses keep showing the
    /// category they were filed under (EXP-09, idee.md 1.4).
    pub fn all_categories(&self) -> Result<Vec<Category>, StorageError> {
        self.query_categories(&format!("SELECT {COLUMNS} FROM category {ORDER}"))
    }

    /// Default categories the user has hidden; they can be shown again.
    pub fn hidden_categories(&self) -> Result<Vec<Category>, StorageError> {
        self.query_categories(&format!(
            "SELECT {COLUMNS} FROM category
             WHERE deleted_at IS NOT NULL AND is_default = 1 {ORDER}"
        ))
    }

    /// Adds a category of the user's own after all others.
    pub fn create_category(&self, new: NewCategory) -> Result<Category, StorageError> {
        let category = Category {
            id: CategoryId::new(new_id()),
            name: people::valid_name(&new.name)?,
            icon: new.icon,
            color: new.color,
            is_default: false,
        };
        self.with(|conn| {
            conn.execute(
                "INSERT INTO category
                     (id, name, icon, color, is_default, sort_order, created_at, updated_at,
                      origin_device_id)
                 VALUES (?1, ?2, ?3, ?4, 0,
                         (SELECT coalesce(max(sort_order), 0) + 1 FROM category), ?5, ?5, ?6)",
                params![
                    category.id.as_str(),
                    category.name,
                    category.icon,
                    category.color,
                    now_ms(),
                    self.device_id()
                ],
            )?;
            Ok(())
        })?;
        Ok(category)
    }

    /// Saves name, icon and color. A default category keeps its translated
    /// name only while `is_default` stays set; the caller clears it when the
    /// user types a name of their own (user decision in AP-28). A category
    /// never becomes a default one.
    pub fn update_category(&self, category: &Category) -> Result<(), StorageError> {
        let name = people::valid_name(&category.name)?;
        let changed = self.with(|conn| {
            Ok(conn.execute(
                "UPDATE category
                 SET name = ?2, icon = ?3, color = ?4, is_default = is_default AND ?5,
                     updated_at = ?6
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![
                    category.id.as_str(),
                    name,
                    category.icon,
                    category.color,
                    category.is_default,
                    now_ms()
                ],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Soft delete; for a default category this is "hide". Expenses keep
    /// pointing at the row (idee.md 4).
    pub fn delete_category(&self, id: &CategoryId) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            Ok(conn.execute(
                "UPDATE category SET deleted_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND deleted_at IS NULL",
                params![id.as_str(), now_ms()],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    /// Undoes [`Db::delete_category`]: the undo toast (UI-11), or showing a
    /// hidden default category again.
    pub fn restore_category(&self, id: &CategoryId) -> Result<(), StorageError> {
        let changed = self.with(|conn| {
            Ok(conn.execute(
                "UPDATE category SET deleted_at = NULL, updated_at = ?2
                 WHERE id = ?1 AND deleted_at IS NOT NULL",
                params![id.as_str(), now_ms()],
            )?)
        })?;
        if changed == 0 {
            return Err(StorageError::NotFound);
        }
        Ok(())
    }

    fn query_categories(&self, sql: &str) -> Result<Vec<Category>, StorageError> {
        self.with(|conn| {
            let mut statement = conn.prepare(sql)?;
            let categories = statement
                .query_map([], category_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(categories)
        })
    }
}

/// The foreign key accepts deleted categories; an expense needs a live one,
/// unless it already had this category before it was deleted (`kept`).
pub(super) fn check_category(
    conn: &Connection,
    id: &CategoryId,
    kept: Option<&str>,
) -> Result<(), StorageError> {
    if kept == Some(id.as_str()) {
        return Ok(());
    }
    conn.query_row(
        "SELECT 1 FROM category WHERE id = ?1 AND deleted_at IS NULL",
        [id.as_str()],
        |_| Ok(()),
    )
    .optional()?
    .ok_or(StorageError::InvalidInput("category does not exist"))
}

fn category_from_row(row: &Row<'_>) -> rusqlite::Result<Category> {
    Ok(Category {
        id: CategoryId::new(row.get::<_, String>(0)?),
        name: row.get(1)?,
        icon: row.get(2)?,
        color: row.get(3)?,
        is_default: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(name: &str) -> NewCategory {
        NewCategory {
            name: name.into(),
            icon: "gift".into(),
            color: "thistle".into(),
        }
    }

    fn names(categories: &[Category]) -> Vec<&str> {
        categories.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn defaults_are_seeded_in_order() {
        let db = Db::open_in_memory().unwrap();
        let categories = db.categories().unwrap();
        assert_eq!(
            names(&categories),
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

    #[test]
    fn own_categories_come_after_the_defaults() {
        let db = Db::open_in_memory().unwrap();
        let souvenirs = db.create_category(new("  Souvenirs ")).unwrap();
        let bakery = db.create_category(new("Bäckerei")).unwrap();
        assert_eq!(souvenirs.name, "Souvenirs");
        assert!(!souvenirs.is_default);
        let categories = db.categories().unwrap();
        assert_eq!(names(&categories[8..]), ["Souvenirs", "Bäckerei"]);
        assert_eq!(categories[9], bakery);
    }

    #[test]
    fn update_changes_name_icon_and_color() {
        let db = Db::open_in_memory().unwrap();
        let souvenirs = db.create_category(new("Souvenirs")).unwrap();
        let updated = Category {
            name: "Mitbringsel".into(),
            icon: "shirt".into(),
            color: "pale-oak".into(),
            ..souvenirs
        };
        db.update_category(&updated).unwrap();
        assert_eq!(db.categories().unwrap()[8], updated);
    }

    #[test]
    fn renamed_default_keeps_its_name_and_stays_put() {
        let db = Db::open_in_memory().unwrap();
        let food = db.categories().unwrap().remove(0);
        // Icon and color only: still a translated default.
        db.update_category(&Category {
            icon: "pizza".into(),
            ..food.clone()
        })
        .unwrap();
        assert!(db.categories().unwrap()[0].is_default);

        db.update_category(&Category {
            name: "Restaurants".into(),
            is_default: false,
            ..food.clone()
        })
        .unwrap();
        let renamed = db.categories().unwrap().remove(0);
        assert_eq!(renamed.id, food.id);
        assert_eq!(renamed.name, "Restaurants");
        assert!(!renamed.is_default);

        // It never turns back into a default.
        db.update_category(&Category {
            is_default: true,
            ..renamed
        })
        .unwrap();
        assert!(!db.categories().unwrap()[0].is_default);
    }

    #[test]
    fn rejects_an_empty_name() {
        let db = Db::open_in_memory().unwrap();
        assert!(matches!(
            db.create_category(new("  ")),
            Err(StorageError::InvalidInput(_))
        ));
        let food = db.categories().unwrap().remove(0);
        assert!(matches!(
            db.update_category(&Category {
                name: " ".into(),
                ..food
            }),
            Err(StorageError::InvalidInput(_))
        ));
    }

    #[test]
    fn hiding_a_default_and_showing_it_again() {
        let db = Db::open_in_memory().unwrap();
        let health = CategoryId::new("default-health");
        db.delete_category(&health).unwrap();
        assert!(!db.categories().unwrap().iter().any(|c| c.id == health));
        assert_eq!(names(&db.hidden_categories().unwrap()), ["health"]);
        assert_eq!(db.all_categories().unwrap().len(), 8);

        db.restore_category(&health).unwrap();
        assert!(db.hidden_categories().unwrap().is_empty());
        assert_eq!(db.categories().unwrap()[6].id, health);
    }

    #[test]
    fn soft_delete_and_restore() {
        let db = Db::open_in_memory().unwrap();
        let souvenirs = db.create_category(new("Souvenirs")).unwrap();
        assert!(matches!(
            db.restore_category(&souvenirs.id),
            Err(StorageError::NotFound)
        ));

        db.delete_category(&souvenirs.id).unwrap();
        assert_eq!(db.categories().unwrap().len(), 8);
        // Own categories are deleted, not hidden.
        assert!(db.hidden_categories().unwrap().is_empty());
        assert!(db.all_categories().unwrap().contains(&souvenirs));
        assert!(matches!(
            db.delete_category(&souvenirs.id),
            Err(StorageError::NotFound)
        ));
        assert!(matches!(
            db.update_category(&souvenirs),
            Err(StorageError::NotFound)
        ));

        db.restore_category(&souvenirs.id).unwrap();
        assert_eq!(db.categories().unwrap()[8], souvenirs);
    }

    #[test]
    fn records_origin_device() {
        let db = Db::open_in_memory().unwrap();
        let souvenirs = db.create_category(new("Souvenirs")).unwrap();
        let origin: String = db
            .with(|conn| {
                Ok(conn.query_row(
                    "SELECT origin_device_id FROM category WHERE id = ?1",
                    [souvenirs.id.as_str()],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(origin, db.device_id());
    }
}
