use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{CreateOutcome, DeleteOutcome};
use crate::services::locking::check_update_result;

#[derive(Debug, Clone)]
pub struct VolumeStateModel {
    pub id: u64,
    pub name: String,
    pub is_loanable: bool,
    pub version: i32,
}

impl std::fmt::Display for VolumeStateModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl VolumeStateModel {
    pub async fn list_active(pool: &DbPool) -> Result<Vec<VolumeStateModel>, AppError> {
        Self::list_all(pool).await
    }

    pub async fn list_all(pool: &DbPool) -> Result<Vec<VolumeStateModel>, AppError> {
        let rows = sqlx::query(
            "SELECT id, name, is_loanable, version FROM volume_states \
             WHERE deleted_at IS NULL ORDER BY name",
        )
        .fetch_all(pool)
        .await?;

        let mut states = Vec::with_capacity(rows.len());
        for r in &rows {
            states.push(VolumeStateModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                is_loanable: r.try_get("is_loanable")?,
                version: r.try_get("version")?,
            });
        }
        Ok(states)
    }

    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<VolumeStateModel>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, is_loanable, version FROM volume_states \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(VolumeStateModel {
                id: r.try_get("id")?,
                name: r.try_get("name")?,
                is_loanable: r.try_get("is_loanable")?,
                version: r.try_get("version")?,
            })),
            None => Ok(None),
        }
    }

    /// Check if a volume is loanable based on its condition state.
    /// Returns true if volume has no condition state (default loanable) or state.is_loanable is true.
    pub async fn is_loanable_by_volume(pool: &DbPool, volume_id: u64) -> Result<bool, AppError> {
        let row = sqlx::query(
            r#"SELECT vs.is_loanable
               FROM volume_states vs
               JOIN volumes v ON v.condition_state_id = vs.id
               WHERE v.id = ? AND v.deleted_at IS NULL AND vs.deleted_at IS NULL"#,
        )
        .bind(volume_id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(r.try_get("is_loanable")?),
            None => Ok(true),
        }
    }

    pub async fn create(
        pool: &DbPool,
        name: &str,
        is_loanable: bool,
    ) -> Result<CreateOutcome, AppError> {
        match sqlx::query("INSERT INTO volume_states (name, is_loanable) VALUES (?, ?)")
            .bind(name)
            .bind(is_loanable)
            .execute(pool)
            .await
        {
            Ok(res) => Ok(CreateOutcome::Created(res.last_insert_id())),
            Err(sqlx::Error::Database(db_err))
                if db_err.code().as_deref() == Some("23000") =>
            {
                let existing: Option<(u64, i32)> = sqlx::query_as(
                    "SELECT id, version FROM volume_states \
                     WHERE name = ? AND deleted_at IS NOT NULL LIMIT 1",
                )
                .bind(name)
                .fetch_optional(pool)
                .await?;
                match existing {
                    Some((id, version)) => {
                        // Story 8-4 P34 (D3-a): preserve the previous `is_loanable`
                        // on reactivation. The form's `is_loanable` is intentionally
                        // ignored — admins must use the explicit toggle to flip it,
                        // so reactivating "Endommagé" (originally NOT loanable) does
                        // NOT silently re-enable loans for the rows still keyed to
                        // it. The `is_loanable` parameter is kept in the signature
                        // for future use (e.g., explicit "override on reactivate"
                        // path) but is otherwise unused on this branch.
                        let _ = is_loanable;
                        let res = sqlx::query(
                            "UPDATE volume_states SET deleted_at = NULL, \
                             version = version + 1 WHERE id = ? AND version = ?",
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
            "UPDATE volume_states SET name = ?, version = version + 1 \
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
        check_update_result(res.rows_affected(), "volume_state")
    }

    pub async fn soft_delete(pool: &DbPool, id: u64, version: i32) -> Result<(), AppError> {
        let res = sqlx::query(
            "UPDATE volume_states SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
        check_update_result(res.rows_affected(), "volume_state")
    }

    /// Count active volumes attached to this state (story 8-4 AC#4).
    pub async fn count_usage(pool: &DbPool, id: u64) -> Result<i64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM volumes WHERE condition_state_id = ? AND deleted_at IS NULL",
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
            "SELECT id FROM volume_states WHERE id = ? AND version = ? AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(id)
        .bind(version)
        .fetch_optional(&mut *tx)
        .await?;
        if locked.is_none() {
            tx.rollback().await?;
            return Err(AppError::Conflict(
                rust_i18n::t!("error.conflict", entity = "volume_state").to_string(),
            ));
        }

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM volumes WHERE condition_state_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if count.0 > 0 {
            tx.rollback().await?;
            return Ok(DeleteOutcome::InUse(count.0));
        }

        let res = sqlx::query(
            "UPDATE volume_states SET deleted_at = NOW(), version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(version)
        .execute(&mut *tx)
        .await?;
        check_update_result(res.rows_affected(), "volume_state")?;

        tx.commit().await?;
        Ok(DeleteOutcome::Deleted)
    }

    /// Toggle `is_loanable` on this state (story 8-4 AC#5). Forward-only —
    /// existing active loans are NOT auto-returned; the transition only
    /// gates new loan creation through `is_loanable_by_volume`.
    pub async fn set_loanable(
        pool: &DbPool,
        id: u64,
        version: i32,
        is_loanable: bool,
    ) -> Result<(), AppError> {
        let res = sqlx::query(
            "UPDATE volume_states SET is_loanable = ?, version = version + 1 \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(is_loanable)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;
        check_update_result(res.rows_affected(), "volume_state")
    }

    /// Count currently-open loans whose volume is in this state. Drives
    /// the loanable-toggle warning modal — story 8-4 AC#5.
    pub async fn count_active_loans_for_state(
        pool: &DbPool,
        state_id: u64,
    ) -> Result<i64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM loans l \
               JOIN volumes v ON l.volume_id = v.id \
              WHERE v.condition_state_id = ? \
                AND l.returned_at IS NULL \
                AND l.deleted_at IS NULL \
                AND v.deleted_at IS NULL",
        )
        .bind(state_id)
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_state_display() {
        let state = VolumeStateModel {
            id: 1,
            name: "Neuf".to_string(),
            is_loanable: true,
            version: 1,
        };
        assert_eq!(state.to_string(), "Neuf");
    }

    #[test]
    fn test_volume_state_clone() {
        let state = VolumeStateModel {
            id: 3,
            name: "Usé".to_string(),
            is_loanable: true,
            version: 2,
        };
        let cloned = state.clone();
        assert_eq!(cloned.id, 3);
        assert_eq!(cloned.name, "Usé");
        assert!(cloned.is_loanable);
        assert_eq!(cloned.version, 2);
    }

    #[test]
    fn test_volume_state_not_loanable() {
        let state = VolumeStateModel {
            id: 5,
            name: "Détruit".to_string(),
            is_loanable: false,
            version: 1,
        };
        assert!(!state.is_loanable);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_volume_state_create_and_find(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = VolumeStateModel::create(&pool, "Z-state-test", true).await?.id();
        let row = VolumeStateModel::find_by_id(&pool, id).await?.unwrap();
        assert_eq!(row.name, "Z-state-test");
        assert!(row.is_loanable);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_volume_state_set_loanable_off_and_on(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = VolumeStateModel::create(&pool, "Z-state-toggle", true).await?.id();
        let row = VolumeStateModel::find_by_id(&pool, id).await?.unwrap();
        VolumeStateModel::set_loanable(&pool, id, row.version, false).await?;
        let updated = VolumeStateModel::find_by_id(&pool, id).await?.unwrap();
        assert!(!updated.is_loanable);
        VolumeStateModel::set_loanable(&pool, id, updated.version, true).await?;
        let toggled_back = VolumeStateModel::find_by_id(&pool, id).await?.unwrap();
        assert!(toggled_back.is_loanable);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_volume_state_count_active_loans_zero_on_unused_state(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = VolumeStateModel::create(&pool, "Z-state-loans-zero", true).await?.id();
        let count = VolumeStateModel::count_active_loans_for_state(&pool, id).await?;
        assert_eq!(count, 0);
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_volume_state_rename_roundtrip(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = VolumeStateModel::create(&pool, "Z-state-old", true).await?.id();
        let row = VolumeStateModel::find_by_id(&pool, id).await?.unwrap();
        VolumeStateModel::rename(&pool, id, row.version, "Z-state-new").await?;
        let renamed = VolumeStateModel::find_by_id(&pool, id).await?.unwrap();
        assert_eq!(renamed.name, "Z-state-new");
        Ok(())
    }

    /// Story 8-4 P34 (D3-a): reactivation PRESERVES the prior `is_loanable`
    /// value. The form's `is_loanable` is intentionally ignored on this path
    /// — admins must use the explicit toggle to flip it. This prevents the
    /// silent UX regression where re-creating a previously-deleted state
    /// (e.g., "Endommagé", originally is_loanable=false) would silently
    /// re-enable loans because the Add form's checkbox defaults to checked.
    #[sqlx::test(migrations = "./migrations")]
    async fn test_volume_state_soft_delete_then_reactivate_preserves_loanable(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Original creation with is_loanable=false (e.g., "damaged" state).
        let id = VolumeStateModel::create(&pool, "Z-state-recycle", false)
            .await?
            .id();
        let row = VolumeStateModel::find_by_id(&pool, id).await?.unwrap();
        assert!(!row.is_loanable);
        VolumeStateModel::soft_delete(&pool, id, row.version).await?;
        assert!(VolumeStateModel::find_by_id(&pool, id).await?.is_none());

        // Reactivation: form's is_loanable=true is IGNORED — prior false wins.
        let outcome = VolumeStateModel::create(&pool, "Z-state-recycle", true).await?;
        match outcome {
            CreateOutcome::Reactivated(reactivated_id) => {
                assert_eq!(reactivated_id, id);
                let restored = VolumeStateModel::find_by_id(&pool, id).await?.unwrap();
                assert!(
                    !restored.is_loanable,
                    "is_loanable should be PRESERVED from prior soft-deleted state, \
                     not overwritten by the form's value (story 8-4 P34 / D3-a)"
                );
            }
            CreateOutcome::Created(_) => panic!("expected Reactivated, got Created"),
        }
        Ok(())
    }
}
