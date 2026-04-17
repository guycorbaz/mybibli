use crate::db::DbPool;
use crate::error::AppError;
use crate::models::location::LocationModel;

pub struct LocationService;

impl LocationService {
    /// Validate L-code format: uppercase L + exactly 4 digits, L0000 rejected.
    pub fn validate_lcode(label: &str) -> bool {
        if label.len() != 5 {
            return false;
        }
        if !label.starts_with('L') {
            return false;
        }
        if label == "L0000" {
            return false;
        }
        label[1..].chars().all(|c| c.is_ascii_digit())
    }

    /// Propose the next available L-code (MAX existing + 1).
    pub async fn get_next_available_lcode(pool: &DbPool) -> Result<String, AppError> {
        let row: Option<(Option<i64>,)> = sqlx::query_as(
            "SELECT CAST(MAX(CAST(SUBSTRING(label, 2) AS UNSIGNED)) AS SIGNED) \
             FROM storage_locations WHERE deleted_at IS NULL",
        )
        .fetch_optional(pool)
        .await?;

        let max_num = row.and_then(|r| r.0).unwrap_or(0);
        let next = max_num + 1;
        if next > 9999 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("location.lcode_exhausted").to_string(),
            ));
        }
        Ok(format!("L{:04}", next))
    }

    /// Create a new location in the hierarchy.
    pub async fn create_location(
        pool: &DbPool,
        name: &str,
        node_type: &str,
        parent_id: Option<u64>,
        label: &str,
    ) -> Result<LocationModel, AppError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("validation.required").to_string(),
            ));
        }

        if !Self::validate_lcode(label) {
            return Err(AppError::BadRequest(
                rust_i18n::t!("location.lcode_invalid").to_string(),
            ));
        }

        // Check L-code uniqueness
        if LocationModel::find_by_label(pool, label).await?.is_some() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("location.lcode_duplicate").to_string(),
            ));
        }

        // Validate parent exists if provided
        if let Some(pid) = parent_id
            && LocationModel::find_by_id(pool, pid).await?.is_none()
        {
            return Err(AppError::NotFound(
                rust_i18n::t!("error.not_found").to_string(),
            ));
        }

        // Validate node_type exists in reference table
        let node_types = LocationModel::find_node_types(pool).await?;
        if !node_types.iter().any(|(_, nt)| nt == node_type) {
            return Err(AppError::BadRequest(
                rust_i18n::t!("location.invalid_node_type").to_string(),
            ));
        }

        let location = LocationModel::create(pool, name, node_type, parent_id, label).await?;
        tracing::info!(id = location.id, name = %name, label = %label, "Location created");
        Ok(location)
    }

    /// Update a location with optimistic locking and cycle detection.
    pub async fn update_location(
        pool: &DbPool,
        id: u64,
        version: i32,
        name: &str,
        node_type: &str,
        parent_id: Option<u64>,
    ) -> Result<LocationModel, AppError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::BadRequest(
                rust_i18n::t!("validation.required").to_string(),
            ));
        }

        // Validate node_type exists in reference table
        let node_types = LocationModel::find_node_types(pool).await?;
        if !node_types.iter().any(|(_, nt)| nt == node_type) {
            return Err(AppError::BadRequest(
                rust_i18n::t!("location.invalid_node_type").to_string(),
            ));
        }

        // Validate parent exists and detect cycles
        if let Some(pid) = parent_id {
            if LocationModel::find_by_id(pool, pid).await?.is_none() {
                return Err(AppError::NotFound(
                    rust_i18n::t!("error.not_found").to_string(),
                ));
            }
            Self::validate_parent_chain(pool, id, pid).await?;
        }

        let location =
            LocationModel::update_with_locking(pool, id, version, name, node_type, parent_id)
                .await?;
        tracing::info!(id = id, name = %name, "Location updated");
        Ok(location)
    }

    /// Delete a location (soft-delete) with guards for children and volumes.
    pub async fn delete_location(pool: &DbPool, id: u64) -> Result<(), AppError> {
        // Check for child locations
        let children_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM storage_locations WHERE parent_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        if children_count.0 > 0 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("location.has_children").to_string(),
            ));
        }

        // Check for volumes at this location
        let volume_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM volumes WHERE location_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        if volume_count.0 > 0 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("location.has_volumes", count = volume_count.0).to_string(),
            ));
        }

        crate::services::soft_delete::SoftDeleteService::soft_delete(pool, "storage_locations", id)
            .await?;

        tracing::info!(id = id, "Location deleted");
        Ok(())
    }

    /// Validate that setting parent_id won't create a cycle.
    /// Walks from new_parent_id upward; if target_id is found, it's a cycle.
    pub async fn validate_parent_chain(
        pool: &DbPool,
        target_id: u64,
        new_parent_id: u64,
    ) -> Result<(), AppError> {
        if target_id == new_parent_id {
            return Err(AppError::BadRequest(
                rust_i18n::t!("location.cycle_detected").to_string(),
            ));
        }

        const MAX_DEPTH: usize = 20;
        let mut current_id = Some(new_parent_id);
        let mut depth = 0;

        while let Some(cid) = current_id {
            if depth >= MAX_DEPTH {
                return Err(AppError::BadRequest(
                    rust_i18n::t!("location.cycle_detected").to_string(),
                ));
            }
            if cid == target_id {
                return Err(AppError::BadRequest(
                    rust_i18n::t!("location.cycle_detected").to_string(),
                ));
            }
            let row: Option<(Option<i64>,)> = sqlx::query_as(
                "SELECT CAST(parent_id AS SIGNED) as parent_id FROM storage_locations WHERE id = ? AND deleted_at IS NULL",
            )
            .bind(cid)
            .fetch_optional(pool)
            .await?;

            current_id = row.and_then(|r| r.0.map(|v| v as u64));
            depth += 1;
        }

        Ok(())
    }

    /// Get recursive volume count for a location and all its descendants.
    pub async fn get_recursive_volume_count(pool: &DbPool, id: u64) -> Result<u64, AppError> {
        let row: (i64,) = sqlx::query_as(
            "WITH RECURSIVE descendants AS ( \
                 SELECT id FROM storage_locations WHERE id = ? AND deleted_at IS NULL \
                 UNION ALL \
                 SELECT sl.id FROM storage_locations sl \
                 JOIN descendants d ON sl.parent_id = d.id \
                 WHERE sl.deleted_at IS NULL \
             ) \
             SELECT COUNT(*) FROM volumes v \
             JOIN descendants d ON v.location_id = d.id \
             WHERE v.deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(pool)
        .await?;

        Ok(row.0 as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_lcode_valid() {
        assert!(LocationService::validate_lcode("L0001"));
        assert!(LocationService::validate_lcode("L9999"));
        assert!(LocationService::validate_lcode("L0042"));
    }

    #[test]
    fn test_validate_lcode_l0000_rejected() {
        assert!(!LocationService::validate_lcode("L0000"));
    }

    #[test]
    fn test_validate_lcode_invalid_prefix() {
        assert!(!LocationService::validate_lcode("V0001"));
        assert!(!LocationService::validate_lcode("X0001"));
    }

    #[test]
    fn test_validate_lcode_wrong_length() {
        assert!(!LocationService::validate_lcode("L001"));
        assert!(!LocationService::validate_lcode("L00001"));
        assert!(!LocationService::validate_lcode(""));
    }

    #[test]
    fn test_validate_lcode_non_numeric() {
        assert!(!LocationService::validate_lcode("LABCD"));
        assert!(!LocationService::validate_lcode("L00A1"));
    }

    #[test]
    fn test_validate_lcode_lowercase() {
        assert!(!LocationService::validate_lcode("l0001"));
    }
}
