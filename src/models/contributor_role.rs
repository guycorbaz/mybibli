use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{CONFLICT_NAME_TAKEN, CreateOutcome, DeleteOutcome};
use crate::services::locking::check_update_result;

#[derive(Debug, Clone)]
pub struct ContributorRoleModel {
    pub id: u64,
    pub name: String,
    pub version: i32,
}

impl std::fmt::Display for ContributorRoleModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl ContributorRoleModel {
    /// Existence check (story 8-4 P27 — renamed from `find_by_id` so the
    /// legacy boolean variant cannot be confused with the struct-returning
    /// `get` added by 8-4). Returns true if a non-deleted row with this id
    /// exists. Callers: `services/contributor.rs` validates incoming
    /// `role_id` against this when creating title-contributor links.
    pub async fn exists(pool: &DbPool, id: u64) -> Result<bool, AppError> {
        let row: Option<(u64,)> =
            sqlx::query_as("SELECT id FROM contributor_roles WHERE id = ? AND deleted_at IS NULL")
                .bind(id)
                .fetch_optional(pool)
                .await?;
        Ok(row.is_some())
    }

    /// Legacy id+name pair listing kept for the title-form dropdown
    /// population. Story 8-4 admin handlers use `list_all` instead.
    pub async fn find_all(pool: &DbPool) -> Result<Vec<(u64, String)>, AppError> {
        let rows: Vec<(u64, String)> = sqlx::query_as(
            "SELECT id, name FROM contributor_roles WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    pub async fn list_all(pool: &DbPool) -> Result<Vec<ContributorRoleModel>, AppError> {
        let rows = sqlx::query(
            "SELECT id, name, version FROM contributor_roles \
             WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;

        let mut roles = Vec::with_capacity(rows.len());
        for r in &rows {
            roles.push(ContributorRoleModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                version: r.try_get("version")?,
            });
        }
        Ok(roles)
    }

    pub async fn get(pool: &DbPool, id: u64) -> Result<Option<ContributorRoleModel>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, version FROM contributor_roles \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(ContributorRoleModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                version: r.try_get("version")?,
            })),
            None => Ok(None),
        }
    }

    pub async fn create(pool: &DbPool, name: &str) -> Result<CreateOutcome, AppError> {
        match sqlx::query("INSERT INTO contributor_roles (name) VALUES (?)")
            .bind(name)
            .execute(pool)
            .await
        {
            Ok(res) => Ok(CreateOutcome::Created(res.last_insert_id())),
            Err(sqlx::Error::Database(db_err))
                if db_err.code().as_deref() == Some("23000") =>
            {
                let existing: Option<(u64, i32)> = sqlx::query_as(
                    "SELECT id, version FROM contributor_roles \
                     WHERE name = ? AND deleted_at IS NOT NULL LIMIT 1",
                )
                .bind(name)
                .fetch_optional(pool)
                .await?;
                match existing {
                    Some((id, version)) => {
                        let res = sqlx::query(
                            "UPDATE contributor_roles SET deleted_at = NULL, version = version + 1 \
                             WHERE id = ? AND version = ?",
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

    pub async fn rename(
        pool: &DbPool,
        id: u64,
        version: i32,
        new_name: &str,
    ) -> Result<(), AppError> {
        let res = sqlx::query(
            "UPDATE contributor_roles SET name = ?, version = version + 1 \
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
        check_update_result(res.rows_affected(), "contributor_role")
    }

    pub async fn soft_delete(pool: &DbPool, id: u64, version: i32) -> Result<(), AppError> {
        let res = sqlx::query(
            "UPDATE contributor_roles SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
        check_update_result(res.rows_affected(), "contributor_role")
    }

    /// Count active title_contributors rows referencing this role.
    pub async fn count_usage(pool: &DbPool, id: u64) -> Result<i64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM title_contributors \
             WHERE role_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }

    /// Atomic count-and-delete (story 8-4 P1) — see GenreModel::delete_if_unused.
    pub async fn delete_if_unused(
        pool: &DbPool,
        id: u64,
        version: i32,
    ) -> Result<DeleteOutcome, AppError> {
        let mut tx = pool.begin().await?;

        let locked = sqlx::query(
            "SELECT id FROM contributor_roles \
             WHERE id = ? AND version = ? AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(id)
        .bind(version)
        .fetch_optional(&mut *tx)
        .await?;
        if locked.is_none() {
            tx.rollback().await?;
            return Err(AppError::Conflict(
                rust_i18n::t!("error.conflict", entity = "contributor_role").to_string(),
            ));
        }

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM title_contributors \
             WHERE role_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if count.0 > 0 {
            tx.rollback().await?;
            return Ok(DeleteOutcome::InUse(count.0));
        }

        let res = sqlx::query(
            "UPDATE contributor_roles SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(&mut *tx)
        .await?;
        check_update_result(res.rows_affected(), "contributor_role")?;

        tx.commit().await?;
        Ok(DeleteOutcome::Deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contributor_role_display() {
        let r = ContributorRoleModel {
            id: 1,
            name: "Auteur".to_string(),
            version: 1,
        };
        assert_eq!(r.to_string(), "Auteur");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_role_create_and_find(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = ContributorRoleModel::create(&pool, "Z-role-test").await?.id();
        let role = ContributorRoleModel::get(&pool, id).await?.unwrap();
        assert_eq!(role.name, "Z-role-test");
        assert!(ContributorRoleModel::exists(&pool, id).await?);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_role_create_collision_with_active(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Seeded role "Auteur" already exists.
        let res = ContributorRoleModel::create(&pool, "Auteur").await;
        assert!(matches!(res, Err(AppError::Conflict(msg)) if msg == "name_taken"));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_role_create_collision_with_deleted_reactivates(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = ContributorRoleModel::create(&pool, "Z-role-recycle").await?.id();
        let row = ContributorRoleModel::get(&pool, id).await?.unwrap();
        ContributorRoleModel::soft_delete(&pool, id, row.version).await?;
        let outcome = ContributorRoleModel::create(&pool, "Z-role-recycle").await?;
        assert_eq!(outcome, CreateOutcome::Reactivated(id));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_role_count_usage_zero_on_unused(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = ContributorRoleModel::create(&pool, "Z-role-usage-zero").await?.id();
        assert_eq!(ContributorRoleModel::count_usage(&pool, id).await?, 0);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_role_list_all_includes_seeded(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let roles = ContributorRoleModel::list_all(&pool).await?;
        assert!(roles.iter().any(|r| r.name == "Auteur"));
        Ok(())
    }
}
