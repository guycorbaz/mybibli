//! Story 13-8 (#580) — move a volume to another title.
//!
//! A label scanned under the wrong title (a publisher EAN shared by several
//! albums, a box-set component) used to be fixable only by deleting and
//! re-creating the volume, losing its history. The move changes
//! `volumes.title_id` alone, under optimistic locking: the label, location,
//! condition, values, entity labels, audit flag and loans — an active loan
//! included (decision of 2026-11-03: the loan follows the volume) — stay
//! with the volume. Series membership belongs to titles, so no expected
//! tome is settled or unsettled by a move; per-title volume counts are
//! computed live.

use crate::db::DbPool;
use crate::error::AppError;
use crate::models::volume_move;
use crate::services::volume::VolumeService;

/// Why a move is refused before anything is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveRefusal {
    /// No target title was picked.
    NoTarget,
    /// The target is the volume's current title.
    SameTitle,
    /// The target does not exist or is deleted.
    TargetGone,
}

impl MoveRefusal {
    /// The localized, Rule #9 message shown in the modal.
    pub fn message(self, loc: &str) -> String {
        let key = match self {
            MoveRefusal::NoTarget => "error.volume_move_no_target",
            MoveRefusal::SameTitle => "error.volume_move_same_title",
            MoveRefusal::TargetGone => "error.volume_move_target_gone",
        };
        rust_i18n::t!(key, locale = loc).to_string()
    }

    /// [`Self::message`] as the error the move returns: a missing target
    /// is `NotFound`, the others `BadRequest`.
    pub fn into_app_error(self, loc: &str) -> AppError {
        match self {
            MoveRefusal::NoTarget | MoveRefusal::SameTitle => {
                AppError::BadRequest(self.message(loc))
            }
            MoveRefusal::TargetGone => AppError::NotFound(self.message(loc)),
        }
    }
}

/// The refusal for moving a volume of `current_title_id` to `target`,
/// `None` when the move may go ahead. `target_live` says whether the
/// target title exists and is not deleted (ignored without a target).
pub fn move_refusal(
    current_title_id: u64,
    target: Option<u64>,
    target_live: bool,
) -> Option<MoveRefusal> {
    match target {
        None => Some(MoveRefusal::NoTarget),
        Some(t) if t == current_title_id => Some(MoveRefusal::SameTitle),
        Some(_) if !target_live => Some(MoveRefusal::TargetGone),
        Some(_) => None,
    }
}

/// A move that went through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeMoved {
    pub volume_id: u64,
    pub label: String,
    pub from_title_id: u64,
    pub to_title_id: u64,
}

impl VolumeService {
    /// Move live volume `volume_id`, read at `version`, to the live title
    /// `target`. One transaction: the volume row is locked, then the
    /// target title is share-locked. The title delete
    /// (`TitleService::delete_without_volumes`) takes that row `FOR
    /// UPDATE` before counting volumes, so the two serialise: a delete
    /// already holding the lock makes the move wait, then refuse with
    /// `TargetGone`; a move holding its share lock makes the delete wait,
    /// then refuse because the title has a volume. Other writers that
    /// soft-delete titles without that lock (Trash, auto-purge of rows
    /// already deleted) are out of this guarantee. The UPDATE is
    /// optimistic (`WHERE version = ?`). Refusals — no target,
    /// same title, deleted/unknown target, deleted volume, stale version —
    /// write nothing and carry a localized message.
    pub async fn move_to_title(
        pool: &DbPool,
        volume_id: u64,
        version: i32,
        target: Option<u64>,
        user_id: Option<u64>,
        loc: &str,
    ) -> Result<VolumeMoved, AppError> {
        let mut tx = pool.begin().await?;

        let Some(volume) = volume_move::lock_volume_in(&mut tx, volume_id).await? else {
            return Err(AppError::NotFound(
                rust_i18n::t!("error.volume_move_volume_gone", locale = loc).to_string(),
            ));
        };
        if volume.version != version {
            return Err(stale(loc));
        }

        let target_live = match target {
            Some(t) if t != volume.title_id => volume_move::title_is_live_in(&mut tx, t).await?,
            _ => false,
        };
        if let Some(refusal) = move_refusal(volume.title_id, target, target_live) {
            return Err(refusal.into_app_error(loc));
        }
        // `move_refusal` returned None, so the target is present.
        let to_title_id = target.ok_or_else(|| {
            AppError::Internal("volume move: target vanished after the check".to_string())
        })?;

        let changed = volume_move::set_title_in(&mut tx, volume_id, version, to_title_id).await?;
        if changed == 0 {
            // Unreachable under the row lock, kept as the optimistic guard.
            return Err(stale(loc));
        }
        tx.commit().await?;

        tracing::info!(
            volume_id,
            label = %volume.label,
            from_title_id = volume.title_id,
            to_title_id,
            user_id = ?user_id,
            "Volume moved to another title"
        );

        Ok(VolumeMoved {
            volume_id,
            label: volume.label,
            from_title_id: volume.title_id,
            to_title_id,
        })
    }
}

fn stale(loc: &str) -> AppError {
    AppError::Conflict(rust_i18n::t!("error.volume_move_stale", locale = loc).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_target_is_refused() {
        assert_eq!(move_refusal(7, None, false), Some(MoveRefusal::NoTarget));
        assert_eq!(move_refusal(7, None, true), Some(MoveRefusal::NoTarget));
    }

    #[test]
    fn the_current_title_is_refused_even_when_live() {
        assert_eq!(move_refusal(7, Some(7), true), Some(MoveRefusal::SameTitle));
        assert_eq!(
            move_refusal(7, Some(7), false),
            Some(MoveRefusal::SameTitle)
        );
    }

    #[test]
    fn a_deleted_or_unknown_target_is_refused() {
        assert_eq!(
            move_refusal(7, Some(8), false),
            Some(MoveRefusal::TargetGone)
        );
    }

    #[test]
    fn a_live_other_title_is_accepted() {
        assert_eq!(move_refusal(7, Some(8), true), None);
    }

    #[test]
    fn refusals_carry_localized_messages_of_the_right_kind() {
        for loc in ["en", "fr", "de", "it"] {
            match MoveRefusal::NoTarget.into_app_error(loc) {
                AppError::BadRequest(m) => assert!(!m.contains("volume_move_"), "{loc}: {m}"),
                other => panic!("{other:?}"),
            }
            match MoveRefusal::SameTitle.into_app_error(loc) {
                AppError::BadRequest(m) => assert!(!m.contains("volume_move_"), "{loc}: {m}"),
                other => panic!("{other:?}"),
            }
            match MoveRefusal::TargetGone.into_app_error(loc) {
                AppError::NotFound(m) => assert!(!m.contains("volume_move_"), "{loc}: {m}"),
                other => panic!("{other:?}"),
            }
            match stale(loc) {
                AppError::Conflict(m) => assert!(!m.contains("volume_move_"), "{loc}: {m}"),
                other => panic!("{other:?}"),
            }
        }
    }
}
