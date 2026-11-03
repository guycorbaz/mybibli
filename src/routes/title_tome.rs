//! Story 13-10 (#579, AC5) — edit a title's position in a series from the
//! title page. Kept out of `titles.rs` (Foundation Rule #12, #597).

use axum::Form;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderName, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;

#[derive(Debug, Deserialize)]
pub struct PositionForm {
    pub version: i32,
    pub position_number: i32,
}

/// `POST /title/{id}/series/{assignment_id}/position` — Librarian+.
/// HTMX: `HX-Redirect` back to the title page on success; errors land in
/// `#feedback-list` (the `AppError` retarget). Without HTMX: 303.
pub async fn change_position(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path((title_id, assignment_id)): Path<(u64, u64)>,
    Form(form): Form<PositionForm>,
) -> Result<Response, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    crate::services::tome::change_position(
        &state.pool,
        title_id,
        assignment_id,
        form.version,
        form.position_number,
        locale.0,
    )
    .await?;
    let target = format!("/title/{title_id}");
    if is_htmx {
        Ok((
            StatusCode::OK,
            [(HeaderName::from_static("hx-redirect"), target)],
        )
            .into_response())
    } else {
        Ok(Redirect::to(&target).into_response())
    }
}
