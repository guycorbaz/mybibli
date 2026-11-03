//! Story 13-4 (#578), code review M1 — markers of the one-shot startup data
//! repairs (`data_repairs` table). See `services::data_repairs::run_once`.

use crate::db::DbPool;
use crate::error::AppError;

pub struct DataRepairModel;

impl DataRepairModel {
    /// Has the repair `name` already run on this database?
    pub async fn is_applied(pool: &DbPool, name: &str) -> Result<bool, AppError> {
        let found: Option<(String,)> =
            sqlx::query_as("SELECT name FROM data_repairs WHERE name = ?")
                .bind(name)
                .fetch_optional(pool)
                .await?;
        Ok(found.is_some())
    }

    /// Record that the repair `name` ran (a second call is a no-op).
    pub async fn mark_applied(pool: &DbPool, name: &str) -> Result<(), AppError> {
        sqlx::query("INSERT IGNORE INTO data_repairs (name) VALUES (?)")
            .bind(name)
            .execute(pool)
            .await?;
        Ok(())
    }
}
