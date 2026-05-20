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

        // Issue #73: wrap UPDATE + existence-check in a single transaction so
        // the 409-vs-404 distinction can't be flipped by a concurrent admin
        // hard-deleting (or restoring) the row between our UPDATE-returning-0
        // and the follow-up SELECT.
        let mut tx = pool.begin().await?;
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

        // Check if update succeeded
        if result.rows_affected() == 0 {
            // Check if item exists at all (within the same tx snapshot).
            let exists = sqlx::query(&format!("SELECT id FROM {} WHERE id = ?", table))
                .bind(id as i64)
                .fetch_optional(&mut *tx)
                .await?;
            tx.commit().await?;

            if exists.is_some() {
                return Err(AppError::Conflict("version_mismatch".to_string()));
            } else {
                return Err(AppError::NotFound("Item not found in trash".to_string()));
            }
        }
        tx.commit().await?;

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
                // Find titles whose soft-deleted assignment to THIS series is
                // shadowed by a newer LIVE assignment to a DIFFERENT series.
                // The real junction table is `title_series` (not the historical
                // name `series_title_assignments`) and the position column is
                // `position_number`. Issue #66.
                let conflict_rows = sqlx::query(
                    "SELECT DISTINCT sta.title_id, t.title FROM title_series sta
                     JOIN titles t ON sta.title_id = t.id
                     WHERE sta.series_id = ? AND sta.series_id != (
                         SELECT series_id FROM title_series WHERE title_id = sta.title_id AND deleted_at IS NULL ORDER BY position_number DESC LIMIT 1
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

    /// Restore with conflicts cleared.
    ///
    /// For "series": hard-deletes the soft-deleted `title_series` rows whose
    /// titles now have a newer LIVE assignment to a different series. Issue #66
    /// — previously this was an `UPDATE ... SET series_id = NULL` against the
    /// (non-existent) `series_title_assignments` table, which would have
    /// violated NOT NULL on the real schema and triggered MariaDB's correlated-
    /// subquery-on-update-target restriction (error 1093). Switched to a
    /// two-step SELECT-then-DELETE to dodge both issues.
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
                // Step 1: collect the conflicting title_ids (read-only — no
                // target-table-in-update-source conflict).
                let conflict_title_ids: Vec<i64> = sqlx::query_scalar(
                    "SELECT DISTINCT CAST(sta.title_id AS SIGNED) FROM title_series sta
                     WHERE sta.series_id = ? AND sta.series_id != (
                         SELECT series_id FROM title_series WHERE title_id = sta.title_id AND deleted_at IS NULL ORDER BY position_number DESC LIMIT 1
                     )",
                )
                .bind(id as i64)
                .fetch_all(&mut *tx)
                .await?;

                // Step 2: hard-delete those soft-deleted assignments in this
                // series. Skipping when empty avoids building a `DELETE ...
                // IN ()` which MariaDB rejects.
                if !conflict_title_ids.is_empty() {
                    let placeholders = std::iter::repeat_n("?", conflict_title_ids.len())
                        .collect::<Vec<_>>()
                        .join(",");
                    let query_str = format!(
                        "DELETE FROM title_series WHERE series_id = ? AND title_id IN ({})",
                        placeholders
                    );
                    let mut q = sqlx::query(&query_str).bind(id as i64);
                    for tid in &conflict_title_ids {
                        q = q.bind(*tid);
                    }
                    q.execute(&mut *tx).await?;
                }
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

        tx.commit().await?;

        // Fetch restored row inline — `get_trash_entry` filters by
        // `deleted_at IS NOT NULL` (trash semantics) which the freshly-
        // restored row no longer matches. Mirrors `restore()` above.
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

        Ok(TrashEntry {
            id: row.get::<i64, _>("id") as u64,
            table_name: row.get::<String, _>("table_name"),
            item_name: row.get::<String, _>("item_name"),
            deleted_at: row.get::<Option<NaiveDateTime>, _>("deleted_at"),
            version: row.get::<i32, _>("version"),
        })
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

    /// Permanently delete a soft-deleted item (hard delete).
    ///
    /// v1.5.1 fix #282 — wraps the delete in a transaction and cascades
    /// through every child FK that points at the parent (using the same
    /// FK-ordered chain `services::auto_purge::run_purge` already
    /// maintains). Without the cascade, MariaDB's default-RESTRICT FKs
    /// reject the bare `DELETE FROM titles` when any child row still
    /// references the title (even soft-deleted children — FK constraints
    /// don't consider `deleted_at`).
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

        // FK-ordered children that need to clear BEFORE the parent.
        // This list mirrors the cascade chain the auto-purge task
        // already runs nightly; the difference is that the trash
        // path targets a single parent id, not "everything older
        // than N days". Empty list = no children to clear.
        let children: &[&str] = match table {
            // titles ← 4 child tables with default-RESTRICT FKs
            "titles" => &[
                "title_contributors",
                "title_series",
                "pending_metadata_updates",
                "volumes",
            ],
            // series ← title_series only
            "series" => &["title_series"],
            // contributors ← title_contributors only
            "contributors" => &["title_contributors"],
            // borrowers ← loans (and loans hold no inbound FKs from
            // active tables — soft-deleted loans go away with the
            // borrower; this matches auto_purge's chain).
            "borrowers" => &["loans"],
            // volumes / storage_locations / users — no child rows
            // we need to clear before the parent. (FKs from sessions
            // / admin_audit are already SET NULL per issues #69/#70.)
            _ => &[],
        };

        let mut tx = pool.begin().await?;

        // Clear every child row referencing the target parent id.
        // FK-column convention: every child references its parent
        // via `<parent_singular>_id`. For our 6 children:
        //   title_contributors.title_id, title_series.title_id,
        //   pending_metadata_updates.title_id, volumes.title_id,
        //   loans.borrower_id (NOT borrower)
        // — we need a per-table column lookup rather than a naive
        // suffix-strip. Build the column name from the parent.
        let fk_column = match table {
            "titles" => "title_id",
            "series" => "series_id",
            "contributors" => "contributor_id",
            "borrowers" => "borrower_id",
            _ => "id", // unused branch — no children for this parent
        };

        for child in children {
            let sql = format!("DELETE FROM {child} WHERE {fk_column} = ?");
            sqlx::query(&sql)
                .bind(id as i64)
                .execute(&mut *tx)
                .await?;
        }

        // Hard delete the parent with optimistic locking.
        let result = sqlx::query(&format!("DELETE FROM {} WHERE id = ? AND version = ?", table))
            .bind(id as i64)
            .bind(version)
            .execute(&mut *tx)
            .await?;

        if result.rows_affected() == 0 {
            // Roll the transaction back before the diagnostic SELECTs
            // — otherwise the cascade DELETEs we just queued would
            // commit while the parent stayed put.
            tx.rollback().await?;
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

        tx.commit().await?;
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

    /// Regression test for issue #66 — series restore conflict path
    /// previously referenced the ghost table `series_title_assignments`
    /// and attempted `UPDATE ... SET series_id = NULL` (violating NOT NULL
    /// + triggering MariaDB's correlated-subquery-on-update-target error).
    /// Now uses two-step SELECT-then-DELETE against the real `title_series`
    /// table.
    #[sqlx::test(migrations = "./migrations")]
    async fn test_series_restore_with_conflicts_cleared(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let old_series_id: u64 = sqlx::query(
            "INSERT INTO series (name, version, deleted_at) VALUES (?, 1, NOW())",
        )
        .bind("Original Series")
        .execute(&pool)
        .await?
        .last_insert_id();

        let new_series_id: u64 =
            sqlx::query("INSERT INTO series (name, version) VALUES (?, 1)")
                .bind("New Series")
                .execute(&pool)
                .await?
                .last_insert_id();

        let title_id: u64 = sqlx::query(
            "INSERT INTO titles (title, media_type, genre_id, version) VALUES (?, 'book', 1, 1)",
        )
        .bind("Reassigned Title")
        .execute(&pool)
        .await?
        .last_insert_id();

        // Soft-deleted assignment to the OLD series (the conflict to clear).
        sqlx::query(
            "INSERT INTO title_series (title_id, series_id, position_number, deleted_at) \
             VALUES (?, ?, 1, NOW())",
        )
        .bind(title_id)
        .bind(old_series_id)
        .execute(&pool)
        .await?;

        // Live assignment to the NEW series (the shadow that creates the conflict).
        sqlx::query(
            "INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, 1)",
        )
        .bind(title_id)
        .bind(new_series_id)
        .execute(&pool)
        .await?;

        let conflicts = TrashService::detect_restore_conflicts(&pool, "series", old_series_id)
            .await?;
        assert_eq!(conflicts.len(), 1, "should detect one conflict");
        assert!(
            conflicts[0].description.contains("Reassigned Title"),
            "conflict description should name the reassigned title, got: {}",
            conflicts[0].description
        );

        let restored = TrashService::restore_with_conflicts_cleared(
            &pool,
            "series",
            old_series_id,
            1,
        )
        .await?;
        assert_eq!(restored.version, 2, "version should be bumped after restore");

        // The conflicting assignment to the old series must be gone.
        let old_assignment: Option<i64> = sqlx::query_scalar(
            "SELECT CAST(id AS SIGNED) FROM title_series WHERE title_id = ? AND series_id = ?",
        )
        .bind(title_id)
        .bind(old_series_id)
        .fetch_optional(&pool)
        .await?;
        assert!(
            old_assignment.is_none(),
            "soft-deleted conflicting assignment should be hard-deleted"
        );

        // The fresh live assignment to the new series must be untouched.
        let new_assignment: Option<i64> = sqlx::query_scalar(
            "SELECT CAST(id AS SIGNED) FROM title_series WHERE title_id = ? AND series_id = ?",
        )
        .bind(title_id)
        .bind(new_series_id)
        .fetch_optional(&pool)
        .await?;
        assert!(
            new_assignment.is_some(),
            "live assignment to the new series should be preserved"
        );

        Ok(())
    }
}
