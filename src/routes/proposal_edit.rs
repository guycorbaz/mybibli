//! Story 11-19 (FR139, UX-DR31) — editing a pending proposal's lines
//! before deciding it: the inline edit form's save, and line removal
//! through the UX-DR8 modal. Librarian-gated like every proposals route.
//! Logic lives in `services::proposal_edit`; handlers extract, call and
//! re-render the detail body.

use std::collections::HashMap;

use axum::Extension;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::proposal::ProposalStatus;
use crate::routes::proposals::{
    ProposalRejectModalTemplate, VersionQuery, body_response, load_detail, render,
};
use crate::services::proposal_edit::{self, EditError, EditOutcome};
use crate::services::proposal_review::ReviewError;
use crate::utils::{feedback_html, html_escape};

/// The version the page showed: missing or garbled is a conflict (the
/// change check could never fire otherwise, D-11-16-11).
fn page_version(raw: Option<&str>) -> Result<i32, EditError> {
    raw.and_then(|v| v.trim().parse::<i32>().ok())
        .ok_or(EditError::Changed)
}

fn user_of(session: &Session, loc: &str) -> Result<u64, AppError> {
    session
        .user_id
        .ok_or_else(|| ReviewError::UnknownDecider.into_app_error(loc))
}

/// `POST /proposals/{id}/items/{item_id}` — save the inline edit form
/// of one line (fields by line kind, see `proposal_edit`), then answer
/// the re-rendered detail body (new version) and an OOB entry. A refusal
/// lands in `#feedback-list`; nothing is saved.
pub async fn edit_line(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path((id, item_id)): Path<(u64, u64)>,
    axum::Form(form): axum::Form<HashMap<String, String>>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    let user_id = user_of(&session, loc)?;
    let outcome = async {
        let version = page_version(form.get("version").map(String::as_str))?;
        proposal_edit::edit_line(&state.pool, id, item_id, user_id, version, &form).await
    }
    .await
    .map_err(|e| e.into_app_error(loc))?;
    let entry = match outcome {
        EditOutcome::Saved => feedback_html(
            "success",
            &rust_i18n::t!("proposals.edit_saved", locale = loc, id = id),
            "",
        ),
        EditOutcome::Unchanged => feedback_html(
            "info",
            &rust_i18n::t!("proposals.edit_unchanged", locale = loc),
            "",
        ),
    };
    body_response(&state, id, loc, entry, false).await
}

/// `GET /proposals/{id}/items/{item_id}/remove-modal?version=N` — HTMX
/// only (405 otherwise). Names the line; refuses a line that cannot be
/// removed before opening.
pub async fn remove_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path((id, item_id)): Path<(u64, u64)>,
    Query(query): Query<VersionQuery>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role_with_return(Role::Librarian, &format!("/proposals/{id}"), loc)?;
    if !is_htmx {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    let detail = load_detail(&state, id, loc).await?;
    match detail.proposal.status {
        ProposalStatus::Pending => {}
        ProposalStatus::Stale => return Err(EditError::Stale.into_app_error(loc)),
        status => return Err(EditError::NotPending { status }.into_app_error(loc)),
    }
    let version = page_version(query.version.as_deref()).map_err(|e| e.into_app_error(loc))?;
    let line = detail
        .lines
        .iter()
        .find(|l| l.item_id == item_id)
        .ok_or_else(|| EditError::LineNotFound.into_app_error(loc))?;
    if !line.removable {
        let error = match proposal_edit::keep_of(detail.proposal.kind, line, detail.lines.len()) {
            Some(keep) => EditError::NotRemovable(keep),
            None => EditError::NotEditable,
        };
        return Err(error.into_app_error(loc));
    }
    let body = rust_i18n::t!(
        "proposals.remove_modal_body",
        locale = loc,
        line = proposal_edit::line_summary(line, loc).as_str()
    );
    let template = ProposalRejectModalTemplate {
        title: rust_i18n::t!("proposals.remove_modal_title", locale = loc, id = id).to_string(),
        body_html: format!("<p>{}</p>", html_escape(&body)),
        confirm_label: rust_i18n::t!("proposals.remove_button", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/proposals/{id}/items/{item_id}/remove"),
        csrf_token: session.csrf_token.clone(),
        version,
        inner_form_html: String::new(),
    };
    render(&template, "proposal_reject_modal")
}

#[derive(Deserialize)]
pub struct RemoveForm {
    #[serde(default)]
    pub version: Option<String>,
}

/// `POST /proposals/{id}/items/{item_id}/remove` — remove the line, then
/// answer the re-rendered body, an OOB entry and
/// `HX-Trigger-After-Swap: modal-close` (D-11-16-6). A refusal stays in
/// the modal.
pub async fn remove_line(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path((id, item_id)): Path<(u64, u64)>,
    axum::Form(form): axum::Form<RemoveForm>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    let user_id = user_of(&session, loc)?;
    async {
        let version = page_version(form.version.as_deref())?;
        proposal_edit::remove_line(&state.pool, id, item_id, user_id, version).await
    }
    .await
    .map_err(|e| e.into_app_error(loc))?;
    let entry = feedback_html(
        "success",
        &rust_i18n::t!("proposals.line_removed", locale = loc, id = id),
        "",
    );
    body_response(&state, id, loc, entry, true).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_version_requires_a_number() {
        assert_eq!(page_version(Some(" 3 ")).unwrap(), 3);
        assert!(matches!(page_version(None), Err(EditError::Changed)));
        assert!(matches!(page_version(Some("x")), Err(EditError::Changed)));
        assert!(matches!(
            page_version(Some("99999999999")),
            Err(EditError::Changed)
        ));
    }
}
