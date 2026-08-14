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

    /// Highest L-code number the format allows: `L` + 4 digits, `L0000` rejected.
    const MAX_LCODE: i64 = 9999;

    /// Propose the **lowest unused** L-code, counting every row — soft-deleted
    /// included (#457).
    ///
    /// Two separate defects are fixed here, and it is worth keeping them apart.
    ///
    /// **The collision.** `storage_locations.label` carries a global `UNIQUE`
    /// index that soft-deletion does not release, while the proposal used to be
    /// computed over live rows only. Deleting a location therefore handed the
    /// next creation a code the deleted row still held, and the insert died on
    /// the index with a raw database error. The fix is to consider every row.
    ///
    /// That removes an asymmetry with [`LocationModel::highest_label_any`] which
    /// the code described as intentional. Re-reading it, only half was:
    /// `highest_label_any` ignores `deleted_at` because a printed sticker on a
    /// shelf outlives the row's trash state — sound. Nothing in that reasoning
    /// justified the *proposal* ignoring deleted rows.
    ///
    /// **The cliff.** The obvious repair — `MAX(all rows) + 1` — is wrong for a
    /// second reason, found by running the E2E suite against it: the space is
    /// only 9 999 codes, so once anything reaches the top the proposal returns
    /// "exhausted" forever, even with thousands of codes free lower down. A
    /// single high code, live or deleted, would brick location creation.
    /// Scanning for the lowest free number instead packs the space densely and
    /// has no cliff.
    ///
    /// A deleted location keeps its code reserved — that is deliberate, and it
    /// is what the "restore it, or purge it to free the code" message promises:
    /// purging the row from the Trash genuinely returns the code to the pool,
    /// because this scan then sees the gap.
    pub async fn get_next_available_lcode(pool: &DbPool) -> Result<String, AppError> {
        // Well-formed codes only. A row whose label somehow escaped
        // `validate_lcode` must not shift the numbering.
        let used: Vec<i64> = sqlx::query_scalar(
            "SELECT CAST(CAST(SUBSTRING(label, 2) AS UNSIGNED) AS SIGNED) \
             FROM storage_locations \
             WHERE label REGEXP '^L[0-9]{4}$' \
             ORDER BY 1",
        )
        .fetch_all(pool)
        .await?;

        // The list is sorted ascending; walk it looking for the first gap.
        // Duplicates cannot occur (UNIQUE index) but skipping them costs
        // nothing and keeps the walk correct if that ever changes.
        let mut candidate: i64 = 1;
        for n in used {
            if n < candidate {
                continue;
            }
            if n > candidate {
                break; // found a gap
            }
            candidate += 1;
        }

        if candidate > Self::MAX_LCODE {
            return Err(AppError::BadRequest(
                rust_i18n::t!("location.lcode_exhausted").to_string(),
            ));
        }
        Ok(format!("L{candidate:04}"))
    }

    /// Create a new location in the hierarchy.
    pub async fn create_location(
        pool: &DbPool,
        name: &str,
        node_type: &str,
        parent_id: Option<u64>,
        label: &str,
        is_organizational: bool,
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

        // Check L-code uniqueness. #457 — the lookup MUST include soft-deleted
        // rows: the UNIQUE index does not release on soft delete, so the
        // live-only `find_by_label` returned `None` for exactly the collision it
        // was meant to explain, and the insert then failed with a raw database
        // error. The two cases need different advice, so they get different
        // messages rather than one vague "already in use".
        if let Some((_, is_deleted)) = LocationModel::find_by_label_any_state(pool, label).await? {
            let key = if is_deleted {
                // Recoverable: the operator can restore the location from the
                // Trash panel, or purge it there to free the code for good.
                "location.lcode_in_trash"
            } else {
                "location.lcode_duplicate"
            };
            return Err(AppError::BadRequest(rust_i18n::t!(key).to_string()));
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

        let location =
            LocationModel::create(pool, name, node_type, parent_id, label, is_organizational)
                .await?;
        tracing::info!(id = location.id, name = %name, label = %label, "Location created");
        Ok(location)
    }

    /// Update a location with optimistic locking and cycle detection.
    ///
    /// CR #280 — if the form flips `is_organizational` from FALSE to
    /// TRUE on a row that still has volumes attached, refuse with a
    /// 400 + the volume count. Silent re-parenting of the volumes is
    /// explicitly rejected (the user must re-shelve them first; that
    /// also gives them a chance to think about WHERE the volumes
    /// should land instead of getting auto-orphaned).
    #[allow(clippy::too_many_arguments)]
    pub async fn update_location(
        pool: &DbPool,
        id: u64,
        version: i32,
        name: &str,
        node_type: &str,
        parent_id: Option<u64>,
        is_organizational: bool,
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

        // CR #280 — block the flip-to-organizational on a row that
        // still holds volumes. Reads the live count; a transient race
        // (volume just got assigned between this check and the UPDATE)
        // is acceptable because the worst case is the form re-render
        // showing the new count.
        if is_organizational {
            let attached = LocationModel::count_assigned_volumes(pool, id).await?;
            if attached > 0 {
                return Err(AppError::BadRequest(
                    rust_i18n::t!(
                        "location.organizational_blocked_has_volumes",
                        count = attached
                    )
                    .to_string(),
                ));
            }
        }

        let location = LocationModel::update_with_locking(
            pool,
            id,
            version,
            name,
            node_type,
            parent_id,
            is_organizational,
        )
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

    // ─── #457 — L-code collision after soft-delete ─────────────────

    /// Helper: insert a location directly, optionally soft-deleted.
    #[cfg(test)]
    async fn seed_location(pool: &DbPool, name: &str, label: &str, deleted: bool) -> u64 {
        let nt: String = sqlx::query_scalar(
            "SELECT name FROM location_node_types WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let r = sqlx::query(
            "INSERT INTO storage_locations (name, node_type, label) VALUES (?, ?, ?)",
        )
        .bind(name)
        .bind(&nt)
        .bind(label)
        .execute(pool)
        .await
        .unwrap();
        let id = r.last_insert_id();
        if deleted {
            sqlx::query("UPDATE storage_locations SET deleted_at = NOW() WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await
                .unwrap();
        }
        id
    }

    /// The heart of #457: the proposal must not hand back a code that a
    /// soft-deleted row still holds, because the UNIQUE index does not release
    /// on soft delete and the insert would then die.
    #[sqlx::test(migrations = "./migrations")]
    async fn next_lcode_skips_codes_held_by_soft_deleted_rows(pool: sqlx::MySqlPool) {
        seed_location(&pool, "Shelf A", "L0001", true).await;

        let next = LocationService::get_next_available_lcode(&pool).await.unwrap();
        assert_ne!(
            next, "L0001",
            "the proposal must not return a code a deleted row still holds"
        );
        assert_eq!(next, "L0002");
    }

    /// The pre-#457 behaviour, stated as a test so the regression is explicit:
    /// with a live-rows-only MAX, deleting the highest code walks the proposal
    /// backwards onto it.
    #[sqlx::test(migrations = "./migrations")]
    async fn next_lcode_does_not_walk_backwards_after_a_delete(pool: sqlx::MySqlPool) {
        seed_location(&pool, "Shelf A", "L0001", false).await;
        seed_location(&pool, "Shelf B", "L0002", false).await;
        assert_eq!(
            LocationService::get_next_available_lcode(&pool).await.unwrap(),
            "L0003"
        );

        // Delete the highest one. The proposal must stay at L0003.
        sqlx::query("UPDATE storage_locations SET deleted_at = NOW() WHERE label = 'L0002'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            LocationService::get_next_available_lcode(&pool).await.unwrap(),
            "L0003",
            "a delete must not make the proposal reuse the freed code"
        );
    }

    /// #457 — the cliff that `MAX + 1` would have introduced, and which the E2E
    /// suite actually hit: one code near the top must not brick creation while
    /// thousands are free below it.
    #[sqlx::test(migrations = "./migrations")]
    async fn a_high_code_does_not_exhaust_the_space(pool: sqlx::MySqlPool) {
        seed_location(&pool, "Top", "L9998", false).await;

        let next = LocationService::get_next_available_lcode(&pool).await.unwrap();
        assert_eq!(
            next, "L0001",
            "the proposal must fill from the bottom, not stall at the top"
        );
    }

    /// Gaps are filled densely, and a soft-deleted row keeps its code reserved
    /// — which is precisely what the "purge it to free the code" advice means.
    #[sqlx::test(migrations = "./migrations")]
    async fn next_lcode_fills_the_lowest_gap_and_respects_reserved_codes(
        pool: sqlx::MySqlPool,
    ) {
        seed_location(&pool, "One", "L0001", false).await;
        seed_location(&pool, "Two", "L0002", true).await; // in the Trash
        seed_location(&pool, "Four", "L0004", false).await;

        // L0003 is the lowest genuinely free code: L0002 is still held by the
        // trashed row and must not be proposed.
        assert_eq!(
            LocationService::get_next_available_lcode(&pool).await.unwrap(),
            "L0003"
        );

        // Purging the trashed row returns its code to the pool.
        sqlx::query("DELETE FROM storage_locations WHERE label = 'L0002'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            LocationService::get_next_available_lcode(&pool).await.unwrap(),
            "L0002",
            "a purge must genuinely free the code, as the error message promises"
        );
    }

    /// Typing a code held by a soft-deleted row must explain the situation
    /// rather than fail on the database index.
    #[sqlx::test(migrations = "./migrations")]
    async fn create_with_a_trashed_lcode_explains_instead_of_failing_on_the_index(
        pool: sqlx::MySqlPool,
    ) {
        seed_location(&pool, "Old shelf", "L0007", true).await;
        let nt: String = sqlx::query_scalar(
            "SELECT name FROM location_node_types WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let err = LocationService::create_location(&pool, "New shelf", &nt, None, "L0007", false)
            .await
            .expect_err("creating with a trashed L-code must fail");

        match err {
            AppError::BadRequest(msg) => {
                assert!(
                    msg.to_lowercase().contains("trash")
                        || msg.to_lowercase().contains("corbeille"),
                    "the message must name the Trash so the operator knows the way out, got: {msg}"
                );
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    /// A live collision keeps its own, different message — the two situations
    /// need different advice.
    #[sqlx::test(migrations = "./migrations")]
    async fn create_with_a_live_lcode_keeps_the_plain_duplicate_message(pool: sqlx::MySqlPool) {
        seed_location(&pool, "Live shelf", "L0008", false).await;
        let nt: String = sqlx::query_scalar(
            "SELECT name FROM location_node_types WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let err = LocationService::create_location(&pool, "Another", &nt, None, "L0008", false)
            .await
            .expect_err("creating with a live L-code must fail");

        match err {
            AppError::BadRequest(msg) => assert!(
                !msg.to_lowercase().contains("trash")
                    && !msg.to_lowercase().contains("corbeille"),
                "a live collision must NOT mention the Trash, got: {msg}"
            ),
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    /// The lookup must see both states and report which one it found.
    #[sqlx::test(migrations = "./migrations")]
    async fn find_by_label_any_state_reports_the_deleted_flag(pool: sqlx::MySqlPool) {
        seed_location(&pool, "Live", "L0011", false).await;
        seed_location(&pool, "Trashed", "L0012", true).await;

        let (live, live_deleted) = LocationModel::find_by_label_any_state(&pool, "L0011")
            .await
            .unwrap()
            .expect("live row must be found");
        assert_eq!(live.name, "Live");
        assert!(!live_deleted);

        let (dead, dead_deleted) = LocationModel::find_by_label_any_state(&pool, "L0012")
            .await
            .unwrap()
            .expect("soft-deleted row must be found too");
        assert_eq!(dead.name, "Trashed");
        assert!(dead_deleted);

        assert!(
            LocationModel::find_by_label_any_state(&pool, "L9998")
                .await
                .unwrap()
                .is_none()
        );
        // The live-only lookup must still NOT see the deleted row — the two
        // helpers have different contracts and both are relied upon.
        assert!(LocationModel::find_by_label(&pool, "L0012").await.unwrap().is_none());
    }
}
