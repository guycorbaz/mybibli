use crate::db::DbPool;
use crate::error::AppError;

/// Allowed table names for soft-delete operations (whitelist prevents SQL injection).
const ALLOWED_TABLES: &[&str] = &[
    "titles",
    "volumes",
    "contributors",
    "storage_locations",
    "borrowers",
    "series",
];

pub struct SoftDeleteService;

impl SoftDeleteService {
    /// Soft-delete an entity by setting `deleted_at = NOW()`.
    /// Table name is validated against a whitelist to prevent SQL injection.
    pub async fn soft_delete(pool: &DbPool, table: &str, id: u64) -> Result<(), AppError> {
        if !ALLOWED_TABLES.contains(&table) {
            return Err(AppError::BadRequest(format!(
                "Invalid entity type: {table}"
            )));
        }

        let query_str = format!(
            "UPDATE {} SET deleted_at = NOW() WHERE id = ? AND deleted_at IS NULL",
            table
        );

        let result = sqlx::query(&query_str).bind(id).execute(pool).await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                rust_i18n::t!("error.not_found").to_string(),
            ));
        }

        tracing::info!(table = %table, id = id, "Entity soft-deleted");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_tables() {
        assert!(ALLOWED_TABLES.contains(&"titles"));
        assert!(ALLOWED_TABLES.contains(&"volumes"));
        assert!(ALLOWED_TABLES.contains(&"contributors"));
        assert!(ALLOWED_TABLES.contains(&"storage_locations"));
        assert!(ALLOWED_TABLES.contains(&"borrowers"));
        assert!(ALLOWED_TABLES.contains(&"series"));
    }

    #[test]
    fn test_disallowed_table_rejected() {
        // Cannot test async without DB, but verify the whitelist check logic
        assert!(!ALLOWED_TABLES.contains(&"users"));
        assert!(!ALLOWED_TABLES.contains(&"sessions"));
        assert!(!ALLOWED_TABLES.contains(&"settings"));
        assert!(!ALLOWED_TABLES.contains(&"DROP TABLE titles; --"));
    }

    #[test]
    fn test_sql_injection_in_table_name() {
        // Verify injection attempts fail whitelist
        assert!(!ALLOWED_TABLES.contains(&"titles; DROP TABLE users"));
        assert!(!ALLOWED_TABLES.contains(&"' OR 1=1 --"));
    }
}
