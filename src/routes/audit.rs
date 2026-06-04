//! CR #237 — shelf-audit workflow routes.
//!
//! Five endpoints:
//!   - `POST /volume/{id}/mark-audit`         — flip a single volume
//!   - `POST /volume/{id}/clear-audit`        — clear a single volume
//!   - `POST /location/{id}/mark-audit`       — bulk-mark a location
//!   - `POST /audit/clear-all`                — clear every flagged volume
//!   - `GET  /audit`                          — list view (sorted location → V-code)
//!
//! Librarian+ on every state-changing route. `GET /audit` is also
//! Librarian+ — the audit workflow is owner-facing.

use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::locale::Locale;
use crate::services::volume_audit::VolumeAuditService;

pub async fn mark_volume_audit(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    VolumeAuditService::mark(&state.pool, id, session.user_id.unwrap_or(0)).await?;
    Ok(Redirect::to(&format!("/volume/{id}")).into_response())
}

pub async fn clear_volume_audit(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    VolumeAuditService::clear(&state.pool, id, session.user_id.unwrap_or(0)).await?;
    Ok(Redirect::to(&format!("/volume/{id}")).into_response())
}

pub async fn mark_location_audit(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(location_id): Path<u64>,
) -> Result<Response, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    VolumeAuditService::mark_for_location(
        &state.pool,
        location_id,
        session.user_id.unwrap_or(0),
    )
    .await?;
    Ok(Redirect::to(&format!("/location/{location_id}")).into_response())
}

pub async fn clear_all_audit(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
) -> Result<Response, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    VolumeAuditService::clear_all(&state.pool, session.user_id.unwrap_or(0)).await?;
    Ok(Redirect::to("/audit").into_response())
}

#[derive(Template)]
#[template(path = "pages/audit_list.html")]
struct AuditListTemplate {
    base: crate::utils::BaseContextFields,
    page_heading: String,
    intro: String,
    empty_state: String,
    col_volume: String,
    col_title: String,
    col_location: String,
    col_action: String,
    btn_check: String,
    btn_clear_all: String,
    items: Vec<AuditRowDisplay>,
}

pub struct AuditRowDisplay {
    pub volume_id: u64,
    pub volume_label: String,
    pub title_id: u64,
    pub title_name: String,
    pub location_path: String,
}

pub async fn audit_list_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    // Pull every flagged volume + its title + its location path,
    // sorted location → V-code so the user walks the shelf in order.
    // The location join is LEFT — an unshelved flagged volume still
    // surfaces, just with an empty path.
    //
    // Fix #321 — location_id is `BIGINT UNSIGNED NULL` but we project
    // it as `CAST(... AS SIGNED)`. Per CLAUDE.md MariaDB type gotcha
    // #2 the decoder MUST be `Option<i64>`, then convert to `u64` for
    // the downstream `LocationModel::get_path(pool, u64)` call. The
    // original `Option<u64>` triggered a sqlx 0.8 type-mismatch and a
    // 500 the moment a flagged-volume row actually existed in prod.
    // Latent since v1.6.0 (#237) — covered by the empty-state branch
    // in CI but never exercised on a non-empty result set.
    let rows = sqlx::query_as::<_, (u64, String, u64, String, Option<i64>)>(
        "SELECT v.id, v.label, t.id, t.title, CAST(v.location_id AS SIGNED) AS location_id \
         FROM volumes v \
         JOIN titles t ON t.id = v.title_id AND t.deleted_at IS NULL \
         WHERE v.under_audit_since IS NOT NULL AND v.deleted_at IS NULL \
         ORDER BY v.location_id, v.label",
    )
    .fetch_all(pool)
    .await?;

    let mut items: Vec<AuditRowDisplay> = Vec::with_capacity(rows.len());
    for (vol_id, vol_label, title_id, title_name, location_id) in rows {
        let location_path = match location_id {
            Some(lid) => crate::models::location::LocationModel::get_path(pool, lid as u64)
                .await
                .unwrap_or_default(),
            None => String::new(),
        };
        items.push(AuditRowDisplay {
            volume_id: vol_id,
            volume_label: vol_label,
            title_id,
            title_name,
            location_path,
        });
    }

    let base = crate::utils::base_context(&session, loc, "audit", &uri, state.session_timeout_secs());
    let template = AuditListTemplate {
        base,
        page_heading: rust_i18n::t!("audit.heading", locale = loc).to_string(),
        intro: rust_i18n::t!("audit.intro", locale = loc).to_string(),
        empty_state: rust_i18n::t!("audit.empty_state", locale = loc).to_string(),
        col_volume: rust_i18n::t!("audit.col_volume", locale = loc).to_string(),
        col_title: rust_i18n::t!("audit.col_title", locale = loc).to_string(),
        col_location: rust_i18n::t!("audit.col_location", locale = loc).to_string(),
        col_action: rust_i18n::t!("audit.col_action", locale = loc).to_string(),
        btn_check: rust_i18n::t!("volume.mark_checked", locale = loc).to_string(),
        btn_clear_all: rust_i18n::t!("audit.btn_clear_all", locale = loc).to_string(),
        items,
    };

    template
        .render()
        .map(|html| Html(html).into_response())
        .map_err(|e| AppError::Internal(format!("audit list render: {e}")))
}
