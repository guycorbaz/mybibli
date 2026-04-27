use chrono::NaiveDateTime;
use sqlx::Row;
use crate::db::DbPool;
use crate::error::AppError;

#[derive(Clone, Debug)]
pub struct AdminAuditEntry {
    pub id: u64,
    pub user_id: u64,
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

        // Read back the DB-assigned timestamp so the in-memory struct matches
        // the persisted row exactly.
        let row = sqlx::query(
            "SELECT CAST(timestamp AS DATETIME) AS ts FROM admin_audit WHERE id = ?"
        )
        .bind(id as i64)
        .fetch_one(&mut *conn)
        .await?;
        let timestamp: NaiveDateTime = row.get("ts");

        Ok(AdminAuditEntry {
            id,
            user_id,
            action: action.to_string(),
            entity_type: entity_type.map(|s| s.to_string()),
            entity_id,
            timestamp,
            details,
        })
    }

    /// Fetch audit entries with optional filtering.
    ///
    /// `details` is read with `CAST(... AS CHAR)` because MariaDB stores JSON
    /// columns as `BLOB` underneath; without the cast SQLx can't decode it
    /// into a `String` (CLAUDE.md MariaDB type gotcha #1) — Patch P14.
    pub async fn list(
        pool: &DbPool,
        user_id: Option<u64>,
        action: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AdminAuditEntry>, AppError> {
        let mut query_builder = String::from(
            "SELECT CAST(id AS SIGNED) as id, CAST(user_id AS SIGNED) as user_id, action, \
             entity_type, CAST(entity_id AS SIGNED) as entity_id, \
             CAST(timestamp AS DATETIME) as timestamp, CAST(details AS CHAR) as details \
             FROM admin_audit WHERE 1=1",
        );

        if user_id.is_some() {
            query_builder.push_str(" AND user_id = ?");
        }
        if action.is_some() {
            query_builder.push_str(" AND action = ?");
        }
        query_builder.push_str(" ORDER BY timestamp DESC LIMIT ? OFFSET ?");

        let mut query = sqlx::query(&query_builder);

        if let Some(uid) = user_id {
            query = query.bind(uid as i64);
        }
        if let Some(act) = action {
            query = query.bind(act);
        }
        query = query.bind(limit).bind(offset);

        let rows = query.fetch_all(pool).await?;

        Ok(rows
            .iter()
            .map(|r| {
                let id = r.get::<i64, _>("id") as u64;
                AdminAuditEntry {
                    id,
                    user_id: r.get::<i64, _>("user_id") as u64,
                    action: r.get::<String, _>("action"),
                    entity_type: r.get::<Option<String>, _>("entity_type"),
                    entity_id: r.get::<Option<i64>, _>("entity_id").map(|id| id as u64),
                    timestamp: r.get::<NaiveDateTime, _>("timestamp"),
                    // R3-N13: log parse failures instead of swallowing them.
                    // A NULL `details` column is a normal None; invalid JSON
                    // (which "shouldn't happen" because INSERTs only ever go
                    // through `create()`) deserves visibility.
                    details: r.get::<Option<String>, _>("details").and_then(|s| {
                        match serde_json::from_str(&s) {
                            Ok(v) => Some(v),
                            Err(e) => {
                                tracing::warn!(
                                    audit_id = id,
                                    error = %e,
                                    "admin_audit.details JSON parse failed; row will surface with details = None"
                                );
                                None
                            }
                        }
                    }),
                }
            })
            .collect())
    }
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
        assert_eq!(entry.user_id, 1);
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

    #[sqlx::test(migrations = "./migrations")]
    async fn test_admin_audit_list(pool: sqlx::Pool<sqlx::MySql>) -> Result<(), Box<dyn std::error::Error>> {
        AdminAuditModel::create(&pool, 1, "test_action", Some("titles"), Some(1), None).await?;
        AdminAuditModel::create(&pool, 1, "test_action", Some("volumes"), Some(2), None).await?;
        AdminAuditModel::create(&pool, 2, "other_action", Some("titles"), Some(3), None).await?;

        let user_1_entries = AdminAuditModel::list(&pool, Some(1), None, 10, 0).await?;
        assert_eq!(user_1_entries.len(), 2);

        let action_entries = AdminAuditModel::list(&pool, None, Some("test_action"), 10, 0).await?;
        assert_eq!(action_entries.len(), 2);

        Ok(())
    }
}
