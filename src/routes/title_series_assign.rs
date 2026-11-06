//! Assigning a title to a series, and removing it, from the title page.
//!
//! Split out of `routes/titles.rs` by story 16-1 (#597, Foundation Rule #12).

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::{Extension, Form};

use axum::response::Redirect;
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::locale::Locale;
use crate::services::series::SeriesService;


#[derive(Deserialize)]
pub struct AssignToSeriesForm {
    pub series_id: u64,
    pub position_number: i32,
    #[serde(
        default,
        deserialize_with = "crate::routes::series::deserialize_optional_i32"
    )]
    pub end_position: Option<i32>,
    #[serde(default)]
    pub omnibus: Option<String>,
}

pub async fn assign_to_series(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(title_id): Path<u64>,
    Form(form): Form<AssignToSeriesForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;

    let is_omnibus = form.omnibus.as_deref() == Some("on");
    if is_omnibus {
        let end = form.end_position.unwrap_or(form.position_number);
        if end == form.position_number {
            // Single position, treat as normal assignment
            SeriesService::assign_title(pool, title_id, form.series_id, form.position_number)
                .await?;
        } else {
            SeriesService::assign_omnibus(
                pool,
                title_id,
                form.series_id,
                form.position_number,
                end,
            )
            .await?;
        }
    } else {
        SeriesService::assign_title(pool, title_id, form.series_id, form.position_number).await?;
    }

    Ok(Redirect::to(&format!("/title/{title_id}")))
}

#[derive(Deserialize)]
pub struct UnassignFromSeriesForm {
    pub series_id: u64,
}

pub async fn unassign_omnibus_from_series(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(title_id): Path<u64>,
    Form(form): Form<UnassignFromSeriesForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;

    SeriesService::unassign_all_from_series(pool, title_id, form.series_id).await?;

    Ok(Redirect::to(&format!("/title/{title_id}")))
}

pub async fn unassign_from_series(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path((title_id, assignment_id)): Path<(u64, u64)>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;

    SeriesService::unassign_title(pool, assignment_id, title_id).await?;

    Ok(Redirect::to(&format!("/title/{title_id}")))
}
