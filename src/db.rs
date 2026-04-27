use sqlx::Executor;
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};

/// Type alias for the database connection pool.
pub type DbPool = MySqlPool;

/// Create a database connection pool from the given URL.
/// The URL must include `?charset=utf8mb4` for proper Unicode support.
///
/// R3-N15: every new connection runs `SET time_zone = '+00:00'` so the
/// session's interpretation of `NOW()` and `TIMESTAMP` reads matches the
/// `chrono::Utc::now().naive_utc()` baseline used by application code
/// (notably the auto-purge `NOW() - INTERVAL 30 DAY` filter and the trash
/// panel's `days_remaining` calculation). Without this hook, a server
/// configured with a non-UTC `default-time-zone` would let `deleted_at`
/// reads and Rust-side comparisons drift by the local offset.
pub async fn create_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    MySqlPoolOptions::new()
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                conn.execute("SET time_zone = '+00:00'").await?;
                Ok(())
            })
        })
        .connect(database_url)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_pool_with_invalid_url_returns_error() {
        let result = create_pool("mysql://invalid:invalid@localhost:99999/nonexistent").await;
        assert!(result.is_err());
    }

    /// R3-N15 smoke: confirm the wrapped `create_pool` still reports an
    /// error on an unreachable host. The `after_connect` hook only runs
    /// AFTER the TCP handshake succeeds, so this exercises the same
    /// "fail before hook" path as the original `MySqlPool::connect`.
    #[tokio::test]
    async fn test_create_pool_after_connect_hook_does_not_break_error_path() {
        let result = create_pool("mysql://invalid:invalid@127.0.0.1:1/nonexistent").await;
        assert!(result.is_err());
    }
}
