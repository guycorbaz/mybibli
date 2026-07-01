//! Polish-2 (#9) — `POST /catalog/undo`.
//!
//! Reverses the most-recent undoable scan action recorded in the session
//! (`sessions.data["last_undoable_action"]`) within a server-enforced
//! window. Split out of `catalog.rs` because that file is already over the
//! 2000-line Foundation-Rule-#12 budget.
//!
//! Design decision (recorded here + in the story Dev Agent Record): the
//! response refreshes only the `guide-strip` OOB region. It deliberately
//! does NOT refresh `context-banner` / `session-counter` — undoing a
//! location change alters neither the active title context nor any counter,
//! so re-rendering them would be a pointless DB round-trip and risk a
//! second, drifting copy of that markup.

use axum::extract::State;
use axum::response::{Html, IntoResponse};
use axum::Extension;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::location::LocationModel;
use crate::models::session::SessionModel;
use crate::models::volume::VolumeModel;
use crate::routes::catalog::guide_strip_html;
use crate::services::scan_undo::{SCAN_UNDO_WINDOW_SECS, UndoKind, undo_is_within_window};
use crate::utils::feedback_html;

pub async fn handle_undo(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let loc = locale.0;
    let pool = &state.pool;

    // Anonymous can't reach here (role gate above), but stay defensive.
    let Some(token) = session.token.as_deref() else {
        let msg = rust_i18n::t!("feedback.undo_nothing", locale = loc).to_string();
        return Ok(Html(feedback_html("info", &msg, "")).into_response());
    };

    let Some(action) = SessionModel::get_last_undoable_action(pool, token).await? else {
        let msg = rust_i18n::t!("feedback.undo_nothing", locale = loc).to_string();
        return Ok(Html(feedback_html("info", &msg, "")).into_response());
    };

    // Server-authoritative window — reject even if the client button lingered.
    let now = chrono::Utc::now().naive_utc();
    if !undo_is_within_window(action.at, now, SCAN_UNDO_WINDOW_SECS) {
        let _ = SessionModel::clear_last_undoable_action(pool, token).await;
        let msg = rust_i18n::t!("feedback.undo_too_late", locale = loc).to_string();
        return Ok(Html(feedback_html("info", &msg, "")).into_response());
    }

    let message = match action.kind {
        UndoKind::ShelveVolume => {
            // A well-formed ShelveVolume always carries volume_id; a missing one
            // means an externally corrupted blob — degrade gracefully to
            // "nothing to undo" (consistent with the corrupt-JSON path) rather
            // than surfacing a 500 for a benign case.
            let Some(volume_id) = action.volume_id else {
                let _ = SessionModel::clear_last_undoable_action(pool, token).await;
                let msg = rust_i18n::t!("feedback.undo_nothing", locale = loc).to_string();
                return Ok(Html(feedback_html("info", &msg, "")).into_response());
            };
            // P5: if the volume was (soft-)deleted within the window, there is
            // nothing to meaningfully reverse — report "nothing to undo"
            // instead of a false success on a 0-row UPDATE.
            if VolumeModel::find_by_id(pool, volume_id).await?.is_none() {
                let _ = SessionModel::clear_last_undoable_action(pool, token).await;
                let msg = rust_i18n::t!("feedback.undo_nothing", locale = loc).to_string();
                return Ok(Html(feedback_html("info", &msg, "")).into_response());
            }
            // P4: re-attach only to a prior location that still exists AND is
            // still assignable (not turned into an organizational container
            // meanwhile) — mirrors the forward shelve guard; otherwise detach.
            let target_loc = match action.prev_location_id {
                Some(prev) => match LocationModel::find_by_id(pool, prev).await? {
                    Some(l) if l.is_assignable() => Some(prev),
                    _ => None,
                },
                None => None,
            };
            VolumeModel::update_location(pool, volume_id, target_loc).await?;
            if let Some(label) = &action.prev_last_volume_label {
                let _ = SessionModel::set_last_volume_label(pool, token, label).await;
            }
            // D1: an L-code shelve also activated the batch location in the
            // same scan — undo restores the prior active location too.
            if action.revert_active_location {
                match action.prev_active_location {
                    Some(prev) => {
                        let _ = SessionModel::set_active_location(pool, token, prev).await;
                    }
                    None => {
                        let _ = SessionModel::clear_active_location(pool, token).await;
                    }
                }
            }
            rust_i18n::t!("feedback.undo_success_shelve", locale = loc).to_string()
        }
        UndoKind::ActivateLocation => {
            match action.prev_active_location {
                Some(prev) => {
                    let _ = SessionModel::set_active_location(pool, token, prev).await;
                }
                None => {
                    let _ = SessionModel::clear_active_location(pool, token).await;
                }
            }
            rust_i18n::t!("feedback.undo_success_activate_location", locale = loc).to_string()
        }
    };

    // Single-use: the reversal is itself NOT undoable (clear the log; do not
    // record a new action).
    SessionModel::clear_last_undoable_action(pool, token).await?;

    let guide = rust_i18n::t!("guide.undone", locale = loc).to_string();
    let resp = HtmxResponse {
        main: feedback_html("success", &message, ""),
        oob: vec![OobUpdate {
            swap_mode: Default::default(),
            target: "guide-strip".to_string(),
            content: guide_strip_html(&guide),
        }],
    };
    Ok(resp.into_response())
}
