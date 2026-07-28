use crate::db::DbPool;
use crate::error::AppError;
use crate::models::location::LocationModel;
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;

/// Outcome of a V-code scan that lands a volume on a title (#442).
///
/// A soft-deleted volume keeps its label locked by the global `UNIQUE` index,
/// so re-sticking a physical label used to be impossible without an admin
/// round-trip through the Trash. `ReusedLabel` is the transparent-reactivation
/// path — same shape as `CreateOutcome::Reactivated` for reference data — and
/// is reported differently to the librarian, because it discards the previous
/// copy's data.
#[derive(Debug)]
pub enum VolumeCreation {
    Created(VolumeModel),
    ReusedLabel(VolumeModel),
}

impl VolumeCreation {
    pub fn volume(&self) -> &VolumeModel {
        match self {
            VolumeCreation::Created(v) | VolumeCreation::ReusedLabel(v) => v,
        }
    }

    pub fn into_volume(self) -> VolumeModel {
        match self {
            VolumeCreation::Created(v) | VolumeCreation::ReusedLabel(v) => v,
        }
    }
}

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
    ///
    /// `actor_user_id` is the librarian performing the scan; it is only used to
    /// attribute the audit entry written when a soft-deleted label is reused
    /// (#442), and may be `None` for non-interactive callers.
    pub async fn create_volume(
        pool: &DbPool,
        label: &str,
        title_id: u64,
        actor_user_id: Option<u64>,
    ) -> Result<VolumeCreation, AppError> {
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
            Ok(vol) => Ok(VolumeCreation::Created(vol)),
            Err(AppError::BadRequest(msg)) if msg.starts_with("DUPLICATE_LABEL:") => {
                Self::resolve_duplicate_label(pool, label, title_id, actor_user_id).await
            }
            Err(e) => Err(e),
        }
    }

    /// #442 — the label already exists. Decide between "taken", "reusable" and
    /// "in the Trash but not reusable".
    ///
    /// The lookup MUST include soft-deleted rows: `volumes.label` is globally
    /// UNIQUE and soft-deletion does not release it, so the previous
    /// `find_by_label` (which filters `deleted_at IS NULL`) returned `None` for
    /// exactly the collision it was trying to explain — and the message named
    /// the owner `"?"`.
    async fn resolve_duplicate_label(
        pool: &DbPool,
        label: &str,
        title_id: u64,
        actor_user_id: Option<u64>,
    ) -> Result<VolumeCreation, AppError> {
        let Some((existing, is_deleted)) =
            VolumeModel::find_by_label_any_state(pool, label).await?
        else {
            // The row vanished between the failed INSERT and this lookup — a
            // concurrent hard delete. Retrying is the honest advice.
            return Err(AppError::Conflict(
                rust_i18n::t!("feedback.volume_label_race", label = label).to_string(),
            ));
        };

        if !is_deleted {
            // Live volume — the label is genuinely taken.
            let owner = Self::get_volume_title_name(pool, &existing).await;
            return Err(AppError::BadRequest(
                rust_i18n::t!("feedback.volume_duplicate", label = label, title = &owner)
                    .to_string(),
            ));
        }

        // Soft-deleted. Reuse is safe only when nothing references the row.
        let loans = VolumeModel::count_loans(pool, existing.id).await?;
        if loans > 0 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("feedback.volume_label_in_trash_with_loans", label = label)
                    .to_string(),
            ));
        }

        // Record what we are about to discard BEFORE the update, so the audit
        // entry can restore it by hand if the reuse was a mistake.
        let discarded = serde_json::json!({
            "label": label,
            "previous_title_id": existing.title_id,
            "previous_location_id": existing.location_id,
            "previous_condition_state_id": existing.condition_state_id,
            "previous_edition_comment": existing.edition_comment,
            "previous_purchase_price": existing.purchase_price,
            "previous_purchase_currency": existing.purchase_currency,
            "previous_current_value": existing.current_value,
            "previous_current_value_currency": existing.current_value_currency,
            "new_title_id": title_id,
        });

        VolumeModel::reactivate_for_reuse(pool, existing.id, title_id).await?;

        if let Some(user_id) = actor_user_id
            && let Err(e) = crate::models::admin_audit::AdminAuditModel::create(
                pool,
                user_id,
                "volume_label_reused",
                Some("volumes"),
                Some(existing.id),
                Some(discarded),
            )
            .await
        {
            // Best-effort: a missing audit row must not undo a successful
            // reuse, but it is worth shouting about.
            tracing::warn!(error = %e, volume_id = existing.id, "Failed to audit volume label reuse");
        }

        let refreshed = VolumeModel::find_by_label(pool, label)
            .await?
            .ok_or_else(|| AppError::Internal("reactivated volume vanished".to_string()))?;
        Ok(VolumeCreation::ReusedLabel(refreshed))
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
            .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

        let location = LocationModel::find_by_label(pool, location_label)
            .await?
            .ok_or_else(|| {
                AppError::BadRequest(
                    rust_i18n::t!("feedback.lcode_not_found", label = location_label).to_string(),
                )
            })?;

        // CR #280 — refuse to assign a volume to an organizational
        // container. The catalog scan flow + the volume-edit form
        // both come through here; rejecting at the service layer
        // catches both surfaces with one guard.
        if !location.is_assignable() {
            return Err(AppError::BadRequest(
                rust_i18n::t!(
                    "feedback.location_organizational",
                    label = location_label
                )
                .to_string(),
            ));
        }

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
