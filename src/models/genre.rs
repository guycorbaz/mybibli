use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{CreateOutcome, DeleteOutcome};
use crate::services::locking::check_update_result;

#[derive(Debug, Clone)]
pub struct GenreModel {
    pub id: u64,
    pub name: String,
    pub version: i32,
}

impl std::fmt::Display for GenreModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl GenreModel {
    pub async fn find_name_by_id(pool: &DbPool, id: u64) -> Result<String, AppError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT name FROM genres WHERE id = ? AND deleted_at IS NULL")
                .bind(id)
                .fetch_optional(pool)
                .await?;
        Ok(row.map(|r| r.0).unwrap_or_default())
    }

    pub async fn list_active(pool: &DbPool) -> Result<Vec<GenreModel>, AppError> {
        Self::list_all(pool).await
    }

    pub async fn list_all(pool: &DbPool) -> Result<Vec<GenreModel>, AppError> {
        let rows = sqlx::query(
            "SELECT id, name, version FROM genres WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;

        let mut genres = Vec::with_capacity(rows.len());
        for r in &rows {
            genres.push(GenreModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                version: r.try_get("version")?,
            });
        }
        Ok(genres)
    }

    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<GenreModel>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, version FROM genres WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(GenreModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                version: r.try_get("version")?,
            })),
            None => Ok(None),
        }
    }

    /// Insert a new genre. On UNIQUE collision with a soft-deleted row,
    /// reactivate it and return `Reactivated` so the handler renders the
    /// "Reactivated" feedback message instead of "Created".
    pub async fn create(pool: &DbPool, name: &str) -> Result<CreateOutcome, AppError> {
        match sqlx::query("INSERT INTO genres (name) VALUES (?)")
            .bind(name)
            .execute(pool)
            .await
        {
            Ok(res) => Ok(CreateOutcome::Created(res.last_insert_id())),
            Err(sqlx::Error::Database(db_err))
                if db_err.code().as_deref() == Some("23000") =>
            {
                let existing: Option<(u64, i32)> = sqlx::query_as(
                    "SELECT id, version FROM genres WHERE name = ? AND deleted_at IS NOT NULL LIMIT 1",
                )
                .bind(name)
                .fetch_optional(pool)
                .await?;

                match existing {
                    Some((id, version)) => {
                        let res = sqlx::query(
                            "UPDATE genres SET deleted_at = NULL, version = version + 1 \
                             WHERE id = ? AND version = ?",
                        )
                        .bind(id)
                        .bind(version)
                        .execute(pool)
                        .await?;
                        if res.rows_affected() == 1 {
                            Ok(CreateOutcome::Reactivated(id))
                        } else {
                            Err(AppError::Conflict("name_taken".to_string()))
                        }
                    }
                    None => Err(AppError::Conflict("name_taken".to_string())),
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
            "UPDATE genres SET name = ?, version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(new_name)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await
        .map_err(|err| match &err {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23000") => {
                AppError::Conflict("name_taken".to_string())
            }
            _ => AppError::from(err),
        })?;
        check_update_result(res.rows_affected(), "genre")
    }

    pub async fn soft_delete(pool: &DbPool, id: u64, version: i32) -> Result<(), AppError> {
        let res = sqlx::query(
            "UPDATE genres SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
        check_update_result(res.rows_affected(), "genre")
    }

    /// Count rows in `titles` actively pointing at this genre. Soft-deleted
    /// titles do NOT count — deleting a genre that's only attached to
    /// trashed titles is allowed (story 8-4 AC#4).
    pub async fn count_usage(pool: &DbPool, id: u64) -> Result<i64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM titles WHERE genre_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Atomic count-and-delete. `SELECT … FOR UPDATE` locks the genre row,
    /// `COUNT(*)` runs in the same transaction (so a concurrent `INSERT INTO
    /// titles (genre_id = this)` blocks on FK lookup until commit), then
    /// the soft-delete UPDATE applies — closing the TOCTOU window the old
    /// `count_usage` + `soft_delete` pair left open (story 8-4 P1).
    pub async fn delete_if_unused(
        pool: &DbPool,
        id: u64,
        version: i32,
    ) -> Result<DeleteOutcome, AppError> {
        let mut tx = pool.begin().await?;

        let locked = sqlx::query(
            "SELECT id FROM genres WHERE id = ? AND version = ? AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(id)
        .bind(version)
        .fetch_optional(&mut *tx)
        .await?;
        if locked.is_none() {
            tx.rollback().await?;
            return Err(AppError::Conflict(
                rust_i18n::t!("error.conflict", entity = "genre").to_string(),
            ));
        }

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM titles WHERE genre_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if count.0 > 0 {
            tx.rollback().await?;
            return Ok(DeleteOutcome::InUse(count.0));
        }

        let res = sqlx::query(
            "UPDATE genres SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(&mut *tx)
        .await?;
        check_update_result(res.rows_affected(), "genre")?;

        tx.commit().await?;
        Ok(DeleteOutcome::Deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genre_display() {
        let genre = GenreModel {
            id: 1,
            name: "Roman".to_string(),
            version: 1,
        };
        assert_eq!(genre.to_string(), "Roman");
    }

    #[test]
    fn test_genre_clone() {
        let genre = GenreModel {
            id: 2,
            name: "BD".to_string(),
            version: 3,
        };
        let cloned = genre.clone();
        assert_eq!(cloned.id, 2);
        assert_eq!(cloned.name, "BD");
        assert_eq!(cloned.version, 3);
    }

    #[test]
    fn test_create_outcome_id() {
        assert_eq!(CreateOutcome::Created(7).id(), 7);
        assert_eq!(CreateOutcome::Reactivated(11).id(), 11);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_genre_create_and_find(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let outcome = GenreModel::create(&pool, "Z-create-test").await?;
        match outcome {
            CreateOutcome::Created(id) => {
                let found = GenreModel::find_by_id(&pool, id).await?.unwrap();
                assert_eq!(found.name, "Z-create-test");
                assert_eq!(found.version, 1);
            }
            CreateOutcome::Reactivated(_) => panic!("expected Created, got Reactivated"),
        }
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_genre_create_collision_with_active_returns_conflict(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // The seed migration includes "Roman".
        let res = GenreModel::create(&pool, "Roman").await;
        match res {
            Err(AppError::Conflict(msg)) => assert_eq!(msg, "name_taken"),
            other => panic!("expected Conflict(name_taken), got {other:?}"),
        }
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_genre_create_collision_with_deleted_reactivates(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let outcome = GenreModel::create(&pool, "Z-reactivate-test").await?;
        let id = outcome.id();
        let row = GenreModel::find_by_id(&pool, id).await?.unwrap();
        GenreModel::soft_delete(&pool, id, row.version).await?;

        let outcome2 = GenreModel::create(&pool, "Z-reactivate-test").await?;
        match outcome2 {
            CreateOutcome::Reactivated(reactivated_id) => assert_eq!(reactivated_id, id),
            CreateOutcome::Created(_) => panic!("expected Reactivated, got Created"),
        }
        let restored = GenreModel::find_by_id(&pool, id).await?.unwrap();
        assert_eq!(restored.name, "Z-reactivate-test");
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_genre_rename_roundtrip(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = GenreModel::create(&pool, "Z-rename-old").await?.id();
        let row = GenreModel::find_by_id(&pool, id).await?.unwrap();
        GenreModel::rename(&pool, id, row.version, "Z-rename-new").await?;
        let renamed = GenreModel::find_by_id(&pool, id).await?.unwrap();
        assert_eq!(renamed.name, "Z-rename-new");
        assert_eq!(renamed.version, row.version + 1);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_genre_rename_version_mismatch_conflict(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = GenreModel::create(&pool, "Z-rename-stale").await?.id();
        let res = GenreModel::rename(&pool, id, 999, "Z-rename-stale-new").await;
        assert!(matches!(res, Err(AppError::Conflict(_))));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_genre_soft_delete_roundtrip(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = GenreModel::create(&pool, "Z-delete-test").await?.id();
        let row = GenreModel::find_by_id(&pool, id).await?.unwrap();
        GenreModel::soft_delete(&pool, id, row.version).await?;
        let found = GenreModel::find_by_id(&pool, id).await?;
        assert!(found.is_none(), "soft-deleted row must not surface");
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_genre_count_usage_zero_on_empty_genre(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = GenreModel::create(&pool, "Z-usage-zero").await?.id();
        let count = GenreModel::count_usage(&pool, id).await?;
        assert_eq!(count, 0);
        Ok(())
    }
}
