use crate::db::DbPool;
use crate::error::AppError;
use crate::models::borrower::BorrowerModel;
use crate::services::soft_delete::SoftDeleteService;

pub struct BorrowerService;

fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl BorrowerService {
    pub async fn create_borrower(
        pool: &DbPool,
        name: &str,
        address: Option<String>,
        email: Option<String>,
        phone: Option<String>,
    ) -> Result<BorrowerModel, AppError> {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("borrower.name_required").to_string(),
            ));
        }

        BorrowerModel::create(
            pool,
            trimmed_name,
            non_empty(&address).as_deref(),
            non_empty(&email).as_deref(),
            non_empty(&phone).as_deref(),
        )
        .await
    }

    pub async fn update_borrower(
        pool: &DbPool,
        id: u64,
        version: i32,
        name: &str,
        address: Option<String>,
        email: Option<String>,
        phone: Option<String>,
    ) -> Result<BorrowerModel, AppError> {
        let trimmed_name = name.trim();
        if trimmed_name.is_empty() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("borrower.name_required").to_string(),
            ));
        }

        BorrowerModel::update_with_locking(
            pool,
            id,
            version,
            trimmed_name,
            non_empty(&address).as_deref(),
            non_empty(&email).as_deref(),
            non_empty(&phone).as_deref(),
        )
        .await
    }

    pub async fn delete_borrower(pool: &DbPool, id: u64) -> Result<(), AppError> {
        let borrower = BorrowerModel::find_by_id(pool, id)
            .await?
            .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

        let active_loans = BorrowerModel::count_active_loans(pool, id).await?;
        if active_loans > 0 {
            return Err(AppError::BadRequest(
                rust_i18n::t!(
                    "borrower.delete_has_loans",
                    name = &borrower.name,
                    count = active_loans
                )
                .to_string(),
            ));
        }

        SoftDeleteService::soft_delete(pool, "borrowers", id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_empty_with_value() {
        assert_eq!(non_empty(&Some("hello".to_string())), Some("hello".to_string()));
    }

    #[test]
    fn test_non_empty_with_whitespace() {
        assert_eq!(non_empty(&Some("  ".to_string())), None);
    }

    #[test]
    fn test_non_empty_with_none() {
        assert_eq!(non_empty(&None), None);
    }

    #[test]
    fn test_non_empty_trims() {
        assert_eq!(non_empty(&Some("  hello  ".to_string())), Some("hello".to_string()));
    }
}
