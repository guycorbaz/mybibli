use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, Path, State};
use axum::response::{Html, IntoResponse, Redirect};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::PaginatedList;
use crate::models::series::{SeriesModel, SeriesType};
use crate::utils::feedback_html;
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
    pub connection_status: crate::utils::ConnectionStatusContext,
    pub shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext,
    pub session_timeout_secs: u64,
    pub csrf_token: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_wishlist: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub nav_menu_open: String,
    pub list_title: String,
    pub add_label: String,
    pub name_label: String,
    pub type_label: String,
    pub type_open_label: String,
    pub type_closed_label: String,
    pub owned_label: String,
    pub total_label: String,
    pub gap_label: String,
    pub empty_heading: String,
    pub empty_body: String,
    pub empty_cta: String,
    pub prev_label: String,
    pub next_label: String,
    pub pagination_aria: String,
    pub series: PaginatedList<SeriesModel>,
    pub series_rows: Vec<SeriesListRow>,
    pub current_url: String,
    pub lang_toggle_aria: String,
}

pub async fn series_list_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    axum::extract::Query(params): axum::extract::Query<SeriesListQuery>,
) -> Result<impl IntoResponse, AppError> {
    // No auth required — anonymous read per FR95
    let pool = &state.pool;
    let loc = locale.0;

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

    // CR #35 (v1.7.11 slice): shared page-template fields built via base_context.
    let base = crate::utils::base_context(&session, loc, "series", &uri, state.session_timeout_secs());
    let template = SeriesListTemplate {
        lang: base.lang,
        role: base.role,
        current_page: base.current_page,
        skip_label: base.skip_label,
        connection_status: base.connection_status,
        shortcuts_cheat_sheet: base.shortcuts_cheat_sheet,
        session_timeout_secs: base.session_timeout_secs,
        csrf_token: base.csrf_token,
        nav_catalog: base.nav_catalog,
        nav_loans: base.nav_loans,
        nav_wishlist: base.nav_wishlist,
        nav_locations: base.nav_locations,
        nav_series: base.nav_series,
        nav_borrowers: base.nav_borrowers,
        nav_admin: base.nav_admin,
        nav_login: base.nav_login,
        nav_logout: base.nav_logout,
        nav_menu_open: base.nav_menu_open,
        list_title: rust_i18n::t!("series.list_title", locale = loc).to_string(),
        add_label: rust_i18n::t!("series.add", locale = loc).to_string(),
        name_label: rust_i18n::t!("series.name", locale = loc).to_string(),
        type_label: rust_i18n::t!("series.type", locale = loc).to_string(),
        type_open_label: rust_i18n::t!("series.type_open", locale = loc).to_string(),
        type_closed_label: rust_i18n::t!("series.type_closed", locale = loc).to_string(),
        owned_label: rust_i18n::t!("series.owned_count", locale = loc).to_string(),
        total_label: rust_i18n::t!("series.total_count", locale = loc).to_string(),
        gap_label: rust_i18n::t!("series.gap_count", locale = loc).to_string(),
        empty_heading: rust_i18n::t!("empty.series_heading", locale = loc).to_string(),
        empty_body: rust_i18n::t!("empty.series_body", locale = loc).to_string(),
        empty_cta: rust_i18n::t!("empty.series_cta", locale = loc).to_string(),
        prev_label: rust_i18n::t!("pagination.previous", locale = loc).to_string(),
        next_label: rust_i18n::t!("pagination.next", locale = loc).to_string(),
        pagination_aria: rust_i18n::t!("pagination.aria_label", locale = loc).to_string(),
        series,
        series_rows,
        current_url: base.current_url,
        lang_toggle_aria: base.lang_toggle_aria,
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::TemplateRenderFailed { template: "series" }),
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
    pub connection_status: crate::utils::ConnectionStatusContext,
    pub shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext,
    pub session_timeout_secs: u64,
    pub csrf_token: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_wishlist: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub nav_menu_open: String,
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
    pub back_label: String,
    pub positions: Vec<SeriesPositionInfo>,
    pub position_label: String,
    pub missing_label: String,
    /// CR #336: short prefix for volume number on series-detail cards.
    pub volume_short_label: String,
    pub grid_label: String,
    pub no_assignments_label: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
    // Fix #235: surfaced for the sort UI (which key is currently
    // selected, which direction). Stable identifiers — see
    // `SeriesSortKey::as_str` / `SortDir::as_str` for the canonical
    // string forms emitted into `?sort=…&dir=…`.
    pub current_sort: &'static str,
    pub current_dir: &'static str,
    pub label_sort_by: String,
    pub label_sort_position: String,
    pub label_sort_dewey: String,
    pub label_sort_title: String,
}

#[derive(serde::Deserialize, Default)]
pub struct SeriesDetailQuery {
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub dir: Option<String>,
}

pub async fn series_detail_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<u64>,
    axum::extract::Query(params): axum::extract::Query<SeriesDetailQuery>,
) -> Result<impl IntoResponse, AppError> {
    // No auth required — anonymous read per FR95
    let pool = &state.pool;
    let loc = locale.0;

    let series = SeriesModel::active_find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    let mut positions = SeriesService::get_series_positions(pool, &series).await?;
    let owned = positions.iter().filter(|p| p.title_id.is_some()).count() as u64;
    let gap = compute_gap(&series, owned);
    let series_name_for_grid = series.name.clone();

    // Fix #235: sort the position grid by the requested key / dir.
    // The owned / gap counters are computed BEFORE the sort because
    // they are scope-invariant — the values must match the underlying
    // catalog state, not whatever order the user is browsing in.
    let sort_key = crate::services::series::SeriesSortKey::from_param(params.sort.as_deref());
    let sort_dir = crate::services::series::SortDir::from_param(params.dir.as_deref());
    crate::services::series::sort_positions(&mut positions, sort_key, sort_dir);

    let base = crate::utils::base_context(&session, loc, "series", &uri, state.session_timeout_secs());
    let template = SeriesDetailTemplate {
        lang: base.lang,
        role: base.role,
        current_page: base.current_page,
        skip_label: base.skip_label,
        connection_status: base.connection_status,
        shortcuts_cheat_sheet: base.shortcuts_cheat_sheet,
        session_timeout_secs: base.session_timeout_secs,
        csrf_token: base.csrf_token,
        nav_catalog: base.nav_catalog,
        nav_loans: base.nav_loans,
        nav_wishlist: base.nav_wishlist,
        nav_locations: base.nav_locations,
        nav_series: base.nav_series,
        nav_borrowers: base.nav_borrowers,
        nav_admin: base.nav_admin,
        nav_login: base.nav_login,
        nav_logout: base.nav_logout,
        nav_menu_open: base.nav_menu_open,
        series,
        owned_count: owned,
        gap_count: gap,
        type_open_label: rust_i18n::t!("series.type_open", locale = loc).to_string(),
        type_closed_label: rust_i18n::t!("series.type_closed", locale = loc).to_string(),
        owned_label: rust_i18n::t!("series.owned_count", locale = loc).to_string(),
        total_label: rust_i18n::t!("series.total_count", locale = loc).to_string(),
        gap_label: rust_i18n::t!("series.gap_count", locale = loc).to_string(),
        edit_label: rust_i18n::t!("series.edit", locale = loc).to_string(),
        delete_label: rust_i18n::t!("series.delete", locale = loc).to_string(),
        back_label: rust_i18n::t!("series.back_to_list", locale = loc).to_string(),
        positions,
        position_label: rust_i18n::t!("series.position", locale = loc).to_string(),
        missing_label: rust_i18n::t!("series.missing_volume", locale = loc).to_string(),
        volume_short_label: rust_i18n::t!("series.volume_short", locale = loc).to_string(),
        grid_label: format!(
            "{} — {}",
            rust_i18n::t!("series.list_title", locale = loc),
            series_name_for_grid
        ),
        no_assignments_label: rust_i18n::t!("series.no_assignments", locale = loc).to_string(),
        current_url: base.current_url,
        lang_toggle_aria: base.lang_toggle_aria,
        current_sort: sort_key.as_str(),
        current_dir: sort_dir.as_str(),
        label_sort_by: rust_i18n::t!("browse.sort_by", locale = loc).to_string(),
        label_sort_position: rust_i18n::t!("series.sort.position", locale = loc).to_string(),
        label_sort_dewey: rust_i18n::t!("series.sort.dewey_code", locale = loc).to_string(),
        label_sort_title: rust_i18n::t!("series.sort.title", locale = loc).to_string(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::TemplateRenderFailed { template: "series" }),
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
    pub connection_status: crate::utils::ConnectionStatusContext,
    pub shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext,
    pub session_timeout_secs: u64,
    pub csrf_token: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_wishlist: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub nav_menu_open: String,
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
    pub current_url: String,
    pub lang_toggle_aria: String,
    pub series_type_help: crate::utils::TooltipData,
}

fn form_template_labels(
    session: &Session,
    session_timeout_secs: u64,
    loc: &str,
    uri: &axum::http::Uri,
) -> SeriesFormTemplate {
    let base = crate::utils::base_context(session, loc, "series", uri, session_timeout_secs);
    SeriesFormTemplate {
        lang: base.lang,
        role: base.role,
        current_page: base.current_page,
        skip_label: base.skip_label,
        connection_status: base.connection_status,
        shortcuts_cheat_sheet: base.shortcuts_cheat_sheet,
        session_timeout_secs: base.session_timeout_secs,
        csrf_token: base.csrf_token,
        nav_catalog: base.nav_catalog,
        nav_loans: base.nav_loans,
        nav_wishlist: base.nav_wishlist,
        nav_locations: base.nav_locations,
        nav_series: base.nav_series,
        nav_borrowers: base.nav_borrowers,
        nav_admin: base.nav_admin,
        nav_login: base.nav_login,
        nav_logout: base.nav_logout,
        nav_menu_open: base.nav_menu_open,
        is_edit: false,
        create_title: rust_i18n::t!("series.add", locale = loc).to_string(),
        edit_title: rust_i18n::t!("series.edit", locale = loc).to_string(),
        name_label: rust_i18n::t!("series.name", locale = loc).to_string(),
        description_label: rust_i18n::t!("series.description", locale = loc).to_string(),
        type_label: rust_i18n::t!("series.type", locale = loc).to_string(),
        type_open_label: rust_i18n::t!("series.type_open", locale = loc).to_string(),
        type_closed_label: rust_i18n::t!("series.type_closed", locale = loc).to_string(),
        total_label: rust_i18n::t!("series.total_count", locale = loc).to_string(),
        save_label: rust_i18n::t!("series.save", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("series.cancel", locale = loc).to_string(),
        back_label: rust_i18n::t!("series.back_to_list", locale = loc).to_string(),
        series_id: 0,
        version: 0,
        name_value: String::new(),
        description_value: String::new(),
        type_value: "open".to_string(),
        total_value: String::new(),
        current_url: base.current_url,
        lang_toggle_aria: base.lang_toggle_aria,
        series_type_help: crate::utils::TooltipData::with_icon(
            "tip-series-type",
            &rust_i18n::t!("help.series.type_summary", locale = loc),
            &rust_i18n::t!("help.series.type_text", locale = loc),
        ),
    }
}

pub async fn create_series_form(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;

    let template = form_template_labels(
        &session,
        state.session_timeout_secs(),
        locale.0,
        &uri,
    );

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::TemplateRenderFailed { template: "series" }),
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

/// Fix #296 — sibling of [`deserialize_optional_i32`] for ID-shaped
/// foreign-key fields. The location-edit form (and several others)
/// renders `<select name="parent_id"><option value="">None</option>
/// ...</select>` so picking "no parent" sends `parent_id=` (empty
/// string). `#[serde(default)]` only short-circuits ABSENT fields,
/// not empty strings, so the `u64` parser used to bail with
/// "cannot parse integer from empty string". Treat empty as `None`
/// just like the i32 + f64 helpers do.
pub fn deserialize_optional_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(ref v) if v.trim().is_empty() => Ok(None),
        Some(v) => v
            .trim()
            .parse::<u64>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// CR #243 — sibling of [`deserialize_optional_i32`] for monetary
/// inputs. Browser submits `<input type="number" step="0.01" value="">`
/// as `name=` (empty string), which `f64` can't parse — convert to
/// `None`. Accepts both `12.50` and `12,50` (FR-locale decimal comma)
/// so the form is forgiving when a user types the way their keyboard
/// suggests.
pub fn deserialize_optional_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(ref v) if v.trim().is_empty() => Ok(None),
        Some(v) => v
            .trim()
            .replace(',', ".")
            .parse::<f64>()
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
    Extension(locale): Extension<Locale>,
    axum::Form(form): axum::Form<CreateSeriesForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
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
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let series = SeriesModel::active_find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    let mut template =
        form_template_labels(&session, state.session_timeout_secs(), loc, &uri);
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
        Err(_) => Err(AppError::TemplateRenderFailed { template: "series" }),
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
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<UpdateSeriesForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
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

// ─── Delete confirmation modal (story 9-13) ─────────────

#[derive(Template)]
#[template(path = "fragments/series_delete_modal.html")]
pub struct SeriesDeleteModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
}

/// `GET /series/:id/delete-modal` — returns the rendered UX-DR8 Modal
/// fragment for the destructive delete-series flow. Librarian-gated
/// (admin > librarian, both pass). Direct browser navigation (no
/// `HX-Request` header) returns 405 — the modal fragment is meaningless
/// without page context. No `Allow:` response header is emitted (per the
/// 9-11 code-review patch — `Allow: GET` self-contradicts 405; we DO
/// support GET, just not without HTMX).
pub async fn delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<axum::response::Response, AppError> {
    // Preserve the series-detail return path so an anonymous user who
    // hits this URL directly (or whose session expired) lands back on the
    // series page after login, not on /home.
    session.require_role_with_return(Role::Librarian, &format!("/series/{id}"), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    if !is_htmx {
        return Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response());
    }

    let series = SeriesModel::active_find_by_id(pool, id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
        })?;

    // Title carries the series name via `%{name}` interpolation. Pass
    // the RAW name through `t!()` and let Askama's default auto-escape
    // (on `{{ title }}` in the macro) handle HTML safety. Pre-escaping
    // would double-escape (`<` → `&lt;` → `&amp;lt;`).
    let title = rust_i18n::t!(
        "series.delete_modal_title",
        locale = loc,
        name = series.name.as_str()
    )
    .to_string();
    let body_text = rust_i18n::t!("series.delete_modal_body", locale = loc).to_string();
    let body_html = format!("<p>{}</p>", crate::utils::html_escape(&body_text));

    tracing::debug!(
        series_id = id,
        user_id = ?session.user_id,
        "delete modal requested"
    );

    let template = SeriesDeleteModalTemplate {
        title,
        body_html,
        confirm_label: rust_i18n::t!("series.delete_modal_confirm", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/series/{}", series.id),
        csrf_token: session.csrf_token.clone(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => Err(AppError::Internal(format!(
            "series delete modal render: {e}"
        ))),
    }
}

// ─── Delete ─────────────────────────────────────────────

/// Trigger UX: see GET /series/:id/delete-modal (story 9-13).
pub async fn delete_series(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

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
                AppError::NotFound(msg) | AppError::Conflict(msg) => msg.clone(),
                _ => rust_i18n::t!("error.internal", locale = loc).to_string(),
            };
            Ok(Html(feedback_html("error", &message, "")).into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fix #296 regression — the three optional-from-empty-string
    /// deserializers must accept `""` (e.g., from `<select><option
    /// value="">None</option></select>`) and return `None` instead of
    /// trying to parse it as a number. Locks the contract for u64 (new
    /// in this fix), i32, and f64.
    #[derive(serde::Deserialize)]
    struct U64Holder {
        #[serde(default, deserialize_with = "deserialize_optional_u64")]
        v: Option<u64>,
    }

    #[derive(serde::Deserialize)]
    struct I32Holder {
        #[serde(default, deserialize_with = "deserialize_optional_i32")]
        v: Option<i32>,
    }

    #[derive(serde::Deserialize)]
    struct F64Holder {
        #[serde(default, deserialize_with = "deserialize_optional_f64")]
        v: Option<f64>,
    }

    #[test]
    fn deserialize_optional_u64_treats_empty_string_as_none() {
        let h: U64Holder = serde_urlencoded::from_str("v=").unwrap();
        assert_eq!(h.v, None);
    }

    #[test]
    fn deserialize_optional_u64_treats_whitespace_as_none() {
        let h: U64Holder = serde_urlencoded::from_str("v=%20%20").unwrap();
        assert_eq!(h.v, None);
    }

    #[test]
    fn deserialize_optional_u64_parses_valid_value() {
        let h: U64Holder = serde_urlencoded::from_str("v=42").unwrap();
        assert_eq!(h.v, Some(42));
    }

    #[test]
    fn deserialize_optional_u64_absent_field_is_none() {
        let h: U64Holder = serde_urlencoded::from_str("other=x").unwrap();
        assert_eq!(h.v, None);
    }

    #[test]
    fn deserialize_optional_u64_rejects_negative_value() {
        let result: Result<U64Holder, _> = serde_urlencoded::from_str("v=-1");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_optional_i32_treats_empty_string_as_none() {
        let h: I32Holder = serde_urlencoded::from_str("v=").unwrap();
        assert_eq!(h.v, None);
    }

    #[test]
    fn deserialize_optional_f64_treats_empty_string_as_none() {
        let h: F64Holder = serde_urlencoded::from_str("v=").unwrap();
        assert_eq!(h.v, None);
    }

    #[test]
    fn deserialize_optional_f64_accepts_comma_decimal() {
        let h: F64Holder = serde_urlencoded::from_str("v=12%2C50").unwrap();
        assert_eq!(h.v, Some(12.5));
    }
}
