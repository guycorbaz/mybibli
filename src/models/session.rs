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
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT data FROM sessions WHERE token = ? AND deleted_at IS NULL",
        )
        .bind(token)
        .fetch_optional(pool)
        .await?;

        let current_data = row.map(|r| r.0).unwrap_or_else(|| "{}".to_string());
        let mut data: serde_json::Value = match serde_json::from_str(&current_data) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "Corrupt session data JSON, resetting");
                serde_json::json!({})
            }
        };

        data["current_title_id"] = serde_json::json!(title_id);

        let result = sqlx::query("UPDATE sessions SET data = ? WHERE token = ? AND deleted_at IS NULL")
            .bind(data.to_string())
            .bind(token)
            .execute(pool)
            .await?;

        if result.rows_affected() == 0 {
            tracing::warn!(token = "***", "Session not found for current title update");
        }

        tracing::debug!(title_id = title_id, "Updated current title in session");
        Ok(())
    }

    pub async fn get_current_title_id(
        pool: &DbPool,
        token: &str,
    ) -> Result<Option<u64>, AppError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT data FROM sessions WHERE token = ? AND deleted_at IS NULL",
        )
        .bind(token)
        .fetch_optional(pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let data: serde_json::Value = match serde_json::from_str(&row.0) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "Corrupt session data JSON, resetting");
                serde_json::json!({})
            }
        };

        Ok(data.get("current_title_id").and_then(|v| v.as_u64()))
    }
}
