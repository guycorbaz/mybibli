use sqlx::MySqlPool;

/// Type alias for the database connection pool.
pub type DbPool = MySqlPool;

/// Create a database connection pool from the given URL.
/// The URL must include `?charset=utf8mb4` for proper Unicode support.
pub async fn create_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    MySqlPool::connect(database_url).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_pool_with_invalid_url_returns_error() {
        let result = create_pool("mysql://invalid:invalid@localhost:99999/nonexistent").await;
        assert!(result.is_err());
    }
}
