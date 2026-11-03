//! Story 13-4 (#578), code review M1 — markers of the one-shot startup data
//! repairs (`data_repairs` table). See `services::data_repairs::run_once`.

use sqlx::MySqlConnection;

use crate::db::DbPool;
use crate::error::AppError;

pub struct DataRepairModel;

impl DataRepairModel {
    /// Has the repair `name` already run on this database? A locking read
    /// (`FOR UPDATE`). On an absent marker it takes a gap lock: if two
    /// processes boot at the same time, both may pass the check and one of
    /// them then deadlocks on the marker insert — its repair rolls back,
    /// the error is logged as non-fatal by `main.rs`, and the other commits
    /// the repair once. Accepted for a single-tenant deployment.
    pub async fn is_applied_in(conn: &mut MySqlConnection, name: &str) -> Result<bool, AppError> {
        let found: Option<(String,)> =
            sqlx::query_as("SELECT name FROM data_repairs WHERE name = ? FOR UPDATE")
                .bind(name)
                .fetch_optional(&mut *conn)
                .await?;
        Ok(found.is_some())
    }

    /// Has the repair `name` already run? A plain read.
    pub async fn is_applied(pool: &DbPool, name: &str) -> Result<bool, AppError> {
        let found: Option<(String,)> =
            sqlx::query_as("SELECT name FROM data_repairs WHERE name = ?")
                .bind(name)
                .fetch_optional(pool)
                .await?;
        Ok(found.is_some())
    }

    /// Record that the repair `name` ran (a second call is a no-op).
    pub async fn mark_applied_in(conn: &mut MySqlConnection, name: &str) -> Result<(), AppError> {
        sqlx::query("INSERT IGNORE INTO data_repairs (name) VALUES (?)")
            .bind(name)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }
}
