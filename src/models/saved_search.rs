use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{CONFLICT_NAME_TAKEN, CreateOutcome};
use crate::services::locking::check_update_result;

/// A named, re-runnable bundle of the home browse state (CR #367). Global
/// (single-tenant) — no per-user scope. The four criteria columns mirror the
/// home `SearchParams` (`q`, `filter`, `sort`, `dir`); `None` means the field
/// was absent from the saved URL.
#[derive(Debug, Clone)]
pub struct SavedSearchModel {
    pub id: u64,
    pub name: String,
    pub q: Option<String>,
    pub filter: Option<String>,
    pub sort: Option<String>,
    pub dir: Option<String>,
    pub version: i32,
}

impl std::fmt::Display for SavedSearchModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl SavedSearchModel {
    fn from_row(r: &sqlx::mysql::MySqlRow) -> Result<Self, AppError> {
        Ok(SavedSearchModel {
            id: r.try_get("id")?,
            name: r.try_get("name")?,
            q: r.try_get("q")?,
            filter: r.try_get("filter")?,
            sort: r.try_get("sort")?,
            dir: r.try_get("dir")?,
            version: r.try_get("version")?,
        })
    }

    /// All non-deleted saved searches, sorted by name — ready for the
    /// home search-bar dropdown.
    pub async fn list_all(pool: &DbPool) -> Result<Vec<SavedSearchModel>, AppError> {
        let rows = sqlx::query(
            "SELECT id, name, q, filter, sort, dir, version \
             FROM saved_searches WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            out.push(Self::from_row(r)?);
        }
        Ok(out)
    }

    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<SavedSearchModel>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, q, filter, sort, dir, version \
             FROM saved_searches WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(Self::from_row(&r)?)),
            None => Ok(None),
        }
    }

    /// Insert a new saved search capturing the current browse criteria. On a
    /// UNIQUE collision with a soft-deleted row, reactivate it (refreshing the
    /// criteria) and return `Reactivated`; on collision with a live row,
    /// `Conflict(name_taken)`.
    pub async fn create(
        pool: &DbPool,
        name: &str,
        q: Option<&str>,
        filter: Option<&str>,
        sort: Option<&str>,
        dir: Option<&str>,
    ) -> Result<CreateOutcome, AppError> {
        match sqlx::query(
            "INSERT INTO saved_searches (name, q, filter, sort, dir) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(name)
        .bind(q)
        .bind(filter)
        .bind(sort)
        .bind(dir)
        .execute(pool)
        .await
        {
            Ok(res) => Ok(CreateOutcome::Created(res.last_insert_id())),
            Err(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("23000") => {
                let existing: Option<(u64, i32)> = sqlx::query_as(
                    "SELECT id, version FROM saved_searches \
                     WHERE name = ? AND deleted_at IS NOT NULL LIMIT 1",
                )
                .bind(name)
                .fetch_optional(pool)
                .await?;

                match existing {
                    Some((id, version)) => {
                        let res = sqlx::query(
                            "UPDATE saved_searches \
                             SET deleted_at = NULL, q = ?, filter = ?, sort = ?, dir = ?, \
                                 version = version + 1 \
                             WHERE id = ? AND version = ?",
                        )
                        .bind(q)
                        .bind(filter)
                        .bind(sort)
                        .bind(dir)
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

    pub async fn rename(
        pool: &DbPool,
        id: u64,
        version: i32,
        new_name: &str,
    ) -> Result<(), AppError> {
        let res = sqlx::query(
            "UPDATE saved_searches SET name = ?, version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(new_name)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await
        .map_err(|err| match &err {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23000") => {
                AppError::Conflict(CONFLICT_NAME_TAKEN.to_string())
            }
            _ => AppError::from(err),
        })?;
        check_update_result(res.rows_affected(), "saved_search")
    }

    pub async fn soft_delete(pool: &DbPool, id: u64, version: i32) -> Result<(), AppError> {
        let res = sqlx::query(
            "UPDATE saved_searches SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
        check_update_result(res.rows_affected(), "saved_search")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        let s = SavedSearchModel {
            id: 1,
            name: "BD without cover".to_string(),
            q: None,
            filter: Some("no_cover".to_string()),
            sort: None,
            dir: None,
            version: 1,
        };
        assert_eq!(s.to_string(), "BD without cover");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn create_and_find_round_trips_criteria(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let outcome = SavedSearchModel::create(
            &pool,
            "Z-uncategorized-recent",
            Some("asterix"),
            Some("uncategorized"),
            Some("title"),
            Some("desc"),
        )
        .await?;
        let id = match outcome {
            CreateOutcome::Created(id) => id,
            CreateOutcome::Reactivated(_) => panic!("expected Created"),
        };
        let found = SavedSearchModel::find_by_id(&pool, id).await?.unwrap();
        assert_eq!(found.name, "Z-uncategorized-recent");
        assert_eq!(found.q.as_deref(), Some("asterix"));
        assert_eq!(found.filter.as_deref(), Some("uncategorized"));
        assert_eq!(found.sort.as_deref(), Some("title"));
        assert_eq!(found.dir.as_deref(), Some("desc"));
        assert_eq!(found.version, 1);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn create_allows_all_null_criteria(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = SavedSearchModel::create(&pool, "Z-empty", None, None, None, None)
            .await?
            .id();
        let found = SavedSearchModel::find_by_id(&pool, id).await?.unwrap();
        assert!(found.q.is_none() && found.filter.is_none());
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn create_collision_with_active_returns_conflict(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        SavedSearchModel::create(&pool, "Z-dup", None, None, None, None).await?;
        let res = SavedSearchModel::create(&pool, "Z-dup", None, None, None, None).await;
        assert!(matches!(&res, Err(AppError::Conflict(m)) if m == CONFLICT_NAME_TAKEN));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn create_collision_with_deleted_reactivates_and_refreshes_criteria(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = SavedSearchModel::create(&pool, "Z-react", Some("old"), None, None, None)
            .await?
            .id();
        let row = SavedSearchModel::find_by_id(&pool, id).await?.unwrap();
        SavedSearchModel::soft_delete(&pool, id, row.version).await?;

        let outcome = SavedSearchModel::create(&pool, "Z-react", Some("new"), Some("no_cover"), None, None).await?;
        match outcome {
            CreateOutcome::Reactivated(rid) => assert_eq!(rid, id),
            CreateOutcome::Created(_) => panic!("expected Reactivated"),
        }
        let restored = SavedSearchModel::find_by_id(&pool, id).await?.unwrap();
        assert_eq!(restored.q.as_deref(), Some("new"), "criteria refreshed on reactivate");
        assert_eq!(restored.filter.as_deref(), Some("no_cover"));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn case_insensitive_name_collision(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        SavedSearchModel::create(&pool, "Z-CaseSaved", None, None, None, None).await?;
        let res = SavedSearchModel::create(&pool, "z-casesaved", None, None, None, None).await;
        assert!(matches!(&res, Err(AppError::Conflict(m)) if m == CONFLICT_NAME_TAKEN));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn rename_round_trip_and_version_bump(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = SavedSearchModel::create(&pool, "Z-old-name", None, None, None, None)
            .await?
            .id();
        let row = SavedSearchModel::find_by_id(&pool, id).await?.unwrap();
        SavedSearchModel::rename(&pool, id, row.version, "Z-new-name").await?;
        let renamed = SavedSearchModel::find_by_id(&pool, id).await?.unwrap();
        assert_eq!(renamed.name, "Z-new-name");
        assert_eq!(renamed.version, row.version + 1);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn rename_version_mismatch_conflicts(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = SavedSearchModel::create(&pool, "Z-stale", None, None, None, None)
            .await?
            .id();
        let res = SavedSearchModel::rename(&pool, id, 999, "Z-stale-new").await;
        assert!(matches!(res, Err(AppError::VersionMismatch { entity: "saved_search" })));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn soft_delete_hides_row(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = SavedSearchModel::create(&pool, "Z-del", None, None, None, None)
            .await?
            .id();
        let row = SavedSearchModel::find_by_id(&pool, id).await?.unwrap();
        SavedSearchModel::soft_delete(&pool, id, row.version).await?;
        assert!(SavedSearchModel::find_by_id(&pool, id).await?.is_none());
        Ok(())
    }
}
