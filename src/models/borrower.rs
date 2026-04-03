use sqlx::Row;

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::{PaginatedList, DEFAULT_PAGE_SIZE};

#[derive(Debug, Clone)]
pub struct BorrowerModel {
    pub id: u64,
    pub name: String,
    pub address: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub version: i32,
}

impl std::fmt::Display for BorrowerModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

fn row_to_borrower(row: sqlx::mysql::MySqlRow) -> Result<BorrowerModel, sqlx::Error> {
    Ok(BorrowerModel {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        address: row.try_get("address")?,
        email: row.try_get("email")?,
        phone: row.try_get("phone")?,
        version: row.try_get("version")?,
    })
}

impl BorrowerModel {
    pub async fn find_by_id(pool: &DbPool, id: u64) -> Result<Option<BorrowerModel>, AppError> {
        let row = sqlx::query(
            "SELECT id, name, address, email, phone, version \
             FROM borrowers WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => Ok(Some(row_to_borrower(r)?)),
            None => Ok(None),
        }
    }

    pub async fn list_active(pool: &DbPool, page: u32) -> Result<PaginatedList<BorrowerModel>, AppError> {
        let offset = (page.saturating_sub(1)) * DEFAULT_PAGE_SIZE;

        let count_row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM borrowers WHERE deleted_at IS NULL",
        )
        .fetch_one(pool)
        .await?;

        let rows = sqlx::query(
            "SELECT id, name, address, email, phone, version \
             FROM borrowers WHERE deleted_at IS NULL \
             ORDER BY name ASC LIMIT ? OFFSET ?",
        )
        .bind(DEFAULT_PAGE_SIZE)
        .bind(offset)
        .fetch_all(pool)
        .await?;

        let items: Vec<BorrowerModel> = rows
            .into_iter()
            .map(row_to_borrower)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(PaginatedList::new(items, page, count_row.0 as u64, None, None, None))
    }

    pub async fn create(
        pool: &DbPool,
        name: &str,
        address: Option<&str>,
        email: Option<&str>,
        phone: Option<&str>,
    ) -> Result<BorrowerModel, AppError> {
        tracing::info!(name = %name, "Creating borrower");

        let result = sqlx::query(
            "INSERT INTO borrowers (name, address, email, phone) VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(address)
        .bind(email)
        .bind(phone)
        .execute(pool)
        .await?;

        let id = result.last_insert_id();
        BorrowerModel::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve created borrower".to_string()))
    }

    pub async fn update_with_locking(
        pool: &DbPool,
        id: u64,
        version: i32,
        name: &str,
        address: Option<&str>,
        email: Option<&str>,
        phone: Option<&str>,
    ) -> Result<BorrowerModel, AppError> {
        let result = sqlx::query(
            "UPDATE borrowers SET name = ?, address = ?, email = ?, phone = ?, \
             version = version + 1, updated_at = NOW() \
             WHERE id = ? AND version = ? AND deleted_at IS NULL",
        )
        .bind(name)
        .bind(address)
        .bind(email)
        .bind(phone)
        .bind(id)
        .bind(version)
        .execute(pool)
        .await?;

        crate::services::locking::check_update_result(result.rows_affected(), "borrower")?;

        BorrowerModel::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to retrieve updated borrower".to_string()))
    }

    pub async fn search_by_name(
        pool: &DbPool,
        query: &str,
        limit: u32,
    ) -> Result<Vec<BorrowerModel>, AppError> {
        let escaped = query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");

        let rows = sqlx::query(
            "SELECT id, name, address, email, phone, version \
             FROM borrowers WHERE name LIKE ? AND deleted_at IS NULL \
             ORDER BY name LIMIT ?",
        )
        .bind(&pattern)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        let items: Vec<BorrowerModel> = rows
            .into_iter()
            .map(row_to_borrower)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(items)
    }

    pub async fn count_active_loans(pool: &DbPool, borrower_id: u64) -> Result<u64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM loans \
             WHERE borrower_id = ? AND returned_at IS NULL AND deleted_at IS NULL",
        )
        .bind(borrower_id)
        .fetch_one(pool)
        .await?;

        Ok(row.0 as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_borrower_display() {
        let b = BorrowerModel {
            id: 1,
            name: "Jean Dupont".to_string(),
            address: Some("123 Rue de Paris".to_string()),
            email: Some("jean@example.com".to_string()),
            phone: Some("+33 6 12 34 56 78".to_string()),
            version: 1,
        };
        assert_eq!(b.to_string(), "Jean Dupont");
    }

    #[test]
    fn test_borrower_minimal() {
        let b = BorrowerModel {
            id: 2,
            name: "Marie".to_string(),
            address: None,
            email: None,
            phone: None,
            version: 1,
        };
        assert_eq!(b.name, "Marie");
        assert!(b.address.is_none());
    }

    #[test]
    fn test_search_escaping() {
        let query = "test%_\\";
        let escaped = query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        assert_eq!(escaped, "test\\%\\_\\\\");
    }
}
