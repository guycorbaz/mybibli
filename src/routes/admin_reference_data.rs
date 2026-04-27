//! Admin → Reference data CRUD (story 8-4).
//!
//! Houses the 4 sub-section CRUD handlers (Genres, Volume States,
//! Contributor Roles, Location Node Types) plus the panel renderer the
//! admin shell calls when the active tab is `AdminTab::ReferenceData`.
//!
//! Lives in its own module because Foundation Rule #12 caps source files
//! at 2000 lines and `routes/admin.rs` is already at 1500+. Every POST
//! handler here is automatically CSRF-protected (8-2 middleware) and
//! every handler's first line is `session.require_role_with_return(Role::Admin, …)?`.

use askama::Template;
use axum::Extension;
use axum::extract::{Form, OriginalUri, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::CreateOutcome;
use crate::models::contributor_role::ContributorRoleModel;
use crate::models::genre::GenreModel;
use crate::models::location_node_type::LocationNodeTypeModel;
use crate::models::volume_state::VolumeStateModel;
use crate::routes::catalog::feedback_html_pub;

// ─── Form structs ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateRefForm {
    pub name: String,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct CreateVolumeStateForm {
    pub name: String,
    pub is_loanable: Option<String>,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct RenameRefForm {
    pub name: String,
    pub version: i32,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct DeleteRefForm {
    pub version: i32,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct LoanableToggleForm {
    pub is_loanable: Option<String>,
    pub version: i32,
    pub _csrf_token: String,
    pub force: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VersionQuery {
    pub version: i32,
}

// ─── Display structs ───────────────────────────────────────────────

#[derive(Debug, Clone)]
struct RefRowDisplay {
    id: u64,
    name: String,
    version: i32,
    is_loanable: bool,
    loanable_label: String,
    usage_count: i64,
    usage_chip: String,
    edit_aria: String,
    delete_aria: String,
}

#[derive(Debug, Clone)]
struct LoanSampleDisplay {
    label: String,
    borrower_name: String,
    loaned_at: String,
}

// ─── Template structs ──────────────────────────────────────────────

#[derive(Template)]
#[template(path = "fragments/admin_reference_data_panel.html")]
struct AdminReferenceDataPanel {
    csrf_token: String,
    panel_heading: String,
    section_genres: String,
    section_volume_states: String,
    section_contributor_roles: String,
    section_node_types: String,
    btn_add_genre: String,
    btn_add_state: String,
    btn_add_role: String,
    btn_add_node_type: String,
    btn_save: String,
    btn_cancel: String,
    loanable_label: String,
    genres_list_html: String,
    volume_states_list_html: String,
    roles_list_html: String,
    node_types_list_html: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_genres_list.html")]
struct AdminRefGenresList {
    entries: Vec<RefRowDisplay>,
    empty_state: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_volume_states_list.html")]
struct AdminRefVolumeStatesList {
    entries: Vec<RefRowDisplay>,
    empty_state: String,
    csrf_token: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_roles_list.html")]
struct AdminRefRolesList {
    entries: Vec<RefRowDisplay>,
    empty_state: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_node_types_list.html")]
struct AdminRefNodeTypesList {
    entries: Vec<RefRowDisplay>,
    empty_state: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_genre_row.html")]
struct AdminRefGenreRow {
    entry: RefRowDisplay,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_volume_state_row.html")]
struct AdminRefVolumeStateRow {
    entry: RefRowDisplay,
    csrf_token: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_role_row.html")]
struct AdminRefRoleRow {
    entry: RefRowDisplay,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_node_type_row.html")]
struct AdminRefNodeTypeRow {
    entry: RefRowDisplay,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_delete_modal.html")]
struct AdminRefDeleteModal {
    csrf_token: String,
    modal_heading: String,
    modal_body: String,
    btn_delete: String,
    btn_cancel: String,
    delete_endpoint: String,
    list_target: String,
    version: i32,
}

#[derive(Template)]
#[template(path = "fragments/admin_ref_loanable_warning_modal.html")]
struct AdminRefLoanableWarningModal {
    csrf_token: String,
    warning_heading: String,
    warning_body: String,
    sample_heading: String,
    btn_apply_anyway: String,
    btn_cancel: String,
    confirm_endpoint: String,
    row_target: String,
    row_revert_endpoint: String,
    version: i32,
    sample_loans: Vec<LoanSampleDisplay>,
}

// ─── Section identifier (for shared helpers) ───────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Genres,
    VolumeStates,
    ContributorRoles,
    NodeTypes,
}

impl Section {
    fn url_segment(self) -> &'static str {
        match self {
            Section::Genres => "genres",
            Section::VolumeStates => "volume-states",
            Section::ContributorRoles => "contributor-roles",
            Section::NodeTypes => "node-types",
        }
    }

    fn list_target(self) -> &'static str {
        match self {
            Section::Genres => "#admin-ref-genres-list",
            Section::VolumeStates => "#admin-ref-volume-states-list",
            Section::ContributorRoles => "#admin-ref-roles-list",
            Section::NodeTypes => "#admin-ref-node-types-list",
        }
    }

    fn entity_label_key(self) -> &'static str {
        match self {
            Section::Genres => "admin.reference_data.entity_genre",
            Section::VolumeStates => "admin.reference_data.entity_state",
            Section::ContributorRoles => "admin.reference_data.entity_role",
            Section::NodeTypes => "admin.reference_data.entity_node_type",
        }
    }

    fn plural_label_key(self) -> &'static str {
        match self {
            Section::Genres => "admin.reference_data.plural_genre",
            Section::VolumeStates => "admin.reference_data.plural_state",
            Section::ContributorRoles => "admin.reference_data.plural_role",
            Section::NodeTypes => "admin.reference_data.plural_node_type",
        }
    }
}

// ─── Validation helpers ────────────────────────────────────────────

fn validate_name(name: &str, loc: &'static str) -> Result<String, AppError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.reference_data.name_empty", locale = loc).to_string(),
        ));
    }
    if trimmed.chars().count() > 255 {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.reference_data.name_too_long", locale = loc).to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

fn checkbox_to_bool(v: &Option<String>) -> bool {
    matches!(v.as_deref(), Some(s) if !s.is_empty() && s != "off" && s != "false")
}

/// Translate the `Conflict("name_taken")` literal returned by the model
/// layer into a user-facing localized message. Other AppErrors pass
/// through unchanged.
fn map_create_or_rename_conflict(err: AppError, loc: &'static str, name: &str) -> AppError {
    match err {
        AppError::Conflict(msg) if msg == "name_taken" => AppError::Conflict(
            rust_i18n::t!("error.reference_data.name_taken", locale = loc, name = name)
                .to_string(),
        ),
        _ => err,
    }
}

// ─── Display builders ──────────────────────────────────────────────

fn make_row(
    id: u64,
    name: &str,
    version: i32,
    is_loanable: bool,
    usage_count: i64,
    loc: &'static str,
) -> RefRowDisplay {
    RefRowDisplay {
        id,
        name: name.to_string(),
        version,
        is_loanable,
        loanable_label: rust_i18n::t!("admin.reference_data.loanable_label", locale = loc)
            .to_string(),
        usage_count,
        usage_chip: rust_i18n::t!(
            "admin.reference_data.usage_count_chip",
            locale = loc,
            count = usage_count
        )
        .to_string(),
        edit_aria: rust_i18n::t!(
            "admin.reference_data.edit_aria",
            locale = loc,
            name = name
        )
        .to_string(),
        delete_aria: rust_i18n::t!(
            "admin.reference_data.delete_aria",
            locale = loc,
            name = name
        )
        .to_string(),
    }
}

async fn build_genre_rows(
    pool: &crate::db::DbPool,
    loc: &'static str,
) -> Result<Vec<RefRowDisplay>, AppError> {
    let entries = GenreModel::list_all(pool).await?;
    let mut rows = Vec::with_capacity(entries.len());
    for e in entries {
        let usage = GenreModel::count_usage(pool, e.id).await?;
        rows.push(make_row(e.id, &e.name, e.version, false, usage, loc));
    }
    Ok(rows)
}

async fn build_volume_state_rows(
    pool: &crate::db::DbPool,
    loc: &'static str,
) -> Result<Vec<RefRowDisplay>, AppError> {
    let entries = VolumeStateModel::list_all(pool).await?;
    let mut rows = Vec::with_capacity(entries.len());
    for e in entries {
        let usage = VolumeStateModel::count_usage(pool, e.id).await?;
        rows.push(make_row(e.id, &e.name, e.version, e.is_loanable, usage, loc));
    }
    Ok(rows)
}

async fn build_role_rows(
    pool: &crate::db::DbPool,
    loc: &'static str,
) -> Result<Vec<RefRowDisplay>, AppError> {
    let entries = ContributorRoleModel::list_all(pool).await?;
    let mut rows = Vec::with_capacity(entries.len());
    for e in entries {
        let usage = ContributorRoleModel::count_usage(pool, e.id).await?;
        rows.push(make_row(e.id, &e.name, e.version, false, usage, loc));
    }
    Ok(rows)
}

async fn build_node_type_rows(
    pool: &crate::db::DbPool,
    loc: &'static str,
) -> Result<Vec<RefRowDisplay>, AppError> {
    let entries = LocationNodeTypeModel::list_all(pool).await?;
    let mut rows = Vec::with_capacity(entries.len());
    for e in entries {
        let usage = LocationNodeTypeModel::count_usage(pool, e.id).await?;
        rows.push(make_row(e.id, &e.name, e.version, false, usage, loc));
    }
    Ok(rows)
}

fn render_genres_list(entries: Vec<RefRowDisplay>, _csrf: &str, loc: &'static str) -> Result<String, AppError> {
    AdminRefGenresList {
        entries,
        empty_state: rust_i18n::t!("admin.reference_data.empty_state", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("genres list render failed".to_string()))
}

fn render_volume_states_list(entries: Vec<RefRowDisplay>, csrf: &str, loc: &'static str) -> Result<String, AppError> {
    AdminRefVolumeStatesList {
        entries,
        empty_state: rust_i18n::t!("admin.reference_data.empty_state", locale = loc).to_string(),
        csrf_token: csrf.to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("volume states list render failed".to_string()))
}

fn render_roles_list(entries: Vec<RefRowDisplay>, _csrf: &str, loc: &'static str) -> Result<String, AppError> {
    AdminRefRolesList {
        entries,
        empty_state: rust_i18n::t!("admin.reference_data.empty_state", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("roles list render failed".to_string()))
}

fn render_node_types_list(entries: Vec<RefRowDisplay>, _csrf: &str, loc: &'static str) -> Result<String, AppError> {
    AdminRefNodeTypesList {
        entries,
        empty_state: rust_i18n::t!("admin.reference_data.empty_state", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("node types list render failed".to_string()))
}

// ─── Public panel renderer (called by admin.rs render_panel) ──────

pub async fn render_panel_html(
    state: &AppState,
    loc: &'static str,
    session: &Session,
) -> Result<String, AppError> {
    let pool = &state.pool;
    let csrf = session.csrf_token.clone();

    let genre_rows = build_genre_rows(pool, loc).await?;
    let volume_state_rows = build_volume_state_rows(pool, loc).await?;
    let role_rows = build_role_rows(pool, loc).await?;
    let node_type_rows = build_node_type_rows(pool, loc).await?;

    let genres_list_html = render_genres_list(genre_rows, &csrf, loc)?;
    let volume_states_list_html = render_volume_states_list(volume_state_rows, &csrf, loc)?;
    let roles_list_html = render_roles_list(role_rows, &csrf, loc)?;
    let node_types_list_html = render_node_types_list(node_type_rows, &csrf, loc)?;

    let panel = AdminReferenceDataPanel {
        csrf_token: csrf,
        panel_heading: rust_i18n::t!("admin.reference_data.panel_heading", locale = loc).to_string(),
        section_genres: rust_i18n::t!("admin.reference_data.section_genres", locale = loc)
            .to_string(),
        section_volume_states: rust_i18n::t!(
            "admin.reference_data.section_volume_states",
            locale = loc
        )
        .to_string(),
        section_contributor_roles: rust_i18n::t!(
            "admin.reference_data.section_contributor_roles",
            locale = loc
        )
        .to_string(),
        section_node_types: rust_i18n::t!(
            "admin.reference_data.section_node_types",
            locale = loc
        )
        .to_string(),
        btn_add_genre: rust_i18n::t!("admin.reference_data.btn_add_genre", locale = loc)
            .to_string(),
        btn_add_state: rust_i18n::t!("admin.reference_data.btn_add_state", locale = loc)
            .to_string(),
        btn_add_role: rust_i18n::t!("admin.reference_data.btn_add_role", locale = loc).to_string(),
        btn_add_node_type: rust_i18n::t!(
            "admin.reference_data.btn_add_node_type",
            locale = loc
        )
        .to_string(),
        btn_save: rust_i18n::t!("admin.reference_data.btn_save", locale = loc).to_string(),
        btn_cancel: rust_i18n::t!("admin.reference_data.btn_cancel", locale = loc).to_string(),
        loanable_label: rust_i18n::t!("admin.reference_data.loanable_label", locale = loc)
            .to_string(),
        genres_list_html,
        volume_states_list_html,
        roles_list_html,
        node_types_list_html,
    };
    panel
        .render()
        .map_err(|_| AppError::Internal("admin reference data panel render failed".to_string()))
}

// ─── Panel route (HTMX swap or full page via admin.rs render_admin) ─

pub async fn admin_reference_data_panel(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
) -> Result<Response, AppError> {
    let return_path = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/admin?tab=reference_data".to_string());
    session.require_role_with_return(Role::Admin, &return_path)?;

    if is_htmx {
        let panel_html = render_panel_html(&state, locale.0, &session).await?;
        Ok((StatusCode::OK, Html(panel_html)).into_response())
    } else {
        crate::routes::admin::render_admin_for_reference_data(
            &state, &session, locale.0, &uri,
        )
        .await
    }
}

// ─── Helpers — render row + feedback as HtmxResponse ───────────────

fn success_feedback(loc: &'static str, key: &str, name: &str) -> String {
    let msg = rust_i18n::t!(key, locale = loc, name = name).to_string();
    feedback_html_pub("success", &msg, "")
}

fn success_feedback_with_count(loc: &'static str, key: &str, name: &str, count: u64) -> String {
    let msg = rust_i18n::t!(key, locale = loc, name = name, count = count).to_string();
    feedback_html_pub("success", &msg, "")
}

// ─── Genres CRUD ───────────────────────────────────────────────────

pub async fn genres_section(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let entries = build_genre_rows(&state.pool, loc).await?;
    let html = render_genres_list(entries, &session.csrf_token, loc)?;
    Ok(Html(html))
}

pub async fn genres_create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<CreateRefForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;
    let outcome = GenreModel::create(&state.pool, &name)
        .await
        .map_err(|err| map_create_or_rename_conflict(err, loc, &name))?;
    let feedback = match outcome {
        CreateOutcome::Created(_) => success_feedback(loc, "success.reference_data.created", &name),
        CreateOutcome::Reactivated(_) => {
            success_feedback(loc, "success.reference_data.reactivated", &name)
        }
    };
    let entries = build_genre_rows(&state.pool, loc).await?;
    let list_html = render_genres_list(entries, &session.csrf_token, loc)?;
    Ok(HtmxResponse {
        main: list_html,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn genres_rename(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<RenameRefForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;
    GenreModel::rename(&state.pool, id, form.version, &name)
        .await
        .map_err(|err| map_create_or_rename_conflict(err, loc, &name))?;
    let row = GenreModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("genre".to_string()))?;
    let usage = GenreModel::count_usage(&state.pool, id).await?;
    let entry = make_row(row.id, &row.name, row.version, false, usage, loc);
    let row_html = AdminRefGenreRow { entry }
        .render()
        .map_err(|_| AppError::Internal("genre row render failed".to_string()))?;
    let feedback = success_feedback(loc, "success.reference_data.renamed", &name);
    Ok(HtmxResponse {
        main: row_html,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn genres_delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Query(q): Query<VersionQuery>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let row = GenreModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("genre".to_string()))?;
    let html = render_delete_modal(
        Section::Genres,
        loc,
        &session.csrf_token,
        id,
        &row.name,
        q.version,
    )?;
    Ok(Html(html))
}

pub async fn genres_delete(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<DeleteRefForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let row = GenreModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("genre".to_string()))?;
    let usage = GenreModel::count_usage(&state.pool, id).await?;
    if usage > 0 {
        return Err(in_use_conflict(loc, Section::Genres, usage));
    }
    GenreModel::soft_delete(&state.pool, id, form.version).await?;

    // Render fresh list + close modal via OOB (admin-modal-slot cleared).
    let entries = build_genre_rows(&state.pool, loc).await?;
    let list_html = render_genres_list(entries, &session.csrf_token, loc)?;
    let feedback = success_feedback(loc, "success.reference_data.deleted", &row.name);
    Ok(HtmxResponse {
        main: list_html,
        oob: vec![
            OobUpdate {
                target: "feedback-list".to_string(),
                content: feedback,
            },
            OobUpdate {
                target: "admin-modal-slot".to_string(),
                content: String::new(),
            },
        ],
    })
}

// ─── Volume States CRUD ────────────────────────────────────────────

pub async fn volume_states_section(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let entries = build_volume_state_rows(&state.pool, loc).await?;
    let html = render_volume_states_list(entries, &session.csrf_token, loc)?;
    Ok(Html(html))
}

pub async fn volume_states_create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<CreateVolumeStateForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;
    let is_loanable = checkbox_to_bool(&form.is_loanable);
    let outcome = VolumeStateModel::create(&state.pool, &name, is_loanable)
        .await
        .map_err(|err| map_create_or_rename_conflict(err, loc, &name))?;
    let feedback = match outcome {
        CreateOutcome::Created(_) => success_feedback(loc, "success.reference_data.created", &name),
        CreateOutcome::Reactivated(_) => {
            success_feedback(loc, "success.reference_data.reactivated", &name)
        }
    };
    let entries = build_volume_state_rows(&state.pool, loc).await?;
    let list_html = render_volume_states_list(entries, &session.csrf_token, loc)?;
    Ok(HtmxResponse {
        main: list_html,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn volume_states_rename(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<RenameRefForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;
    VolumeStateModel::rename(&state.pool, id, form.version, &name)
        .await
        .map_err(|err| map_create_or_rename_conflict(err, loc, &name))?;
    let row = VolumeStateModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("volume_state".to_string()))?;
    let usage = VolumeStateModel::count_usage(&state.pool, id).await?;
    let entry = make_row(row.id, &row.name, row.version, row.is_loanable, usage, loc);
    let row_html = AdminRefVolumeStateRow {
        entry,
        csrf_token: session.csrf_token.clone(),
    }
    .render()
    .map_err(|_| AppError::Internal("volume state row render failed".to_string()))?;
    let feedback = success_feedback(loc, "success.reference_data.renamed", &name);
    Ok(HtmxResponse {
        main: row_html,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn volume_states_delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Query(q): Query<VersionQuery>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let row = VolumeStateModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("volume_state".to_string()))?;
    let html = render_delete_modal(
        Section::VolumeStates,
        loc,
        &session.csrf_token,
        id,
        &row.name,
        q.version,
    )?;
    Ok(Html(html))
}

pub async fn volume_states_delete(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<DeleteRefForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let row = VolumeStateModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("volume_state".to_string()))?;
    let usage = VolumeStateModel::count_usage(&state.pool, id).await?;
    if usage > 0 {
        return Err(in_use_conflict(loc, Section::VolumeStates, usage));
    }
    VolumeStateModel::soft_delete(&state.pool, id, form.version).await?;
    let entries = build_volume_state_rows(&state.pool, loc).await?;
    let list_html = render_volume_states_list(entries, &session.csrf_token, loc)?;
    let feedback = success_feedback(loc, "success.reference_data.deleted", &row.name);
    Ok(HtmxResponse {
        main: list_html,
        oob: vec![
            OobUpdate {
                target: "feedback-list".to_string(),
                content: feedback,
            },
            OobUpdate {
                target: "admin-modal-slot".to_string(),
                content: String::new(),
            },
        ],
    })
}

pub async fn volume_states_loanable_toggle(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<LoanableToggleForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let new_loanable = checkbox_to_bool(&form.is_loanable);
    let force = checkbox_to_bool(&form.force);

    let row = VolumeStateModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("volume_state".to_string()))?;

    // Toggle-OFF that drops loanable from `true → false` may need confirmation.
    if !force && row.is_loanable && !new_loanable {
        let active = VolumeStateModel::count_active_loans_for_state(&state.pool, id).await?;
        if active > 0 {
            // Surface the warning modal — server inspects DB so the client
            // cannot bypass the check by lying about `force=true`.
            let samples = sample_active_loans(&state.pool, id, 5).await?;
            let modal_html = render_loanable_warning_modal(
                loc,
                &session.csrf_token,
                id,
                &row.name,
                form.version,
                active,
                samples,
            )?;
            return Ok((StatusCode::OK, Html(modal_html)).into_response());
        }
    }

    apply_loanable_toggle(
        &state.pool,
        &session,
        loc,
        id,
        form.version,
        new_loanable,
        false,
    )
    .await
}

pub async fn volume_states_loanable_confirm(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<LoanableToggleForm>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let new_loanable = checkbox_to_bool(&form.is_loanable);
    apply_loanable_toggle(
        &state.pool,
        &session,
        loc,
        id,
        form.version,
        new_loanable,
        true,
    )
    .await
}

async fn apply_loanable_toggle(
    pool: &crate::db::DbPool,
    session: &Session,
    loc: &'static str,
    id: u64,
    version: i32,
    new_loanable: bool,
    close_modal: bool,
) -> Result<Response, AppError> {
    VolumeStateModel::set_loanable(pool, id, version, new_loanable).await?;
    let row = VolumeStateModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("volume_state".to_string()))?;
    let usage = VolumeStateModel::count_usage(pool, id).await?;
    let entry = make_row(row.id, &row.name, row.version, row.is_loanable, usage, loc);
    let row_html = AdminRefVolumeStateRow {
        entry,
        csrf_token: session.csrf_token.clone(),
    }
    .render()
    .map_err(|_| AppError::Internal("volume state row render failed".to_string()))?;
    let feedback_key = if new_loanable {
        "success.reference_data.loanable_on"
    } else {
        "success.reference_data.loanable_off"
    };
    let feedback = success_feedback(loc, feedback_key, &row.name);
    let mut oob = vec![OobUpdate {
        target: "feedback-list".to_string(),
        content: feedback,
    }];
    if close_modal {
        oob.push(OobUpdate {
            target: "admin-modal-slot".to_string(),
            content: String::new(),
        });
    }
    Ok(HtmxResponse {
        main: row_html,
        oob,
    }
    .into_response())
}

pub async fn volume_states_row_view(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let row = VolumeStateModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("volume_state".to_string()))?;
    let usage = VolumeStateModel::count_usage(&state.pool, id).await?;
    let entry = make_row(row.id, &row.name, row.version, row.is_loanable, usage, loc);
    let html = AdminRefVolumeStateRow {
        entry,
        csrf_token: session.csrf_token.clone(),
    }
    .render()
    .map_err(|_| AppError::Internal("volume state row render failed".to_string()))?;
    Ok(Html(html))
}

// ─── Contributor Roles CRUD ────────────────────────────────────────

pub async fn roles_section(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let entries = build_role_rows(&state.pool, loc).await?;
    let html = render_roles_list(entries, &session.csrf_token, loc)?;
    Ok(Html(html))
}

pub async fn roles_create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<CreateRefForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;
    let outcome = ContributorRoleModel::create(&state.pool, &name)
        .await
        .map_err(|err| map_create_or_rename_conflict(err, loc, &name))?;
    let feedback = match outcome {
        CreateOutcome::Created(_) => success_feedback(loc, "success.reference_data.created", &name),
        CreateOutcome::Reactivated(_) => {
            success_feedback(loc, "success.reference_data.reactivated", &name)
        }
    };
    let entries = build_role_rows(&state.pool, loc).await?;
    let list_html = render_roles_list(entries, &session.csrf_token, loc)?;
    Ok(HtmxResponse {
        main: list_html,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn roles_rename(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<RenameRefForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;
    ContributorRoleModel::rename(&state.pool, id, form.version, &name)
        .await
        .map_err(|err| map_create_or_rename_conflict(err, loc, &name))?;
    let row = ContributorRoleModel::get(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("contributor_role".to_string()))?;
    let usage = ContributorRoleModel::count_usage(&state.pool, id).await?;
    let entry = make_row(row.id, &row.name, row.version, false, usage, loc);
    let row_html = AdminRefRoleRow { entry }
        .render()
        .map_err(|_| AppError::Internal("role row render failed".to_string()))?;
    let feedback = success_feedback(loc, "success.reference_data.renamed", &name);
    Ok(HtmxResponse {
        main: row_html,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn roles_delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Query(q): Query<VersionQuery>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let row = ContributorRoleModel::get(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("contributor_role".to_string()))?;
    let html = render_delete_modal(
        Section::ContributorRoles,
        loc,
        &session.csrf_token,
        id,
        &row.name,
        q.version,
    )?;
    Ok(Html(html))
}

pub async fn roles_delete(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<DeleteRefForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let row = ContributorRoleModel::get(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("contributor_role".to_string()))?;
    let usage = ContributorRoleModel::count_usage(&state.pool, id).await?;
    if usage > 0 {
        return Err(in_use_conflict(loc, Section::ContributorRoles, usage));
    }
    ContributorRoleModel::soft_delete(&state.pool, id, form.version).await?;
    let entries = build_role_rows(&state.pool, loc).await?;
    let list_html = render_roles_list(entries, &session.csrf_token, loc)?;
    let feedback = success_feedback(loc, "success.reference_data.deleted", &row.name);
    Ok(HtmxResponse {
        main: list_html,
        oob: vec![
            OobUpdate {
                target: "feedback-list".to_string(),
                content: feedback,
            },
            OobUpdate {
                target: "admin-modal-slot".to_string(),
                content: String::new(),
            },
        ],
    })
}

// ─── Location Node Types CRUD ──────────────────────────────────────

pub async fn node_types_section(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let entries = build_node_type_rows(&state.pool, loc).await?;
    let html = render_node_types_list(entries, &session.csrf_token, loc)?;
    Ok(Html(html))
}

pub async fn node_types_create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<CreateRefForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;
    let outcome = LocationNodeTypeModel::create(&state.pool, &name)
        .await
        .map_err(|err| map_create_or_rename_conflict(err, loc, &name))?;
    let feedback = match outcome {
        CreateOutcome::Created(_) => success_feedback(loc, "success.reference_data.created", &name),
        CreateOutcome::Reactivated(_) => {
            success_feedback(loc, "success.reference_data.reactivated", &name)
        }
    };
    let entries = build_node_type_rows(&state.pool, loc).await?;
    let list_html = render_node_types_list(entries, &session.csrf_token, loc)?;
    Ok(HtmxResponse {
        main: list_html,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn node_types_rename(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<RenameRefForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;

    let cascade_rows = LocationNodeTypeModel::rename(&state.pool, id, form.version, &name)
        .await
        .map_err(|err| map_create_or_rename_conflict(err, loc, &name))?;

    tracing::info!(
        node_type_id = id,
        new_name = %name,
        cascade_rows = cascade_rows,
        "Renamed location_node_type"
    );

    let row = LocationNodeTypeModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("location_node_type".to_string()))?;
    let usage = LocationNodeTypeModel::count_usage(&state.pool, id).await?;
    let entry = make_row(row.id, &row.name, row.version, false, usage, loc);
    let row_html = AdminRefNodeTypeRow { entry }
        .render()
        .map_err(|_| AppError::Internal("node type row render failed".to_string()))?;

    let feedback = if cascade_rows > 0 {
        success_feedback_with_count(
            loc,
            "success.reference_data.node_type_renamed_cascaded",
            &name,
            cascade_rows,
        )
    } else {
        success_feedback(loc, "success.reference_data.renamed", &name)
    };

    Ok(HtmxResponse {
        main: row_html,
        oob: vec![OobUpdate {
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn node_types_delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Query(q): Query<VersionQuery>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let row = LocationNodeTypeModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("location_node_type".to_string()))?;
    let html = render_delete_modal(
        Section::NodeTypes,
        loc,
        &session.csrf_token,
        id,
        &row.name,
        q.version,
    )?;
    Ok(Html(html))
}

pub async fn node_types_delete(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<DeleteRefForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    let loc = locale.0;
    let row = LocationNodeTypeModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("location_node_type".to_string()))?;
    let usage = LocationNodeTypeModel::count_usage(&state.pool, id).await?;
    if usage > 0 {
        return Err(in_use_conflict(loc, Section::NodeTypes, usage));
    }
    LocationNodeTypeModel::soft_delete(&state.pool, id, form.version).await?;
    let entries = build_node_type_rows(&state.pool, loc).await?;
    let list_html = render_node_types_list(entries, &session.csrf_token, loc)?;
    let feedback = success_feedback(loc, "success.reference_data.deleted", &row.name);
    Ok(HtmxResponse {
        main: list_html,
        oob: vec![
            OobUpdate {
                target: "feedback-list".to_string(),
                content: feedback,
            },
            OobUpdate {
                target: "admin-modal-slot".to_string(),
                content: String::new(),
            },
        ],
    })
}

// ─── Modal renderers ───────────────────────────────────────────────

fn render_delete_modal(
    section: Section,
    loc: &'static str,
    csrf: &str,
    id: u64,
    item_name: &str,
    version: i32,
) -> Result<String, AppError> {
    let entity = rust_i18n::t!(section.entity_label_key(), locale = loc).to_string();
    let modal = AdminRefDeleteModal {
        csrf_token: csrf.to_string(),
        modal_heading: rust_i18n::t!(
            "admin.reference_data.delete_modal_heading",
            locale = loc,
            entity = &entity
        )
        .to_string(),
        modal_body: rust_i18n::t!(
            "admin.reference_data.delete_modal_body",
            locale = loc,
            name = item_name
        )
        .to_string(),
        btn_delete: rust_i18n::t!("admin.reference_data.btn_delete", locale = loc).to_string(),
        btn_cancel: rust_i18n::t!("admin.reference_data.btn_cancel", locale = loc).to_string(),
        delete_endpoint: format!("/admin/reference-data/{}/{}/delete", section.url_segment(), id),
        list_target: section.list_target().to_string(),
        version,
    };
    modal
        .render()
        .map_err(|_| AppError::Internal("delete modal render failed".to_string()))
}

fn render_loanable_warning_modal(
    loc: &'static str,
    csrf: &str,
    id: u64,
    name: &str,
    version: i32,
    active_count: i64,
    sample_loans: Vec<LoanSampleDisplay>,
) -> Result<String, AppError> {
    let modal = AdminRefLoanableWarningModal {
        csrf_token: csrf.to_string(),
        warning_heading: rust_i18n::t!(
            "admin.reference_data.loanable_warning_heading",
            locale = loc,
            count = active_count
        )
        .to_string(),
        warning_body: rust_i18n::t!(
            "admin.reference_data.loanable_warning_body",
            locale = loc,
            name = name,
            count = active_count
        )
        .to_string(),
        sample_heading: rust_i18n::t!(
            "admin.reference_data.loanable_warning_sample_heading",
            locale = loc
        )
        .to_string(),
        btn_apply_anyway: rust_i18n::t!(
            "admin.reference_data.btn_apply_anyway",
            locale = loc
        )
        .to_string(),
        btn_cancel: rust_i18n::t!("admin.reference_data.btn_cancel", locale = loc).to_string(),
        confirm_endpoint: format!("/admin/reference-data/volume-states/{}/loanable/confirm", id),
        row_target: format!("#admin-ref-volume-states-row-{}", id),
        row_revert_endpoint: format!("/admin/reference-data/volume-states/{}/row", id),
        version,
        sample_loans,
    };
    modal
        .render()
        .map_err(|_| AppError::Internal("loanable warning modal render failed".to_string()))
}

fn in_use_conflict(loc: &'static str, section: Section, count: i64) -> AppError {
    let singular = rust_i18n::t!(section.entity_label_key(), locale = loc).to_string();
    let plural = rust_i18n::t!(section.plural_label_key(), locale = loc).to_string();
    let msg = rust_i18n::t!(
        "error.reference_data.in_use_no_link",
        locale = loc,
        count = count,
        plural = &plural,
        singular = &singular
    )
    .to_string();
    AppError::Conflict(msg)
}

async fn sample_active_loans(
    pool: &crate::db::DbPool,
    state_id: u64,
    limit: i64,
) -> Result<Vec<LoanSampleDisplay>, AppError> {
    let rows = sqlx::query(
        "SELECT v.label AS volume_label, b.name AS borrower_name, l.loaned_at \
           FROM loans l \
           JOIN volumes v ON l.volume_id = v.id \
           JOIN borrowers b ON l.borrower_id = b.id \
          WHERE v.condition_state_id = ? \
            AND l.returned_at IS NULL \
            AND l.deleted_at IS NULL \
            AND v.deleted_at IS NULL \
            AND b.deleted_at IS NULL \
          ORDER BY l.loaned_at DESC \
          LIMIT ?",
    )
    .bind(state_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    use sqlx::Row;
    let mut samples = Vec::with_capacity(rows.len());
    for r in &rows {
        let label: String = r.try_get("volume_label").unwrap_or_default();
        let borrower: String = r.try_get("borrower_name").unwrap_or_default();
        let loaned_at: chrono::NaiveDateTime = r
            .try_get("loaned_at")
            .unwrap_or_else(|_| chrono::NaiveDateTime::default());
        samples.push(LoanSampleDisplay {
            label,
            borrower_name: borrower,
            loaned_at: loaned_at.format("%Y-%m-%d").to_string(),
        });
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name_rejects_empty() {
        assert!(matches!(
            validate_name("   ", "en"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn test_validate_name_rejects_too_long() {
        let long = "a".repeat(256);
        assert!(matches!(
            validate_name(&long, "en"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn test_validate_name_trims() {
        assert_eq!(validate_name("  hello  ", "en").unwrap(), "hello");
    }

    #[test]
    fn test_checkbox_to_bool() {
        assert!(checkbox_to_bool(&Some("on".to_string())));
        assert!(checkbox_to_bool(&Some("true".to_string())));
        assert!(!checkbox_to_bool(&None));
        assert!(!checkbox_to_bool(&Some("off".to_string())));
        assert!(!checkbox_to_bool(&Some(String::new())));
    }

    #[test]
    fn test_section_url_segment() {
        assert_eq!(Section::Genres.url_segment(), "genres");
        assert_eq!(Section::VolumeStates.url_segment(), "volume-states");
        assert_eq!(Section::ContributorRoles.url_segment(), "contributor-roles");
        assert_eq!(Section::NodeTypes.url_segment(), "node-types");
    }

    #[test]
    fn test_in_use_conflict_returns_localized_conflict() {
        match in_use_conflict("en", Section::Genres, 3) {
            AppError::Conflict(msg) => {
                assert!(msg.contains("3"));
                assert!(msg.to_lowercase().contains("cannot delete"));
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
    }
}
