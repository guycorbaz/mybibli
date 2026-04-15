use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Redirect};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::models::PaginatedList;
use crate::models::series::{SeriesModel, SeriesType};
use crate::routes::catalog::feedback_html_pub;
use crate::services::series::{SeriesPositionInfo, SeriesService};

/// Compute gap count for a closed series: total - owned, clamped to 0.
fn compute_gap(series: &SeriesModel, owned: u64) -> u64 {
    if series.series_type == SeriesType::Closed {
        let total = series.total_volume_count.unwrap_or(0).max(0) as u64;
        total.saturating_sub(owned)
    } else {
        0
    }
}

// ─── List page ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct SeriesListQuery {
    #[serde(default = "default_page")]
    pub page: u32,
}

fn default_page() -> u32 {
    1
}

/// A row in the series list with computed stats.
pub struct SeriesListRow {
    pub series: SeriesModel,
    pub owned_count: u64,
    pub gap_count: u64,
}

#[derive(Template)]
#[template(path = "pages/series_list.html")]
pub struct SeriesListTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub session_timeout_secs: u64,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub list_title: String,
    pub add_label: String,
    pub name_label: String,
    pub type_label: String,
    pub type_open_label: String,
    pub type_closed_label: String,
    pub owned_label: String,
    pub total_label: String,
    pub gap_label: String,
    pub empty_state: String,
    pub prev_label: String,
    pub next_label: String,
    pub series: PaginatedList<SeriesModel>,
    pub series_rows: Vec<SeriesListRow>,
}

pub async fn series_list_page(
    State(state): State<AppState>,
    session: Session,
    axum::extract::Query(params): axum::extract::Query<SeriesListQuery>,
) -> Result<impl IntoResponse, AppError> {
    // No auth required — anonymous read per FR95
    let pool = &state.pool;

    let series = SeriesModel::active_list(pool, params.page).await?;

    // Compute stats for each series
    let mut series_rows = Vec::with_capacity(series.items.len());
    for s in &series.items {
        let owned = SeriesModel::active_count_titles(pool, s.id).await?;
        let gap = compute_gap(s, owned);
        series_rows.push(SeriesListRow {
            series: s.clone(),
            owned_count: owned,
            gap_count: gap,
        });
    }

    let template = SeriesListTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "series",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        session_timeout_secs: state.session_timeout_secs(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
        nav_locations: rust_i18n::t!("nav.locations").to_string(),
        nav_series: rust_i18n::t!("nav.series").to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        list_title: rust_i18n::t!("series.list_title").to_string(),
        add_label: rust_i18n::t!("series.add").to_string(),
        name_label: rust_i18n::t!("series.name").to_string(),
        type_label: rust_i18n::t!("series.type").to_string(),
        type_open_label: rust_i18n::t!("series.type_open").to_string(),
        type_closed_label: rust_i18n::t!("series.type_closed").to_string(),
        owned_label: rust_i18n::t!("series.owned_count").to_string(),
        total_label: rust_i18n::t!("series.total_count").to_string(),
        gap_label: rust_i18n::t!("series.gap_count").to_string(),
        empty_state: rust_i18n::t!("series.empty_state").to_string(),
        prev_label: rust_i18n::t!("pagination.previous").to_string(),
        next_label: rust_i18n::t!("pagination.next").to_string(),
        series,
        series_rows,
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

// ─── Detail page ────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/series_detail.html")]
pub struct SeriesDetailTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub session_timeout_secs: u64,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub series: SeriesModel,
    pub owned_count: u64,
    pub gap_count: u64,
    pub type_open_label: String,
    pub type_closed_label: String,
    pub owned_label: String,
    pub total_label: String,
    pub gap_label: String,
    pub edit_label: String,
    pub delete_label: String,
    pub confirm_delete: String,
    pub back_label: String,
    pub positions: Vec<SeriesPositionInfo>,
    pub position_label: String,
    pub missing_label: String,
    pub grid_label: String,
    pub no_assignments_label: String,
}

pub async fn series_detail_page(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    // No auth required — anonymous read per FR95
    let pool = &state.pool;

    let series = SeriesModel::active_find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    let positions = SeriesService::get_series_positions(pool, &series).await?;
    let owned = positions.iter().filter(|p| p.title_id.is_some()).count() as u64;
    let gap = compute_gap(&series, owned);
    let series_name_for_grid = series.name.clone();

    let template = SeriesDetailTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "series",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        session_timeout_secs: state.session_timeout_secs(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
        nav_locations: rust_i18n::t!("nav.locations").to_string(),
        nav_series: rust_i18n::t!("nav.series").to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        series,
        owned_count: owned,
        gap_count: gap,
        type_open_label: rust_i18n::t!("series.type_open").to_string(),
        type_closed_label: rust_i18n::t!("series.type_closed").to_string(),
        owned_label: rust_i18n::t!("series.owned_count").to_string(),
        total_label: rust_i18n::t!("series.total_count").to_string(),
        gap_label: rust_i18n::t!("series.gap_count").to_string(),
        edit_label: rust_i18n::t!("series.edit").to_string(),
        delete_label: rust_i18n::t!("series.delete").to_string(),
        confirm_delete: rust_i18n::t!("series.confirm_delete").to_string(),
        back_label: rust_i18n::t!("series.back_to_list").to_string(),
        positions,
        position_label: rust_i18n::t!("series.position").to_string(),
        missing_label: rust_i18n::t!("series.missing_volume").to_string(),
        grid_label: format!(
            "{} — {}",
            rust_i18n::t!("series.list_title"),
            series_name_for_grid
        ),
        no_assignments_label: rust_i18n::t!("series.no_assignments").to_string(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

// ─── Create form + handler ──────────────────────────────

#[derive(Template)]
#[template(path = "pages/series_form.html")]
pub struct SeriesFormTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub session_timeout_secs: u64,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub is_edit: bool,
    pub create_title: String,
    pub edit_title: String,
    pub name_label: String,
    pub description_label: String,
    pub type_label: String,
    pub type_open_label: String,
    pub type_closed_label: String,
    pub total_label: String,
    pub save_label: String,
    pub cancel_label: String,
    pub back_label: String,
    pub series_id: u64,
    pub version: i32,
    pub name_value: String,
    pub description_value: String,
    pub type_value: String,
    pub total_value: String,
}

fn form_template_labels(session: &Session, session_timeout_secs: u64) -> SeriesFormTemplate {
    SeriesFormTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "series",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        session_timeout_secs,
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
        nav_locations: rust_i18n::t!("nav.locations").to_string(),
        nav_series: rust_i18n::t!("nav.series").to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        is_edit: false,
        create_title: rust_i18n::t!("series.add").to_string(),
        edit_title: rust_i18n::t!("series.edit").to_string(),
        name_label: rust_i18n::t!("series.name").to_string(),
        description_label: rust_i18n::t!("series.description").to_string(),
        type_label: rust_i18n::t!("series.type").to_string(),
        type_open_label: rust_i18n::t!("series.type_open").to_string(),
        type_closed_label: rust_i18n::t!("series.type_closed").to_string(),
        total_label: rust_i18n::t!("series.total_count").to_string(),
        save_label: rust_i18n::t!("series.save").to_string(),
        cancel_label: rust_i18n::t!("series.cancel").to_string(),
        back_label: rust_i18n::t!("series.back_to_list").to_string(),
        series_id: 0,
        version: 0,
        name_value: String::new(),
        description_value: String::new(),
        type_value: "open".to_string(),
        total_value: String::new(),
    }
}

pub async fn create_series_form(
    State(state): State<AppState>,
    session: Session,
    uri: axum::http::Uri,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path())?;

    let template = form_template_labels(&session, state.session_timeout_secs());

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

#[derive(Deserialize)]
pub struct CreateSeriesForm {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_series_type")]
    pub series_type: String,
    #[serde(default, deserialize_with = "deserialize_optional_i32")]
    pub total_volume_count: Option<i32>,
}

/// Deserialize an optional i32 from a form field that may be empty string.
pub fn deserialize_optional_i32<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(ref v) if v.trim().is_empty() => Ok(None),
        Some(v) => v
            .trim()
            .parse::<i32>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

fn default_series_type() -> String {
    "open".to_string()
}

pub async fn create_series(
    State(state): State<AppState>,
    session: Session,
    axum::Form(form): axum::Form<CreateSeriesForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    let series_type = form
        .series_type
        .parse::<SeriesType>()
        .unwrap_or(SeriesType::Open);

    let desc = form.description.as_deref().filter(|s| !s.trim().is_empty());

    let series =
        SeriesService::create_series(pool, &form.name, desc, series_type, form.total_volume_count)
            .await?;

    tracing::info!(series_id = series.id, name = %series.name, "Series created");
    Ok(Redirect::to(&format!("/series/{}", series.id)))
}

// ─── Edit form + handler ────────────────────────────────

pub async fn edit_series_form(
    State(state): State<AppState>,
    session: Session,
    uri: axum::http::Uri,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path())?;
    let pool = &state.pool;

    let series = SeriesModel::active_find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    let mut template = form_template_labels(&session, state.session_timeout_secs());
    template.is_edit = true;
    template.series_id = series.id;
    template.version = series.version;
    template.name_value = series.name;
    template.description_value = series.description.unwrap_or_default();
    template.type_value = series.series_type.to_string();
    template.total_value = series
        .total_volume_count
        .map(|n| n.to_string())
        .unwrap_or_default();

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

#[derive(Deserialize)]
pub struct UpdateSeriesForm {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_series_type")]
    pub series_type: String,
    #[serde(default, deserialize_with = "deserialize_optional_i32")]
    pub total_volume_count: Option<i32>,
    pub version: i32,
}

pub async fn update_series(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<UpdateSeriesForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    let series_type = form
        .series_type
        .parse::<SeriesType>()
        .unwrap_or(SeriesType::Open);

    let desc = form.description.as_deref().filter(|s| !s.trim().is_empty());

    SeriesService::update_series(
        pool,
        id,
        form.version,
        &form.name,
        desc,
        series_type,
        form.total_volume_count,
    )
    .await?;

    Ok(Redirect::to(&format!("/series/{id}")))
}

// ─── Delete ─────────────────────────────────────────────

pub async fn delete_series(
    State(state): State<AppState>,
    session: Session,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    match SeriesService::delete_series(pool, id).await {
        Ok(()) => {
            if is_htmx {
                Ok((
                    axum::http::StatusCode::OK,
                    [(
                        axum::http::header::HeaderName::from_static("hx-redirect"),
                        "/series".to_string(),
                    )],
                    String::new(),
                )
                    .into_response())
            } else {
                Ok(Redirect::to("/series").into_response())
            }
        }
        Err(e) => {
            let message = match &e {
                AppError::NotFound(msg) => msg.clone(),
                _ => rust_i18n::t!("error.internal").to_string(),
            };
            Ok(Html(feedback_html_pub("error", &message, "")).into_response())
        }
    }
}
