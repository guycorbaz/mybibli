use crate::db::DbPool;
use crate::error::AppError;

pub struct SessionRow {
    pub token: String,
    pub user_id: Option<u64>,
    pub role: String,
}

pub struct SessionModel;

impl SessionModel {
    pub async fn find_with_role(
        pool: &DbPool,
        token: &str,
    ) -> Result<Option<SessionRow>, AppError> {
        let row = sqlx::query_as!(
            SessionRow,
            r#"SELECT s.token, s.user_id, u.role as `role: String`
               FROM sessions s
               JOIN users u ON s.user_id = u.id
               WHERE s.token = ?
                 AND s.deleted_at IS NULL
                 AND u.deleted_at IS NULL
                 AND s.last_activity > DATE_SUB(NOW(), INTERVAL 4 HOUR)"#,
            token
        )
        .fetch_optional(pool)
        .await?;

        Ok(row)
    }

    pub async fn update_last_activity(
        pool: &DbPool,
        token: &str,
    ) -> Result<(), AppError> {
        sqlx::query!(
            "UPDATE sessions SET last_activity = NOW() WHERE token = ?",
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

    pub async fn get_current_title_id(
        pool: &DbPool,
        token: &str,
    ) -> Result<Option<u64>, AppError> {
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
        Ok(data.get("last_volume_label")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from))
    }

    pub async fn increment_session_counter(
        pool: &DbPool,
        token: &str,
    ) -> Result<u64, AppError> {
        let mut data = Self::load_session_data(pool, token).await?;
        let count = data.get("session_item_count").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
        data["session_item_count"] = serde_json::json!(count);
        Self::save_session_data(pool, token, &data).await?;
        Ok(count)
    }

    pub async fn get_session_counter(
        pool: &DbPool,
        token: &str,
    ) -> Result<u64, AppError> {
        let data = Self::load_session_data(pool, token).await?;
        Ok(data.get("session_item_count").and_then(|v| v.as_u64()).unwrap_or(0))
    }

    // ─── Internal helpers ─────────────────────────────────────────

    async fn load_session_data(pool: &DbPool, token: &str) -> Result<serde_json::Value, AppError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT data FROM sessions WHERE token = ? AND deleted_at IS NULL",
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
        let result = sqlx::query(
            "UPDATE sessions SET data = ? WHERE token = ? AND deleted_at IS NULL",
        )
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
