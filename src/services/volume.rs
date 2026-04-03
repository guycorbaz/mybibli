use crate::db::DbPool;
use crate::error::AppError;
use crate::models::location::LocationModel;
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;

pub struct VolumeService;

impl VolumeService {
    /// Validate V-code format: uppercase V + exactly 4 digits, V0001-V9999.
    pub fn validate_vcode(label: &str) -> bool {
        if label.len() != 5 {
            return false;
        }
        if !label.starts_with('V') {
            return false;
        }
        let digits = &label[1..];
        if !digits.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        // Reject V0000
        digits != "0000"
    }

    /// Create a new volume attached to the given title.
    /// Validates V-code format, checks title exists, checks label uniqueness.
    pub async fn create_volume(
        pool: &DbPool,
        label: &str,
        title_id: u64,
    ) -> Result<VolumeModel, AppError> {
        if !Self::validate_vcode(label) {
            return Err(AppError::BadRequest(
                rust_i18n::t!("feedback.vcode_invalid").to_string(),
            ));
        }

        // Verify title exists
        let title = TitleModel::find_by_id(pool, title_id).await?;
        if title.is_none() {
            return Err(AppError::NotFound(
                rust_i18n::t!("error.not_found").to_string(),
            ));
        }

        // Create volume — handle UNIQUE constraint with user-friendly message
        match VolumeModel::create(pool, title_id, label).await {
            Ok(vol) => Ok(vol),
            Err(AppError::BadRequest(msg)) if msg.starts_with("DUPLICATE_LABEL:") => {
                // Duplicate label — look up which title owns the existing volume
                let existing_title = if let Some(existing_vol) = VolumeModel::find_by_label(pool, label).await? {
                    Self::get_volume_title_name(pool, &existing_vol).await
                } else {
                    "?".to_string()
                };
                Err(AppError::BadRequest(
                    rust_i18n::t!("feedback.volume_duplicate", label = label, title = &existing_title).to_string(),
                ))
            }
            Err(e) => Err(e),
        }
    }

    /// Assign a location to a volume by their labels.
    /// Returns the volume and the location path string.
    pub async fn assign_location(
        pool: &DbPool,
        volume_label: &str,
        location_label: &str,
    ) -> Result<(VolumeModel, String), AppError> {
        let volume = VolumeModel::find_by_label(pool, volume_label)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(
                    rust_i18n::t!("error.not_found").to_string(),
                )
            })?;

        let location = LocationModel::find_by_label(pool, location_label)
            .await?
            .ok_or_else(|| {
                AppError::BadRequest(
                    rust_i18n::t!("feedback.lcode_not_found", label = location_label).to_string(),
                )
            })?;

        VolumeModel::update_location(pool, volume.id, Some(location.id)).await?;

        let path = LocationModel::get_path(pool, location.id).await?;

        tracing::info!(
            volume_label = %volume_label,
            location_label = %location_label,
            location_path = %path,
            "Volume location assigned"
        );

        // Return volume with updated location_id
        let mut updated_volume = volume;
        updated_volume.location_id = Some(location.id);
        Ok((updated_volume, path))
    }

    /// Get the title name for a volume (for error messages like "already assigned to {title}").
    pub async fn get_volume_title_name(pool: &DbPool, volume: &VolumeModel) -> String {
        match TitleModel::find_by_id(pool, volume.title_id).await {
            Ok(Some(title)) => title.title,
            _ => String::from("?"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_vcode_v0001() {
        assert!(VolumeService::validate_vcode("V0001"));
    }

    #[test]
    fn test_valid_vcode_v9999() {
        assert!(VolumeService::validate_vcode("V9999"));
    }

    #[test]
    fn test_valid_vcode_v0042() {
        assert!(VolumeService::validate_vcode("V0042"));
    }

    #[test]
    fn test_invalid_vcode_v0000() {
        assert!(!VolumeService::validate_vcode("V0000"));
    }

    #[test]
    fn test_invalid_vcode_too_short() {
        assert!(!VolumeService::validate_vcode("V123"));
    }

    #[test]
    fn test_invalid_vcode_too_long() {
        assert!(!VolumeService::validate_vcode("V00001"));
    }

    #[test]
    fn test_invalid_vcode_non_numeric() {
        assert!(!VolumeService::validate_vcode("VABCD"));
    }

    #[test]
    fn test_invalid_vcode_lowercase() {
        assert!(!VolumeService::validate_vcode("v0042"));
    }

    #[test]
    fn test_invalid_vcode_no_prefix() {
        assert!(!VolumeService::validate_vcode("00042"));
    }

    #[test]
    fn test_invalid_vcode_empty() {
        assert!(!VolumeService::validate_vcode(""));
    }

    #[test]
    fn test_invalid_vcode_just_v() {
        assert!(!VolumeService::validate_vcode("V"));
    }
}
