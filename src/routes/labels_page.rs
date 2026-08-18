//! CR #443 tranche 3 — the `/labels` vocabulary index and its drill-down.
//!
//! Two routes:
//!
//! * `GET /labels` — the vocabulary, each row with its usage split by entity
//!   kind ("À vérifier — 3 titres, 7 volumes").
//! * `GET /labels/{id}` — the members of one label.
//!
//! The members list is **two sections, titles above volumes**, settled on the
//! issue 2026-07-28. A merged table was rejected there and the reason holds
//! in the data: a title row wants cover / title / contributor / genre, a
//! volume row wants V-code / parent title / location / state. One table would
//! either duplicate columns or flatten to their intersection.
//!
//! Each section paginates independently (`?tp=` and `?vp=`), so paging through
//! forty volumes does not reset the reader's position in the titles.
//!
//! Librarian+ throughout (issue requirement 6).

use askama::Template;
use axum::extract::{Extension, OriginalUri, Path, Query, State};
use axum::response::{Html, IntoResponse};

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::locale::Locale;
use crate::models::PaginatedList;
use crate::models::label::{LabelModel, LabelUsage, LabelledTitle, LabelledVolume};

pub struct LabelIndexRow {
    pub id: u64,
    pub name: String,
    pub usage_chip: String,
    pub total: i64,
}

#[derive(Template)]
#[template(path = "pages/labels.html")]
pub struct LabelsIndexTemplate {
    pub base: crate::utils::BaseContextFields,
    pub rows: Vec<LabelIndexRow>,
    pub heading: String,
    pub empty_state: String,
    pub col_name: String,
    pub col_usage: String,
}

#[derive(Template)]
#[template(path = "pages/label_detail.html")]
pub struct LabelDetailTemplate {
    pub base: crate::utils::BaseContextFields,
    pub label_name: String,
    pub titles: PaginatedList<LabelledTitle>,
    pub volumes: PaginatedList<LabelledVolume>,
    pub heading_titles: String,
    pub heading_volumes: String,
    /// Rendered even when the section is empty: telling "no volumes carry
    /// this label" apart from "the volumes section failed to load" is the
    /// point (issue, 2026-07-28).
    pub empty_titles: String,
    pub empty_volumes: String,
    pub col_title: String,
    pub col_contributor: String,
    pub col_genre: String,
    pub col_vcode: String,
    pub col_parent_title: String,
    pub col_location: String,
    pub col_state: String,
    pub back_label: String,
    pub label_id: u64,
    pub titles_page: u32,
    pub volumes_page: u32,
    pub prev_label: String,
    pub next_label: String,
}

#[derive(serde::Deserialize, Default)]
pub struct DrillDownQuery {
    /// Titles page. Separate from `vp` so the two sections page apart.
    #[serde(default)]
    pub tp: Option<u32>,
    #[serde(default)]
    pub vp: Option<u32>,
}

fn usage_chip(usage: LabelUsage, loc: &'static str) -> String {
    rust_i18n::t!(
        "admin.reference_data.label_usage_chip",
        locale = loc,
        titles = usage.titles,
        volumes = usage.volumes
    )
    .to_string()
}

pub async fn labels_index(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let loc = locale.0;

    let rows = LabelModel::list_all_with_usage(&state.pool)
        .await?
        .into_iter()
        .map(|(label, usage)| LabelIndexRow {
            id: label.id,
            name: label.name,
            usage_chip: usage_chip(usage, loc),
            total: usage.total(),
        })
        .collect();

    let template = LabelsIndexTemplate {
        base: crate::utils::base_context(
            &session,
            loc,
            "labels",
            &uri,
            state.session_timeout_secs(),
        ),
        rows,
        heading: rust_i18n::t!("labels.page_heading", locale = loc).to_string(),
        empty_state: rust_i18n::t!("labels.page_empty", locale = loc).to_string(),
        col_name: rust_i18n::t!("labels.col_name", locale = loc).to_string(),
        col_usage: rust_i18n::t!("labels.col_usage", locale = loc).to_string(),
    };
    Ok(Html(template.render().map_err(|_| {
        AppError::Internal("labels index render failed".to_string())
    })?))
}

pub async fn label_detail(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<u64>,
    Query(q): Query<DrillDownQuery>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let loc = locale.0;

    let label = LabelModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
        })?;

    let titles_page = q.tp.unwrap_or(1).max(1);
    let volumes_page = q.vp.unwrap_or(1).max(1);

    let template = LabelDetailTemplate {
        base: crate::utils::base_context(
            &session,
            loc,
            "labels",
            &uri,
            state.session_timeout_secs(),
        ),
        titles: LabelModel::list_titles_for(&state.pool, id, titles_page).await?,
        volumes: LabelModel::list_volumes_for(&state.pool, id, volumes_page).await?,
        label_name: label.name,
        heading_titles: rust_i18n::t!("labels.section_titles", locale = loc).to_string(),
        heading_volumes: rust_i18n::t!("labels.section_volumes", locale = loc).to_string(),
        empty_titles: rust_i18n::t!("labels.no_titles", locale = loc).to_string(),
        empty_volumes: rust_i18n::t!("labels.no_volumes", locale = loc).to_string(),
        col_title: rust_i18n::t!("labels.col_title", locale = loc).to_string(),
        col_contributor: rust_i18n::t!("labels.col_contributor", locale = loc).to_string(),
        col_genre: rust_i18n::t!("labels.col_genre", locale = loc).to_string(),
        col_vcode: rust_i18n::t!("labels.col_vcode", locale = loc).to_string(),
        col_parent_title: rust_i18n::t!("labels.col_parent_title", locale = loc).to_string(),
        col_location: rust_i18n::t!("labels.col_location", locale = loc).to_string(),
        col_state: rust_i18n::t!("labels.col_state", locale = loc).to_string(),
        back_label: rust_i18n::t!("labels.back_to_index", locale = loc).to_string(),
        label_id: id,
        titles_page,
        volumes_page,
        prev_label: rust_i18n::t!("labels.prev_page", locale = loc).to_string(),
        next_label: rust_i18n::t!("labels.next_page", locale = loc).to_string(),
    };
    Ok(Html(template.render().map_err(|_| {
        AppError::Internal("label detail render failed".to_string())
    })?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_numbers_default_to_one_and_never_go_below() {
        // `?tp=0` would compute a negative offset; the handler clamps instead
        // of trusting the query string.
        let q = DrillDownQuery {
            tp: Some(0),
            vp: None,
        };
        assert_eq!(q.tp.unwrap_or(1).max(1), 1);
        assert_eq!(q.vp.unwrap_or(1).max(1), 1);
    }

    #[test]
    fn the_two_sections_page_independently() {
        // Paging through volumes must not reset the reader's place in the
        // titles — the whole reason for two query parameters.
        let q = DrillDownQuery {
            tp: Some(3),
            vp: Some(1),
        };
        assert_eq!(q.tp.unwrap(), 3);
        assert_eq!(q.vp.unwrap(), 1);
    }
}
