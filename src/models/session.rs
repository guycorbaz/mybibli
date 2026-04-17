use crate::db::DbPool;
use crate::error::AppError;

pub struct SessionRow {
    pub token: String,
    pub user_id: Option<u64>,
    pub role: String,
    pub last_activity: chrono::DateTime<chrono::Utc>,
    /// Stored `users.preferred_language` (`'fr'`/`'en'`). `None` when the user
    /// has not picked a language — locale resolution then falls through to the
    /// cookie / `Accept-Language` / default chain.
    pub preferred_language: Option<String>,
}

pub struct SessionModel;

impl SessionModel {
    /// Fetch the session row joined with the owning user's role. The caller is
    /// responsible for comparing `last_activity` against the configured
    /// `session_timeout_secs` and deciding whether to treat the session as
    /// expired — see `src/middleware/auth.rs`.
    pub async fn find_with_role(
        pool: &DbPool,
        token: &str,
    ) -> Result<Option<SessionRow>, AppError> {
        let row = sqlx::query_as!(
            SessionRow,
            r#"SELECT s.token,
                      s.user_id,
                      u.role as `role: String`,
                      s.last_activity,
                      u.preferred_language as `preferred_language?: String`
               FROM sessions s
               JOIN users u ON s.user_id = u.id
               WHERE s.token = ?
                 AND s.deleted_at IS NULL
                 AND u.deleted_at IS NULL"#,
            token
        )
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    /// Returns true if `last_activity` is more than `timeout_secs` ago compared
    /// to `now`. Extracted as a pure function to make expiry boundaries trivial
    /// to unit-test.
    pub fn is_expired(
        last_activity: chrono::DateTime<chrono::Utc>,
        now: chrono::DateTime<chrono::Utc>,
        timeout_secs: u64,
    ) -> bool {
        // Saturate to i64::MAX so a pathological u64 setting (above i64::MAX)
        // cannot wrap to a negative Duration and expire every session instantly.
        let clamped = i64::try_from(timeout_secs).unwrap_or(i64::MAX);
        let elapsed = now.signed_duration_since(last_activity);
        elapsed > chrono::Duration::seconds(clamped)
    }

    pub async fn update_last_activity(pool: &DbPool, token: &str) -> Result<(), AppError> {
        // UTC_TIMESTAMP() — explicit UTC, independent of the MariaDB server
        // `time_zone` setting. Comparison in the auth extractor uses
        // `chrono::Utc::now()`, so both sides of the inequality are UTC.
        sqlx::query!(
            "UPDATE sessions SET last_activity = UTC_TIMESTAMP() WHERE token = ?",
            token
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn set_current_title(
        pool: &DbPool,
        token: &str,
        title_id: u64,
    ) -> Result<(), AppError> {
        let mut data = Self::load_session_data(pool, token).await?;
        data["current_title_id"] = serde_json::json!(title_id);
        Self::save_session_data(pool, token, &data).await?;
        tracing::debug!(title_id = title_id, "Updated current title in session");
        Ok(())
    }

    pub async fn get_current_title_id(pool: &DbPool, token: &str) -> Result<Option<u64>, AppError> {
        let data = Self::load_session_data(pool, token).await?;
        Ok(data.get("current_title_id").and_then(|v| v.as_u64()))
    }

    pub async fn set_last_volume_label(
        pool: &DbPool,
        token: &str,
        label: &str,
    ) -> Result<(), AppError> {
        let mut data = Self::load_session_data(pool, token).await?;
        data["last_volume_label"] = serde_json::json!(label);
        Self::save_session_data(pool, token, &data).await
    }

    pub async fn get_last_volume_label(
        pool: &DbPool,
        token: &str,
    ) -> Result<Option<String>, AppError> {
        let data = Self::load_session_data(pool, token).await?;
        Ok(data
            .get("last_volume_label")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from))
    }

    pub async fn increment_session_counter(pool: &DbPool, token: &str) -> Result<u64, AppError> {
        let mut data = Self::load_session_data(pool, token).await?;
        let count = data
            .get("session_item_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            + 1;
        data["session_item_count"] = serde_json::json!(count);
        Self::save_session_data(pool, token, &data).await?;
        Ok(count)
    }

    pub async fn set_active_location(
        pool: &DbPool,
        token: &str,
        location_id: u64,
    ) -> Result<(), AppError> {
        let mut data = Self::load_session_data(pool, token).await?;
        data["active_location_id"] = serde_json::json!(location_id);
        Self::save_session_data(pool, token, &data).await
    }

    pub async fn get_active_location(pool: &DbPool, token: &str) -> Result<Option<u64>, AppError> {
        let data = Self::load_session_data(pool, token).await?;
        Ok(data.get("active_location_id").and_then(|v| v.as_u64()))
    }

    pub async fn clear_active_location(pool: &DbPool, token: &str) -> Result<(), AppError> {
        let mut data = Self::load_session_data(pool, token).await?;
        data.as_object_mut().map(|o| o.remove("active_location_id"));
        Self::save_session_data(pool, token, &data).await
    }

    pub async fn get_session_counter(pool: &DbPool, token: &str) -> Result<u64, AppError> {
        let data = Self::load_session_data(pool, token).await?;
        Ok(data
            .get("session_item_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0))
    }

    // ─── Internal helpers ─────────────────────────────────────────

    async fn load_session_data(pool: &DbPool, token: &str) -> Result<serde_json::Value, AppError> {
        // CAST to CHAR because MariaDB stores JSON as BLOB internally,
        // which is incompatible with sqlx's String decoder
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT CAST(data AS CHAR) FROM sessions WHERE token = ? AND deleted_at IS NULL",
        )
        .bind(token)
        .fetch_optional(pool)
        .await?;

        let raw = row.map(|r| r.0).unwrap_or_else(|| "{}".to_string());
        match serde_json::from_str(&raw) {
            Ok(v) => Ok(v),
            Err(e) => {
                tracing::warn!(error = %e, "Corrupt session data JSON, resetting");
                Ok(serde_json::json!({}))
            }
        }
    }

    async fn save_session_data(
        pool: &DbPool,
        token: &str,
        data: &serde_json::Value,
    ) -> Result<(), AppError> {
        let result =
            sqlx::query("UPDATE sessions SET data = ? WHERE token = ? AND deleted_at IS NULL")
                .bind(data.to_string())
                .bind(token)
                .execute(pool)
                .await?;

        if result.rows_affected() == 0 {
            tracing::warn!("Session not found for data update");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn test_is_expired_boundary_not_yet_expired() {
        let now = Utc::now();
        let last = now - Duration::seconds(59);
        assert!(!SessionModel::is_expired(last, now, 60));
    }

    #[test]
    fn test_is_expired_boundary_just_expired() {
        let now = Utc::now();
        let last = now - Duration::seconds(61);
        assert!(SessionModel::is_expired(last, now, 60));
    }

    #[test]
    fn test_is_expired_exact_boundary_still_valid() {
        // elapsed == timeout => NOT expired (strict greater-than).
        let now = Utc::now();
        let last = now - Duration::seconds(60);
        assert!(!SessionModel::is_expired(last, now, 60));
    }

    #[test]
    fn test_is_expired_fresh_activity() {
        let now = Utc::now();
        let last = now;
        assert!(!SessionModel::is_expired(last, now, 14400));
    }
}
