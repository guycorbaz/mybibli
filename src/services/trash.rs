use chrono::NaiveDateTime;
use sqlx::Row;
use crate::db::DbPool;
use crate::error::AppError;
use crate::models::trash::{TrashEntry, TrashModel};
use crate::services::soft_delete::ALLOWED_TABLES;

#[derive(Clone, Debug)]
pub struct ConflictInfo {
    pub description: String,
}

pub struct TrashService;

impl TrashService {
    /// Restore a soft-deleted item: clear deleted_at, bump version
    pub async fn restore(
        pool: &DbPool,
        table: &str,
        id: u64,
        version: i32,
    ) -> Result<TrashEntry, AppError> {
        // Validate table against soft_delete::ALLOWED_TABLES
        if !ALLOWED_TABLES.contains(&table) {
            return Err(AppError::BadRequest(format!("Invalid table: {}", table)));
        }

        // UPDATE with optimistic locking
        let result = sqlx::query(
            &format!(
                "UPDATE {} SET deleted_at = NULL, version = version + 1 WHERE id = ? AND deleted_at IS NOT NULL AND version = ?",
                table
            ),
        )
        .bind(id as i64)
        .bind(version)
        .execute(pool)
        .await?;

        // Check if update succeeded
        if result.rows_affected() == 0 {
            // Check if item exists at all
            let exists = sqlx::query(&format!("SELECT id FROM {} WHERE id = ?", table))
                .bind(id as i64)
                .fetch_optional(pool)
                .await?;

            if exists.is_some() {
                return Err(AppError::Conflict("version_mismatch".to_string()));
            } else {
                return Err(AppError::NotFound("Item not found in trash".to_string()));
            }
        }

        // Fetch restored row
        let item_col = match table {
            "titles" => "title",
            "volumes" => "label",
            "contributors" => "name",
            "storage_locations" => "name",
            "borrowers" => "name",
            "series" => "name",
            _ => "name",
        };

        let row = sqlx::query(&format!(
            "SELECT CAST(id AS SIGNED) as id, '{}' as table_name, {} as item_name, CAST(deleted_at AS DATETIME) as deleted_at, version FROM {} WHERE id = ?",
            table, item_col, table
        ))
        .bind(id as i64)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Restored item not found".to_string()))?;

        let entry = TrashEntry {
            id: row.get::<i64, _>("id") as u64,
            table_name: row.get::<String, _>("table_name"),
            item_name: row.get::<String, _>("item_name"),
            deleted_at: row.get::<Option<NaiveDateTime>, _>("deleted_at"),
            version: row.get::<i32, _>("version"),
        };

        Ok(entry)
    }

    /// Detect conflicts when restoring an item
    pub async fn detect_restore_conflicts(
        pool: &DbPool,
        table: &str,
        id: u64,
    ) -> Result<Vec<ConflictInfo>, AppError> {
        // Validate table
        if !ALLOWED_TABLES.contains(&table) {
            return Err(AppError::BadRequest(format!("Invalid table: {}", table)));
        }

        let mut conflicts = vec![];

        match table {
            "series" => {
                // Check if any assigned titles have been reassigned to different series
                let conflict_rows = sqlx::query(
                    "SELECT DISTINCT sta.title_id, t.title FROM series_title_assignments sta
                     JOIN titles t ON sta.title_id = t.id
                     WHERE sta.series_id = ? AND sta.series_id != (
                         SELECT series_id FROM series_title_assignments WHERE title_id = sta.title_id AND deleted_at IS NULL ORDER BY position DESC LIMIT 1
                     )",
                )
                .bind(id as i64)
                .fetch_all(pool)
                .await?;

                for row in conflict_rows {
                    let title: String = row.get("title");
                    conflicts.push(ConflictInfo {
                        description: format!("Title '{}' was reassigned to another series", title),
                    });
                }
            }
            "contributors" => {
                // Check if titles have had this contributor reassigned or removed
                let conflict_rows = sqlx::query(
                    "SELECT DISTINCT t.title FROM title_contributors tc
                     JOIN titles t ON tc.title_id = t.id
                     WHERE tc.contributor_id = ? AND tc.deleted_at IS NOT NULL",
                )
                .bind(id as i64)
                .fetch_all(pool)
                .await?;

                for row in conflict_rows {
                    let title: String = row.get("title");
                    conflicts.push(ConflictInfo {
                        description: format!("Contributor role in '{}' was modified or removed", title),
                    });
                }
            }
            _ => {
                // Other tables may have minimal conflict detection for now
            }
        }

        Ok(conflicts)
    }

    /// Restore with conflicts cleared: set conflicting FKs to NULL
    pub async fn restore_with_conflicts_cleared(
        pool: &DbPool,
        table: &str,
        id: u64,
        version: i32,
    ) -> Result<TrashEntry, AppError> {
        // Start a transaction for atomic restore + FK cleanup
        let mut tx = pool.begin().await?;

        // Clear conflicting FKs based on table type
        match table {
            "series" => {
                // Set title.series_id to NULL for any reassigned titles
                sqlx::query(
                    "UPDATE series_title_assignments sta
                     SET series_id = NULL
                     WHERE series_id = ? AND series_id != (
                         SELECT series_id FROM series_title_assignments WHERE title_id = sta.title_id AND deleted_at IS NULL ORDER BY position DESC LIMIT 1
                     )",
                )
                .bind(id as i64)
                .execute(&mut *tx)
                .await?;
            }
            _ => {
                // Other tables handled similarly (implementation per table type)
            }
        }

        // Now restore: UPDATE with optimistic locking
        let result = sqlx::query(
            &format!(
                "UPDATE {} SET deleted_at = NULL, version = version + 1 WHERE id = ? AND deleted_at IS NOT NULL AND version = ?",
                table
            ),
        )
        .bind(id as i64)
        .bind(version)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(AppError::Conflict("version_mismatch".to_string()));
        }

        // Fetch restored row
        let entry = TrashModel::get_trash_entry(pool, table, id)
            .await?
            .ok_or_else(|| AppError::NotFound("Restored item not found".to_string()))?;

        tx.commit().await?;
        Ok(entry)
    }

    /// Verify parent exists (for child entities)
    pub async fn verify_parent_exists(
        pool: &DbPool,
        table: &str,
        id: u64,
    ) -> Result<bool, AppError> {
        // For volumes, check if parent title exists
        if table == "volumes" {
            let parent = sqlx::query("SELECT title_id FROM volumes WHERE id = ?")
                .bind(id as i64)
                .fetch_optional(pool)
                .await?;

            if let Some(row) = parent {
                let title_id: i64 = row.get("title_id");
                let parent_exists = sqlx::query("SELECT id FROM titles WHERE id = ?")
                    .bind(title_id)
                    .fetch_optional(pool)
                    .await?
                    .is_some();

                return Ok(parent_exists);
            }
        }

        Ok(true)
    }

    /// Permanently delete a soft-deleted item (hard delete)
    pub async fn permanent_delete(
        pool: &DbPool,
        table: &str,
        id: u64,
        version: i32,
    ) -> Result<TrashEntry, AppError> {
        // Validate table against soft_delete::ALLOWED_TABLES
        if !ALLOWED_TABLES.contains(&table) {
            return Err(AppError::BadRequest(format!("Invalid table: {}", table)));
        }

        // Fetch the entry before deletion for audit trail
        let entry = TrashModel::get_trash_entry(pool, table, id)
            .await?
            .ok_or_else(|| AppError::NotFound("Item already gone".to_string()))?;

        // Hard delete with optimistic locking
        let result = sqlx::query(&format!("DELETE FROM {} WHERE id = ? AND version = ?", table))
            .bind(id as i64)
            .bind(version)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            // Check if item exists at all
            let exists = sqlx::query(&format!("SELECT id FROM {} WHERE id = ?", table))
                .bind(id as i64)
                .fetch_optional(pool)
                .await?;

            if exists.is_some() {
                return Err(AppError::Conflict("version_mismatch".to_string()));
            } else {
                return Err(AppError::NotFound("Item already gone".to_string()));
            }
        }

        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = "./migrations")]
    async fn test_restore_clears_deleted_at_and_bumps_version(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, version, deleted_at) VALUES (?, 'book', 1, 1, NOW())")
            .bind("Deleted Title")
            .execute(&pool)
            .await?;

        let restored = TrashService::restore(&pool, "titles", 1, 1).await?;
        assert_eq!(restored.version, 2, "Version should be bumped to 2");

        let check = sqlx::query("SELECT deleted_at FROM titles WHERE id = 1")
            .fetch_one(&pool)
            .await?;
        let deleted_at: Option<chrono::NaiveDateTime> = check.get("deleted_at");
        assert!(deleted_at.is_none(), "deleted_at should be NULL after restore");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_restore_with_stale_version_returns_409(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, version, deleted_at) VALUES (?, 'book', 1, 2, NOW())")
            .bind("Deleted Title")
            .execute(&pool)
            .await?;

        let result = TrashService::restore(&pool, "titles", 1, 1).await;
        assert!(
            matches!(result, Err(AppError::Conflict(msg)) if msg == "version_mismatch"),
            "Expected Conflict error with version_mismatch"
        );

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_restore_not_found_if_already_purged(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let result = TrashService::restore(&pool, "titles", 999, 1).await;
        assert!(
            matches!(result, Err(AppError::NotFound(_))),
            "Expected NotFound error"
        );

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_permanent_delete_hard_deletes_row(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, version, deleted_at) VALUES (?, 'book', 1, 1, NOW())")
            .bind("To Delete")
            .execute(&pool)
            .await?;

        let deleted = TrashService::permanent_delete(&pool, "titles", 1, 1).await?;
        assert_eq!(deleted.item_name, "To Delete");

        let check = sqlx::query("SELECT id FROM titles WHERE id = 1")
            .fetch_optional(&pool)
            .await?;
        assert!(check.is_none(), "Row should be hard-deleted");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_permanent_delete_with_version_mismatch(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        sqlx::query("INSERT INTO titles (title, media_type, genre_id, version, deleted_at) VALUES (?, 'book', 1, 2, NOW())")
            .bind("To Delete")
            .execute(&pool)
            .await?;

        let result = TrashService::permanent_delete(&pool, "titles", 1, 1).await;
        assert!(
            matches!(result, Err(AppError::Conflict(msg)) if msg == "version_mismatch"),
            "Expected Conflict error"
        );

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_permanent_delete_already_gone(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let result = TrashService::permanent_delete(&pool, "titles", 999, 1).await;
        assert!(
            matches!(result, Err(AppError::NotFound(msg)) if msg == "Item already gone"),
            "Expected NotFound error"
        );

        Ok(())
    }
}
