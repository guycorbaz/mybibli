//! Polish-2 (#9) — undo the last scan action on `/catalog`.
//!
//! A librarian can reverse the most-recent shelving / batch-location
//! activation within a short window. The action is logged server-side in
//! `sessions.data["last_undoable_action"]` (see
//! `SessionModel::{set,get,clear}_last_undoable_action`), so the window is
//! enforced by the server — the client button is UX only. Only the single
//! most-recent action is ever undoable (each new forward action overwrites
//! the log). Undo reverses *location state only*; it never deletes a
//! created title or volume.

use serde::{Deserialize, Serialize};

/// Undo window, in seconds. v1 freeze — NOT admin-configurable (mirrors
/// `RECENT_ACTIVITY_DAYS` in `home_indicators.rs`). The handler takes the
/// window as a parameter so a future extract to `AppSettings` is a focused
/// diff.
pub const SCAN_UNDO_WINDOW_SECS: i64 = 30;

/// Which forward action can be undone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UndoKind {
    /// A volume was shelved / attached to a location. Undo restores the
    /// volume's previous `location_id` (which may be `None` = was unshelved
    /// or freshly created — undo then detaches it, never deletes it).
    #[serde(rename = "shelve_volume")]
    ShelveVolume,
    /// A batch storage-location was activated in the session. Undo restores
    /// the previous active location (or clears it if there was none).
    #[serde(rename = "activate_location")]
    ActivateLocation,
}

/// The most-recent undoable scan action, persisted per session in
/// `sessions.data["last_undoable_action"]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoableAction {
    pub kind: UndoKind,
    /// Volume that was shelved (set for `ShelveVolume`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_id: Option<u64>,
    /// Volume's `location_id` BEFORE the shelve (`None` = was unshelved).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_location_id: Option<u64>,
    /// Session active-location BEFORE activation (for `ActivateLocation`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_active_location: Option<u64>,
    /// `last_volume_label` cleared by the shelve, restored on undo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_last_volume_label: Option<String>,
    /// When the action happened (UTC naive), for the window check.
    pub at: chrono::NaiveDateTime,
}

impl UndoableAction {
    pub fn shelve_volume(
        volume_id: u64,
        prev_location_id: Option<u64>,
        prev_last_volume_label: Option<String>,
        at: chrono::NaiveDateTime,
    ) -> Self {
        Self {
            kind: UndoKind::ShelveVolume,
            volume_id: Some(volume_id),
            prev_location_id,
            prev_active_location: None,
            prev_last_volume_label,
            at,
        }
    }

    pub fn activate_location(prev_active_location: Option<u64>, at: chrono::NaiveDateTime) -> Self {
        Self {
            kind: UndoKind::ActivateLocation,
            volume_id: None,
            prev_location_id: None,
            prev_active_location,
            prev_last_volume_label: None,
            at,
        }
    }
}

/// Pure, testable window check: is `at` within `window_secs` of `now`?
///
/// Inclusive on the boundary (`elapsed <= window`). A negative elapsed
/// (clock skew, `at` slightly in the future) is treated as within-window.
pub fn undo_is_within_window(
    at: chrono::NaiveDateTime,
    now: chrono::NaiveDateTime,
    window_secs: i64,
) -> bool {
    (now - at).num_seconds() <= window_secs
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, NaiveDate};

    fn base() -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 7, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
    }

    #[test]
    fn window_constant_is_thirty_seconds() {
        // v1 spec freeze — locks the value so a silent change trips the test.
        assert_eq!(SCAN_UNDO_WINDOW_SECS, 30);
    }

    #[test]
    fn within_window_at_zero_and_boundary() {
        let at = base();
        assert!(undo_is_within_window(at, at, 30)); // 0s elapsed
        assert!(undo_is_within_window(
            at,
            at + Duration::seconds(29),
            30
        ));
        // Inclusive boundary: exactly 30s still counts as within.
        assert!(undo_is_within_window(
            at,
            at + Duration::seconds(30),
            30
        ));
    }

    #[test]
    fn outside_window_past_boundary() {
        let at = base();
        assert!(!undo_is_within_window(
            at,
            at + Duration::seconds(31),
            30
        ));
        assert!(!undo_is_within_window(
            at,
            at + Duration::seconds(120),
            30
        ));
    }

    #[test]
    fn clock_skew_future_at_is_within_window() {
        let at = base();
        // `at` is 5s in the future relative to `now` → negative elapsed.
        assert!(undo_is_within_window(at, at - Duration::seconds(5), 30));
    }

    #[test]
    fn shelve_action_roundtrips_through_json() {
        let action =
            UndoableAction::shelve_volume(42, Some(7), Some("V0042".to_string()), base());
        let json = serde_json::to_string(&action).unwrap();
        // kind serializes to the stable snake string.
        assert!(json.contains("\"shelve_volume\""));
        let back: UndoableAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, back);
    }

    #[test]
    fn activate_action_roundtrips_and_omits_none_fields() {
        let action = UndoableAction::activate_location(None, base());
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("\"activate_location\""));
        // `skip_serializing_if` keeps None fields out of the blob.
        assert!(!json.contains("volume_id"));
        assert!(!json.contains("prev_active_location"));
        let back: UndoableAction = serde_json::from_str(&json).unwrap();
        assert_eq!(action, back);
    }
}
