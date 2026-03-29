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
}
