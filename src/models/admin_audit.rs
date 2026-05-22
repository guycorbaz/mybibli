use chrono::NaiveDateTime;
use sqlx::Row;
use crate::db::DbPool;
use crate::error::AppError;

#[derive(Clone, Debug)]
pub struct AdminAuditEntry {
    pub id: u64,
    /// `None` when the row's actor has been hard-deleted (issue #70).
    /// The audit row's persistent attribution lives in
    /// `details.user_username` + `details.user_role`, set at insert
    /// time by every production call site.
    pub user_id: Option<u64>,
    pub action: String,
    pub entity_type: Option<String>,
    pub entity_id: Option<u64>,
    pub timestamp: NaiveDateTime,
    pub details: Option<serde_json::Value>,
}

pub struct AdminAuditModel;

impl AdminAuditModel {
    /// Create an admin audit record (append-only).
    ///
    /// The returned `timestamp` is the actual DB-assigned value (read back via
    /// `LAST_INSERT_ID()` since MariaDB doesn't reliably support `INSERT ...
    /// RETURNING`) — Patch P13. Previously this returned a Rust-side
    /// `chrono::Local::now()` which drifted from the stored row whenever the
    /// process clock and DB clock disagreed.
    ///
    /// `CAST(timestamp AS DATETIME)` is needed because dynamic-query SQLx
    /// can't decode raw `TIMESTAMP` columns into `NaiveDateTime` (CLAUDE.md
    /// MariaDB type gotcha #4).
    pub async fn create(
        pool: &DbPool,
        user_id: u64,
        action: &str,
        entity_type: Option<&str>,
        entity_id: Option<u64>,
        details: Option<serde_json::Value>,
    ) -> Result<AdminAuditEntry, AppError> {
        // R3-N4: pin the INSERT and the follow-up SELECT to the SAME
        // pooled connection. The id we read is captured from the INSERT's
        // result, so the SELECT-by-id is correct on either connection —
        // but pinning protects us from any future refactor that reaches
        // for `LAST_INSERT_ID()`-like session state, and avoids a
        // gratuitous round-trip through pool acquisition for the SELECT.
        let mut conn = pool.acquire().await?;

        let result = sqlx::query(
            "INSERT INTO admin_audit (user_id, action, entity_type, entity_id, details) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(user_id as i64)
        .bind(action)
        .bind(entity_type)
        .bind(entity_id.map(|id| id as i64))
        .bind(details.clone())
        .execute(&mut *conn)
        .await?;

        let id = result.last_insert_id();

        // Issue #80: read back the DB-assigned timestamp so the in-memory
        // struct matches the persisted row exactly — but DON'T propagate a
        // SELECT failure as `Err`. The INSERT has already committed; bubbling
        // up an error would tell the caller "failure" while the audit row is
        // permanently stored, leaving the admin with a 500 after a hard delete
        // already happened. Fall back to the local clock and log a warning
        // instead. The drift vs DB is at most a few ms.
        let timestamp: NaiveDateTime = match sqlx::query(
            "SELECT CAST(timestamp AS DATETIME) AS ts FROM admin_audit WHERE id = ?",
        )
        .bind(id as i64)
        .fetch_optional(&mut *conn)
        .await
        {
            Ok(Some(row)) => row.get("ts"),
            Ok(None) => {
                tracing::warn!(
                    audit_id = id,
                    "audit row inserted but follow-up SELECT returned no row (purged or dropped between INSERT and SELECT?) — using local time"
                );
                chrono::Utc::now().naive_utc()
            }
            Err(e) => {
                tracing::warn!(
                    audit_id = id,
                    error = %e,
                    "audit row inserted but timestamp re-fetch failed — using local time"
                );
                chrono::Utc::now().naive_utc()
            }
        };

        Ok(AdminAuditEntry {
            id,
            user_id: Some(user_id),
            action: action.to_string(),
            entity_type: entity_type.map(|s| s.to_string()),
            entity_id,
            timestamp,
            details,
        })
    }

    // Fix #72 — `AdminAuditModel::list` was added during story 8-7
    // (Story 8-7 DF2) but no acceptance criterion / production caller
    // ever required it. The audit list is purely append-only forensics
    // accessed via direct SQL when needed (or the admin Trash panel,
    // which goes through `TrashModel`, not this model). Keeping the
    // method as dead-code invited future maintainers to wire it into
    // an admin UI without first deciding pagination / filtering UX
    // (Story 8-7 P21 deliberately deferred that work). Deleted, along
    // with `test_admin_audit_list` which only existed to exercise it.
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[sqlx::test(migrations = "./migrations")]
    async fn test_admin_audit_create(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        let entry = AdminAuditModel::create(
            &pool,
            1,
            "permanent_delete_from_trash",
            Some("titles"),
            Some(42),
            Some(json!({"item_name": "Test Title"})),
        )
        .await?;

        assert!(entry.id > 0);
        assert_eq!(entry.user_id, Some(1));
        assert_eq!(entry.action, "permanent_delete_from_trash");
        assert_eq!(entry.entity_type, Some("titles".to_string()));
        assert_eq!(entry.entity_id, Some(42));

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn test_admin_audit_create_system_action(
        pool: sqlx::Pool<sqlx::MySql>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let entry = AdminAuditModel::create(
            &pool,
            1,
            "auto_purge",
            None,
            None,
            Some(json!({"titles": 5, "volumes": 12})),
        )
        .await?;

        assert!(entry.id > 0);
        assert_eq!(entry.action, "auto_purge");
        assert_eq!(entry.entity_type, None);
        assert_eq!(entry.entity_id, None);

        Ok(())
    }

    // Fix #72 — `test_admin_audit_list` removed along with the
    // `AdminAuditModel::list` method it exercised (see comment in
    // the impl block).
}
