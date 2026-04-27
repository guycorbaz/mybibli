use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{CONFLICT_NAME_TAKEN, CreateOutcome, DeleteOutcome};
use crate::services::locking::check_update_result;

/// Reference taxonomy for `storage_locations.node_type`. The link from
/// `storage_locations` to this table is by NAME, not by id (the
/// `node_type` column is `VARCHAR(50)`, predates the reference table).
/// Story 8-4 keeps the loose coupling and pays for it in two places:
///
/// * `rename` is **transactional** and cascades the new name into every
///   matching `storage_locations` row.
/// * `count_usage` matches by name through the live row's name —
///   intentional asymmetry with the cascade.
#[derive(Debug, Clone)]
pub struct LocationNodeTypeModel {
    pub id: u64,
    pub name: String,
    pub version: i32,
}

impl std::fmt::Display for LocationNodeTypeModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl LocationNodeTypeModel {
    /// Legacy id+name pair listing kept for the location-form dropdown
    /// population. Story 8-4 admin handlers use `list_all` instead.
    pub async fn list_active_pairs(pool: &DbPool) -> Result<Vec<(u64, String)>, AppError> {
        let rows: Vec<(u64, String)> = sqlx::query_as(
            "SELECT id, name FROM location_node_types WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn list_all(pool: &DbPool) -> Result<Vec<LocationNodeTypeModel>, AppError> {
        let rows = sqlx::query(
            "SELECT id, name, version FROM location_node_types \
             WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;

        let mut nodes = Vec::with_capacity(rows.len());
        for r in &rows {
            nodes.push(LocationNodeTypeModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                version: r.try_get("version")?,
            });
        }
        Ok(nodes)
    }

    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<LocationNodeTypeModel>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, version FROM location_node_types \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(LocationNodeTypeModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                version: r.try_get("version")?,
            })),
            None => Ok(None),
        }
    }

    pub async fn create(pool: &DbPool, name: &str) -> Result<CreateOutcome, AppError> {
        match sqlx::query("INSERT INTO location_node_types (name) VALUES (?)")
            .bind(name)
            .execute(pool)
            .await
        {
            Ok(res) => Ok(CreateOutcome::Created(res.last_insert_id())),
            Err(sqlx::Error::Database(db_err))
                if db_err.code().as_deref() == Some("23000") =>
            {
                let existing: Option<(u64, i32)> = sqlx::query_as(
                    "SELECT id, version FROM location_node_types \
                     WHERE name = ? AND deleted_at IS NOT NULL LIMIT 1",
                )
                .bind(name)
                .fetch_optional(pool)
                .await?;
                match existing {
                    Some((id, version)) => {
                        let res = sqlx::query(
                            "UPDATE location_node_types SET deleted_at = NULL, \
                             version = version + 1 WHERE id = ? AND version = ?",
                        )
                        .bind(id)
                        .bind(version)
                        .execute(pool)
                        .await?;
                        if res.rows_affected() == 1 {
                            Ok(CreateOutcome::Reactivated(id))
                        } else {
                            Err(AppError::Conflict(CONFLICT_NAME_TAKEN.to_string()))
                        }
                    }
                    None => Err(AppError::Conflict(CONFLICT_NAME_TAKEN.to_string())),
                }
            }
            Err(other) => Err(AppError::from(other)),
        }
    }

    /// Transactional rename — updates the reference row AND cascades the
    /// new name into every `storage_locations` row whose `node_type`
    /// equals the OLD name. Returns the cascade row count so the success
    /// FeedbackEntry can surface "Renamed → N locations updated".
    ///
    /// On any failure (version mismatch, unique violation, DB error) the
    /// whole transaction rolls back — neither the type rename nor the
    /// cascade is applied.
    pub async fn rename(
        pool: &DbPool,
        id: u64,
        version: i32,
        new_name: &str,
    ) -> Result<u64, AppError> {
        let mut tx = pool.begin().await?;

        let current = sqlx::query("SELECT name FROM location_node_types WHERE id = ? AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
        let old_name: String = match current {
            Some(r) => r.try_get("name")?,
            None => {
                tx.rollback().await?;
                return Err(AppError::NotFound("location_node_type".to_string()));
            }
        };

        let res = sqlx::query(
            "UPDATE location_node_types SET name = ?, version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(new_name)
        .bind(id)
        .bind(version)
        .execute(&mut *tx)
        .await
        .map_err(|err| match &err {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23000") => {
                AppError::Conflict(CONFLICT_NAME_TAKEN.to_string())
            }
            _ => AppError::from(err),
        })?;

        if res.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(AppError::Conflict(
                rust_i18n::t!("error.conflict", entity = "location_node_type").to_string(),
            ));
        }

        // Story 8-4 P35 (D4-a): cascade across soft-deleted rows too. A
        // location restored from the trash (story 8-7) keeps its `node_type`
        // VARCHAR — if we excluded `deleted_at IS NOT NULL` rows here, that
        // restored row would carry the OLD name and break joins/dropdowns.
        //
        // Story 8-4 P4 (intentional version bump): incrementing
        // `storage_locations.version` here means an admin currently editing
        // one of those rows in another tab will get a 409 on save. That is
        // the correct optimistic-lock signal — the row WAS modified (its
        // `node_type` changed). The user reloads, sees the new node_type,
        // and re-applies their edit. The alternative (skip the bump to
        // preserve in-flight edits) silently overwrites the cascade and is
        // strictly worse.
        let cascade = sqlx::query(
            "UPDATE storage_locations SET node_type = ?, version = version + 1 \
             WHERE node_type = ?",
        )
        .bind(new_name)
        .bind(&old_name)
        .execute(&mut *tx)
        .await?;
        let cascade_rows = cascade.rows_affected();

        tx.commit().await?;
        Ok(cascade_rows)
    }

    pub async fn soft_delete(pool: &DbPool, id: u64, version: i32) -> Result<(), AppError> {
        let res = sqlx::query(
            "UPDATE location_node_types SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
        check_update_result(res.rows_affected(), "location_node_type")
    }

    /// Count active `storage_locations` rows whose `node_type` matches
    /// this row's name. Sub-query so we keep the by-name semantic
    /// matching the cascade contract.
    pub async fn count_usage(pool: &DbPool, id: u64) -> Result<i64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM storage_locations \
             WHERE node_type = ( \
                SELECT name FROM location_node_types \
                 WHERE id = ? AND deleted_at IS NULL \
             ) AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Atomic count-and-delete (story 8-4 P1). The count uses the by-name
    /// semantic (matching the cascade contract); the lock is on the ref row
    /// itself, so a concurrent rename of the same node_type is serialized.
    pub async fn delete_if_unused(
        pool: &DbPool,
        id: u64,
        version: i32,
    ) -> Result<DeleteOutcome, AppError> {
        let mut tx = pool.begin().await?;

        let locked = sqlx::query(
            "SELECT name FROM location_node_types \
             WHERE id = ? AND version = ? AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(id)
        .bind(version)
        .fetch_optional(&mut *tx)
        .await?;
        let name: String = match locked {
            Some(r) => r.try_get("name")?,
            None => {
                tx.rollback().await?;
                return Err(AppError::Conflict(
                    rust_i18n::t!("error.conflict", entity = "location_node_type").to_string(),
                ));
            }
        };

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM storage_locations \
             WHERE node_type = ? AND deleted_at IS NULL",
        )
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?;
        if count.0 > 0 {
            tx.rollback().await?;
            return Ok(DeleteOutcome::InUse(count.0));
        }

        let res = sqlx::query(
            "UPDATE location_node_types SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(&mut *tx)
        .await?;
        check_update_result(res.rows_affected(), "location_node_type")?;

        tx.commit().await?;
        Ok(DeleteOutcome::Deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type_display() {
        let n = LocationNodeTypeModel {
            id: 1,
            name: "Room".to_string(),
            version: 1,
        };
        assert_eq!(n.to_string(), "Room");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_node_type_create_and_find(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = LocationNodeTypeModel::create(&pool, "Z-NodeType-Test").await?.id();
        let row = LocationNodeTypeModel::find_by_id(&pool, id).await?.unwrap();
        assert_eq!(row.name, "Z-NodeType-Test");
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_node_type_create_collision_with_active(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // "Room" is seeded by 20260401000001_seed_location_node_types.sql.
        let res = LocationNodeTypeModel::create(&pool, "Room").await;
        assert!(matches!(res, Err(AppError::Conflict(msg)) if msg == "name_taken"));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_node_type_rename_cascades_to_storage_locations(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = LocationNodeTypeModel::create(&pool, "Z-Cascade-Old").await?.id();
        let row = LocationNodeTypeModel::find_by_id(&pool, id).await?.unwrap();

        // Insert 2 storage_locations rows pointing to this type by name.
        // `label` is CHAR(5) so we use 5-char L-codes that don't collide with seeds.
        for (name, label) in [("Z-Cascade-Loc-1", "L9990"), ("Z-Cascade-Loc-2", "L9991")] {
            sqlx::query(
                "INSERT INTO storage_locations (name, node_type, parent_id, label) \
                 VALUES (?, ?, NULL, ?)",
            )
            .bind(name)
            .bind("Z-Cascade-Old")
            .bind(label)
            .execute(&pool)
            .await?;
        }

        let cascaded = LocationNodeTypeModel::rename(&pool, id, row.version, "Z-Cascade-New").await?;
        assert_eq!(cascaded, 2);

        let count_old: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM storage_locations WHERE node_type = ? AND deleted_at IS NULL",
        )
        .bind("Z-Cascade-Old")
        .fetch_one(&pool)
        .await?;
        assert_eq!(count_old.0, 0);
        let count_new: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM storage_locations WHERE node_type = ? AND deleted_at IS NULL",
        )
        .bind("Z-Cascade-New")
        .fetch_one(&pool)
        .await?;
        assert_eq!(count_new.0, 2);

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_node_type_count_usage_matches_by_name(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = LocationNodeTypeModel::create(&pool, "Z-NT-Count").await?.id();
        sqlx::query(
            "INSERT INTO storage_locations (name, node_type, parent_id, label) \
             VALUES (?, ?, NULL, ?)",
        )
        .bind("Z-NT-Count-Loc")
        .bind("Z-NT-Count")
        .bind("L9992")
        .execute(&pool)
        .await?;
        assert_eq!(LocationNodeTypeModel::count_usage(&pool, id).await?, 1);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_node_type_rename_version_mismatch_rolls_back(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = LocationNodeTypeModel::create(&pool, "Z-NT-Stale").await?.id();
        sqlx::query(
            "INSERT INTO storage_locations (name, node_type, parent_id, label) \
             VALUES (?, ?, NULL, ?)",
        )
        .bind("Z-NT-Stale-Loc")
        .bind("Z-NT-Stale")
        .bind("L9993")
        .execute(&pool)
        .await?;

        // Wrong version → rolled back; no cascade.
        let res = LocationNodeTypeModel::rename(&pool, id, 999, "Z-NT-Stale-New").await;
        assert!(matches!(res, Err(AppError::Conflict(_))));

        // Cascade did NOT happen.
        let still_old: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM storage_locations WHERE node_type = ? AND deleted_at IS NULL",
        )
        .bind("Z-NT-Stale")
        .fetch_one(&pool)
        .await?;
        assert_eq!(still_old.0, 1);
        Ok(())
    }
}
