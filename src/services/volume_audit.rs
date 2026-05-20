//! CR #237 — shelf-audit workflow service.
//!
//! The flag is purely user-driven — set/clear actions never fire
//! automatically (no auto-clear on move, re-fetch, or loan return).
//!
//! Single-volume operations (`mark`, `clear`) and bulk operations
//! (`mark_for_location`, `mark_for_search`, `clear_all`) all funnel
//! through here so the admin_audit shape stays consistent.

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::admin_audit::AdminAuditModel;
use crate::models::volume::VolumeModel;
use serde_json::json;

pub struct VolumeAuditService;

impl VolumeAuditService {
    /// Mark a single volume as "À contrôler". Idempotent —
    /// re-marking an already-marked volume just refreshes the
    /// timestamp. Audit row written.
    pub async fn mark(pool: &DbPool, volume_id: u64, user_id: u64) -> Result<(), AppError> {
        let volume = VolumeModel::find_by_id(pool, volume_id).await?.ok_or_else(|| {
            AppError::NotFound(rust_i18n::t!("error.not_found").to_string())
        })?;

        sqlx::query(
            "UPDATE volumes SET under_audit_since = NOW(), updated_at = NOW() \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(volume_id)
        .execute(pool)
        .await?;

        let _ = AdminAuditModel::create(
            pool,
            user_id,
            "volume_under_audit_set",
            Some("volumes"),
            Some(volume_id),
            Some(json!({ "label": volume.label })),
        )
        .await
        .map_err(|e| tracing::warn!(error = %e, volume_id, "audit insert failed"));

        Ok(())
    }

    /// Clear the audit flag on a single volume. Idempotent — a
    /// volume that's already cleared returns Ok with no DB write.
    pub async fn clear(pool: &DbPool, volume_id: u64, user_id: u64) -> Result<(), AppError> {
        let volume = VolumeModel::find_by_id(pool, volume_id).await?.ok_or_else(|| {
            AppError::NotFound(rust_i18n::t!("error.not_found").to_string())
        })?;
        if volume.under_audit_since.is_none() {
            return Ok(());
        }

        sqlx::query(
            "UPDATE volumes SET under_audit_since = NULL, updated_at = NOW() \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(volume_id)
        .execute(pool)
        .await?;

        let _ = AdminAuditModel::create(
            pool,
            user_id,
            "volume_under_audit_cleared",
            Some("volumes"),
            Some(volume_id),
            Some(json!({ "label": volume.label })),
        )
        .await
        .map_err(|e| tracing::warn!(error = %e, volume_id, "audit insert failed"));

        Ok(())
    }

    /// Bulk-mark every active volume currently assigned to a given
    /// location. Returns the count of newly-flagged volumes (rows
    /// already flagged don't get the timestamp refreshed; rows in
    /// soft-deleted state never count).
    pub async fn mark_for_location(
        pool: &DbPool,
        location_id: u64,
        user_id: u64,
    ) -> Result<u64, AppError> {
        let result = sqlx::query(
            "UPDATE volumes SET under_audit_since = NOW(), updated_at = NOW() \
             WHERE location_id = ? AND deleted_at IS NULL AND under_audit_since IS NULL",
        )
        .bind(location_id)
        .execute(pool)
        .await?;
        let n = result.rows_affected();

        if n > 0 {
            let _ = AdminAuditModel::create(
                pool,
                user_id,
                "volume_under_audit_bulk_set",
                Some("volumes"),
                None,
                Some(json!({ "scope": "location", "location_id": location_id, "count": n })),
            )
            .await
            .map_err(|e| tracing::warn!(error = %e, location_id, "bulk-mark audit insert failed"));
        }
        Ok(n)
    }

    /// Clear the audit flag on every currently-marked volume.
    /// Returns the count of cleared rows. Used by the audit view's
    /// "Unmark all" affordance at the end of an audit pass.
    pub async fn clear_all(pool: &DbPool, user_id: u64) -> Result<u64, AppError> {
        let result = sqlx::query(
            "UPDATE volumes SET under_audit_since = NULL, updated_at = NOW() \
             WHERE deleted_at IS NULL AND under_audit_since IS NOT NULL",
        )
        .execute(pool)
        .await?;
        let n = result.rows_affected();

        if n > 0 {
            let _ = AdminAuditModel::create(
                pool,
                user_id,
                "volume_under_audit_bulk_cleared",
                Some("volumes"),
                None,
                Some(json!({ "scope": "all", "count": n })),
            )
            .await
            .map_err(|e| tracing::warn!(error = %e, "clear-all audit insert failed"));
        }
        Ok(n)
    }

    /// Count of active volumes currently flagged. Used by the home
    /// dashboard "Volumes à contrôler" indicator.
    pub async fn count_under_audit(pool: &DbPool) -> Result<i64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM volumes WHERE under_audit_since IS NOT NULL AND deleted_at IS NULL",
        )
        .fetch_one(pool)
        .await?;
        Ok(row.0)
    }
}
