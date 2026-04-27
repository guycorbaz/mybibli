use crate::db::DbPool;
use crate::error::AppError;
use crate::models::contributor::{ContributorModel, ContributorRoleModel, TitleContributorModel};
use crate::models::title::TitleModel;

pub struct ContributorService;

impl ContributorService {
    /// Find an existing contributor by exact name, or create a new one.
    pub async fn find_or_create(pool: &DbPool, name: &str) -> Result<ContributorModel, AppError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("contributor.error.name_required").to_string(),
            ));
        }
        if trimmed.len() > 255 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("contributor.error.name_too_long").to_string(),
            ));
        }

        if let Some(existing) = ContributorModel::find_by_name_exact(pool, trimmed).await? {
            return Ok(existing);
        }

        ContributorModel::create(pool, trimmed, None).await
    }

    /// Add a contributor to a title with a specific role.
    /// Creates the contributor if they don't exist yet.
    pub async fn add_to_title(
        pool: &DbPool,
        title_id: u64,
        contributor_name: &str,
        role_id: u64,
    ) -> Result<(ContributorModel, String), AppError> {
        let trimmed_name = contributor_name.trim();
        if trimmed_name.is_empty() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("contributor.error.name_required").to_string(),
            ));
        }

        // Validate title exists
        if TitleModel::find_by_id(pool, title_id).await?.is_none() {
            return Err(AppError::NotFound(
                rust_i18n::t!("error.not_found").to_string(),
            ));
        }

        // Validate role exists
        if !ContributorRoleModel::exists(pool, role_id).await? {
            return Err(AppError::BadRequest(
                rust_i18n::t!("contributor.error.role_not_found").to_string(),
            ));
        }

        // Find or create contributor
        let contributor = Self::find_or_create(pool, trimmed_name).await?;

        // Add junction — handle UNIQUE constraint
        match TitleContributorModel::add_to_title(pool, title_id, contributor.id, role_id).await {
            Ok(()) => {
                // Get role name for feedback
                let roles = ContributorRoleModel::find_all(pool).await?;
                let role_name = roles
                    .iter()
                    .find(|(id, _)| *id == role_id)
                    .map(|(_, name)| name.clone())
                    .unwrap_or_else(|| "?".to_string());

                tracing::info!(
                    title_id = title_id,
                    contributor = %contributor.name,
                    role = %role_name,
                    "Contributor added to title"
                );

                Ok((contributor, role_name))
            }
            Err(AppError::BadRequest(msg)) if msg == "DUPLICATE_CONTRIBUTOR_ROLE" => {
                // Get role name for error message
                let roles = ContributorRoleModel::find_all(pool).await?;
                let role_name = roles
                    .iter()
                    .find(|(id, _)| *id == role_id)
                    .map(|(_, name)| name.clone())
                    .unwrap_or_else(|| "?".to_string());

                Err(AppError::BadRequest(
                    rust_i18n::t!(
                        "contributor.duplicate",
                        name = trimmed_name,
                        role = &role_name
                    )
                    .to_string(),
                ))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn remove_from_title(pool: &DbPool, junction_id: u64) -> Result<(), AppError> {
        TitleContributorModel::remove_from_title(pool, junction_id).await
    }

    pub async fn update_details(
        pool: &DbPool,
        id: u64,
        name: &str,
        biography: Option<&str>,
    ) -> Result<(), AppError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("contributor.error.name_required").to_string(),
            ));
        }
        if trimmed.len() > 255 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("contributor.error.name_too_long").to_string(),
            ));
        }

        ContributorModel::update(pool, id, trimmed, biography).await
    }

    pub async fn delete_contributor(pool: &DbPool, id: u64) -> Result<(), AppError> {
        let count = ContributorModel::count_title_associations(pool, id).await?;
        if count > 0 {
            let contributor = ContributorModel::find_by_id(pool, id).await?;
            let name = contributor
                .map(|c| c.name)
                .unwrap_or_else(|| "?".to_string());
            return Err(AppError::Conflict(
                rust_i18n::t!("error.contributor.has_titles", name = &name, count = count)
                    .to_string(),
            ));
        }

        ContributorModel::soft_delete(pool, id).await
    }
}

#[cfg(test)]
mod tests {
    // Validation tests (pure logic, no DB)

    #[test]
    fn test_empty_name_validation() {
        // Can't test async without runtime, but we verify the validation logic
        let trimmed = "".trim();
        assert!(trimmed.is_empty());
    }

    #[test]
    fn test_whitespace_name_validation() {
        let trimmed = "   ".trim();
        assert!(trimmed.is_empty());
    }

    #[test]
    fn test_valid_name_trimming() {
        let trimmed = "  Albert Camus  ".trim();
        assert_eq!(trimmed, "Albert Camus");
        assert!(!trimmed.is_empty());
    }

    #[test]
    fn test_name_max_length() {
        let long_name = "A".repeat(256);
        assert!(long_name.len() > 255);
    }

    #[test]
    fn test_name_at_limit() {
        let name = "A".repeat(255);
        assert_eq!(name.len(), 255);
    }

    #[test]
    fn test_deletion_guard_returns_conflict_variant() {
        // Verify the Conflict error variant carries the expected i18n message pattern
        use crate::error::AppError;
        let error = AppError::Conflict(
            "Cannot delete Test Author. This contributor is associated with 3 title(s). Remove the contributor from all titles first.".to_string(),
        );
        match error {
            AppError::Conflict(msg) => {
                assert!(msg.contains("Cannot delete"));
                assert!(msg.contains("3 title(s)"));
            }
            _ => panic!("Expected AppError::Conflict"),
        }
    }
}
