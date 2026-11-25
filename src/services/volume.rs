use crate::db::DbPool;
use crate::error::AppError;
use crate::models::location::LocationModel;
use crate::models::volume::VolumeModel;

/// What an add of a V-code to a title did (story 19-3, D2).
#[derive(Debug)]
pub enum VolumeAdd {
    /// A new volume of the title now carries the label.
    Created(VolumeModel),
    /// The label already labels a live volume — of this title or another;
    /// typically a concurrent post of the same code won the INSERT. The
    /// caller answers it as #585 (`existing_label_response`), never as a
    /// refusal.
    LabelLive(VolumeModel),
}

impl VolumeAdd {
    /// The created volume, `None` when the label was already live.
    pub fn into_created(self) -> Option<VolumeModel> {
        match self {
            VolumeAdd::Created(c) => Some(c),
            VolumeAdd::LabelLive(_) => None,
        }
    }
}

pub struct VolumeService;

impl VolumeService {
    /// Story 17-1 (#651, Rule #20) — the canonical V-code of `code` when it
    /// is `v` or `V` followed by exactly 4 ASCII digits (`v0042` → `V0042`);
    /// `None` for anything else, which is never rewritten. The one rule every
    /// V-code reader goes through (catalog and home scans, loans, search).
    pub fn normalize_vcode(code: &str) -> Option<String> {
        let digits = code.strip_prefix(['v', 'V'])?;
        (digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit()))
            .then(|| format!("V{digits}"))
    }

    /// A volume label as typed in a field: trimmed, and canonical when it is
    /// a V-code of either case (`normalize_vcode`); otherwise kept as typed so
    /// the caller's own check names what is wrong.
    pub fn label_from_input(input: &str) -> String {
        let trimmed = input.trim();
        Self::normalize_vcode(trimmed).unwrap_or_else(|| trimmed.to_string())
    }

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
    /// A label held by a volume in the Trash is refused (story 19-3, D9).
    pub async fn create_volume(
        pool: &DbPool,
        label: &str,
        title_id: u64,
    ) -> Result<VolumeAdd, AppError> {
        let loc = rust_i18n::locale();
        Self::create_volume_in(pool, label, title_id, &loc).await
    }

    /// [`Self::create_volume`] with its refusals worded in `loc`, the
    /// request locale (story 17-2: the shared V-code arm answers in the
    /// librarian's language, #661).
    ///
    /// Story 19-3 (D1, Rule #22): one transaction that share-locks the title
    /// row by primary key first, then inserts.
    /// A title delete takes that row `FOR UPDATE`, so the two serialise: the
    /// add first → the delete counts the new volume and refuses; the delete
    /// first → the add waits, reads the title gone and answers `NotFound`.
    /// No live volume ever lands on a trashed title. Every read before the
    /// INSERT is a locking read, so the plain duplicate lookup after a
    /// refused INSERT sees the row the INSERT waited for.
    pub async fn create_volume_in(
        pool: &DbPool,
        label: &str,
        title_id: u64,
        loc: &str,
    ) -> Result<VolumeAdd, AppError> {
        if !Self::validate_vcode(label) {
            return Err(AppError::BadRequest(
                rust_i18n::t!("feedback.vcode_invalid", locale = loc).to_string(),
            ));
        }

        let mut tx = pool.begin().await?;
        if !crate::models::volume_move::title_is_live_in(&mut tx, title_id).await? {
            tx.rollback().await?;
            return Err(AppError::NotFound(
                rust_i18n::t!("error.not_found", locale = loc).to_string(),
            ));
        }

        // Create volume — handle UNIQUE constraint with user-friendly message
        match VolumeModel::create_in(&mut tx, title_id, label).await {
            Ok(vol) => {
                tx.commit().await?;
                Ok(VolumeAdd::Created(vol))
            }
            Err(AppError::BadRequest(msg)) if msg.starts_with("DUPLICATE_LABEL:") => {
                Self::resolve_duplicate_label(tx, label, loc).await
            }
            Err(e) => Err(e),
        }
    }

    /// The label already exists (#442). Live → `LabelLive` (story 19-3 D2:
    /// the caller answers it like its own #585 pre-check); in the Trash →
    /// refused (story 19-3 D9); vanished → "scan it again". Runs on the add's
    /// transaction `tx`, which still holds the title's share lock, and
    /// writes nothing.
    ///
    /// The lookup MUST include soft-deleted rows: `volumes.label` is globally
    /// UNIQUE and soft-deletion does not release it, so the previous
    /// `find_by_label` (which filters `deleted_at IS NULL`) returned `None` for
    /// exactly the collision it was trying to explain — and the message named
    /// the owner `"?"`.
    ///
    /// D9 (2026-11-25, supersedes the #442 reuse): a label held by a volume
    /// in the Trash is never reused — reusing it discarded the trashed
    /// copy's data and broke the Trash's 30-day recovery. The label is free
    /// again once the row is gone (permanent delete, auto-purge) or live
    /// again after a restore.
    async fn resolve_duplicate_label(
        mut tx: sqlx::Transaction<'_, sqlx::MySql>,
        label: &str,
        loc: &str,
    ) -> Result<VolumeAdd, AppError> {
        let Some((existing, is_deleted)) =
            VolumeModel::find_by_label_any_state_in(&mut tx, label).await?
        else {
            // The row vanished between the failed INSERT and this lookup — a
            // concurrent hard delete. Retrying is the honest advice.
            tx.rollback().await?;
            return Err(AppError::Conflict(
                rust_i18n::t!("feedback.volume_label_race", locale = loc, label = label).to_string(),
            ));
        };

        if !is_deleted {
            // Live volume — the label is taken (by this title or another).
            tx.rollback().await?;
            return Ok(VolumeAdd::LabelLive(existing));
        }

        // In the Trash: name the trashed volume's title, itself possibly in
        // the Trash.
        let title = crate::models::title::TitleModel::name_any_state_in(&mut tx, existing.title_id)
            .await?
            .unwrap_or_else(|| "?".to_string());
        tx.rollback().await?;
        tracing::info!(label = %label, volume_id = existing.id, "V-code held by a volume in the Trash; refused");
        Err(AppError::BadRequest(
            rust_i18n::t!("feedback.volume_label_in_trash", locale = loc, label = label, title = &title)
                .to_string(),
        ))
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

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_vcode_accepts_either_case_of_v_and_four_ascii_digits() {
        for (input, expected) in [("V0042", "V0042"), ("v0042", "V0042"), ("v9999", "V9999"), ("v0000", "V0000")] {
            assert_eq!(VolumeService::normalize_vcode(input).as_deref(), Some(expected), "{input}");
        }
    }

    #[test]
    fn normalize_vcode_leaves_every_other_shape_alone() {
        for input in [
            "", "V", "v", "V123", "v12345", "V00001", "VABCD", "V12A4", "V,/+", "11338",
            "L0042", "l0042", " V0042", "V0042 ", "W0042", "V٠١٢٣", "V００４２", "\u{200b}V0042", "vv0042",
        ] {
            assert_eq!(VolumeService::normalize_vcode(input), None, "{input:?}");
        }
    }

    #[test]
    fn label_from_input_trims_and_canonicalises_v_codes_only() {
        assert_eq!(VolumeService::label_from_input("  v0042 "), "V0042");
        assert_eq!(VolumeService::label_from_input("V0042"), "V0042");
        assert_eq!(VolumeService::label_from_input(" v12a4"), "v12a4");
        assert_eq!(VolumeService::label_from_input("l0042"), "l0042");
    }

    fn volume(id: u64) -> VolumeModel {
        VolumeModel {
            id,
            title_id: 1,
            label: "V0042".to_string(),
            condition_state_id: None,
            edition_comment: None,
            location_id: None,
            version: 1,
            purchase_price: None,
            purchase_currency: None,
            current_value: None,
            current_value_currency: None,
            current_value_updated_at: None,
            under_audit_since: None,
        }
    }

    #[test]
    fn a_live_label_is_no_creation() {
        assert!(VolumeAdd::LabelLive(volume(7)).into_created().is_none());
        let created = VolumeAdd::Created(volume(7)).into_created().expect("a creation");
        assert_eq!(created.id, 7);
    }

    #[test]
    fn the_trash_refusal_names_the_label_and_the_title_in_every_locale() {
        for loc in ["en", "fr", "de", "it"] {
            let msg = rust_i18n::t!("feedback.volume_label_in_trash", locale = loc, label = "V0042", title = "Tintin")
                .to_string();
            assert!(msg.contains("V0042") && msg.contains("Tintin"), "{loc}: {msg}");
            assert!(!msg.contains("feedback."), "{loc}: key resolved");
        }
    }

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
