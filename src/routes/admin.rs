//! Admin page shell + Health tab (story 8-1) + Users panel (story 8-3).
//!
//! One entry point (`GET /admin`) with five tabs. Health and Users are complete;
//! the other three are stubs that later Epic 8 stories fill in exactly one at
//! a time:
//!   - Users         → story 8-3 ✓
//!   - Reference     → story 8-4
//!   - System        → story 8-5
//!   - Trash (view)  → story 8-6
//!   - Trash (purge) → story 8-7
//!
//! Middleware order follows AR16 — admin routes live at the top level
//! alongside the non-catalog routes so they skip `pending_updates_middleware`
//! (catalog-only). Each handler's first line is `require_role(Role::Admin)?`.

use std::sync::Arc;

use askama::Template;
use axum::Extension;
use axum::extract::{Form, OriginalUri, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::metadata::registry::ProviderRegistry;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest};
use crate::middleware::locale::Locale;
use crate::models::user::UserModel;
use crate::routes::catalog::feedback_html_pub;
use crate::services::admin_health;
use crate::services::password;
use crate::tasks::provider_health::{ProviderHealthMap, ProviderStatus};
use crate::utils::current_url;

// ─── Tab resolution ─────────────────────────────────────────────

/// One of the five admin tabs. Invalid / missing query values fall back to
/// `Health` — the landing tab — so a deep link to `/admin?tab=../../etc/passwd`
/// cannot throw an error and cannot escape the allowed-values set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminTab {
    Health,
    Users,
    ReferenceData,
    Trash,
    System,
}

impl AdminTab {
    /// Resolve a query-string tab name. Empty / unknown → Health.
    pub fn from_query_str(s: Option<&str>) -> Self {
        match s.unwrap_or("") {
            "health" => AdminTab::Health,
            "users" => AdminTab::Users,
            "reference_data" => AdminTab::ReferenceData,
            "trash" => AdminTab::Trash,
            "system" => AdminTab::System,
            _ => AdminTab::Health,
        }
    }

    /// Template-facing snake_case name (matches i18n keys + panel ids).
    pub fn as_str(&self) -> &'static str {
        match self {
            AdminTab::Health => "health",
            AdminTab::Users => "users",
            AdminTab::ReferenceData => "reference_data",
            AdminTab::Trash => "trash",
            AdminTab::System => "system",
        }
    }

    /// URL-facing path segment. Reference data uses a hyphen to keep URLs
    /// idiomatic while the i18n key keeps an underscore.
    pub fn hx_path(&self) -> &'static str {
        match self {
            AdminTab::Health => "health",
            AdminTab::Users => "users",
            AdminTab::ReferenceData => "reference-data",
            AdminTab::Trash => "trash",
            AdminTab::System => "system",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AdminQuery {
    pub tab: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UsersQuery {
    pub role: Option<String>,
    pub status: Option<String>,
    pub page: Option<u32>,
}

#[derive(Deserialize)]
pub struct CreateUserForm {
    pub username: String,
    pub password: String,
    pub role: String,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct UpdateUserForm {
    pub username: String,
    pub role: String,
    pub password: String,
    pub version: i32,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct DeactivateForm {
    pub version: i32,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct ReactivateForm {
    pub version: i32,
    pub _csrf_token: String,
}

// ─── Templates ──────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/admin.html")]
struct AdminPageTemplate {
    // Base-layout context (matches the other page templates).
    lang: String,
    role: String,
    current_page: &'static str,
    skip_label: String,
    session_timeout_secs: u64,
    csrf_token: String,
    nav_catalog: String,
    nav_loans: String,
    nav_locations: String,
    nav_series: String,
    nav_borrowers: String,
    nav_admin: String,
    nav_login: String,
    nav_logout: String,
    current_url: String,
    lang_toggle_aria: String,

    // Page-specific.
    admin_page_title: String,
    shell_html: String,
}

#[derive(Template)]
#[template(path = "components/admin_tabs.html")]
struct AdminShellTemplate {
    tabs: Vec<AdminTabItem>,
    tabs_aria: String,
    active_tab_name: &'static str,
    panel_html: String,
}

struct AdminTabItem {
    name: &'static str,
    hx_path: &'static str,
    label: String,
    aria_selected: bool,
    /// 0 means "no badge" — the template hides it.
    badge_count: i64,
    badge_aria: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_health_panel.html")]
struct AdminHealthPanel {
    versions_heading: String,
    app_version_label: String,
    app_version: &'static str,
    db_version_label: String,
    db_version: String,
    disk_usage_label: String,
    disk_usage_value: String,
    counts_heading: String,
    count_titles_label: String,
    count_titles: i64,
    count_volumes_label: String,
    count_volumes: i64,
    count_contributors_label: String,
    count_contributors: i64,
    count_borrowers_label: String,
    count_borrowers: i64,
    count_active_loans_label: String,
    count_active_loans: i64,
    providers_heading: String,
    providers: Vec<ProviderHealthRow>,
}

struct ProviderHealthRow {
    name: String,
    status_label: String,
    status_class: &'static str,
    last_checked_label: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_users_panel.html")]
struct AdminUsersPanel {
    lang: String,
    csrf_token: String,
    heading: String,
    pagination_aria: String,
    empty_state: String,
    filter_role_label: String,
    filter_status_label: String,
    filter_role_all: String,
    filter_status_active: String,
    filter_status_deactivated: String,
    filter_status_all: String,
    col_username: String,
    col_role: String,
    col_status: String,
    col_created: String,
    col_last_login: String,
    col_actions: String,
    role_librarian: String,
    role_admin: String,
    status_active: String,
    status_deactivated: String,
    last_login_never: String,
    btn_new: String,
    btn_edit: String,
    btn_deactivate: String,
    btn_reactivate: String,
    users: Vec<crate::models::user::UserRow>,
    filter_role: Option<String>,
    filter_status: String,
    page: u32,
    total_pages: u32,
    total_items: u64,
    acting_admin_id: u64,
    confirm_deactivate: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_users_form_create.html")]
struct AdminUsersFormCreate {
    csrf_token: String,
    form_label_username: String,
    form_label_password: String,
    form_label_role: String,
    role_librarian: String,
    role_admin: String,
    btn_cancel: String,
    btn_save: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_users_row.html")]
struct AdminUsersRow {
    user: crate::models::user::UserRow,
    csrf_token: String,
    role_admin: String,
    role_librarian: String,
    status_active: String,
    status_deactivated: String,
    last_login_never: String,
    btn_edit: String,
    btn_deactivate: String,
    btn_reactivate: String,
    confirm_deactivate: String,
    acting_admin_id: u64,
}

#[derive(Template)]
#[template(path = "fragments/admin_users_form_edit.html")]
struct AdminUsersFormEdit {
    user: crate::models::user::UserRow,
    csrf_token: String,
    form_label_username: String,
    form_label_password_edit: String,
    form_label_role: String,
    role_librarian: String,
    role_admin: String,
    btn_cancel: String,
    btn_save: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_reference_data_panel.html")]
struct AdminReferenceDataPanel {
    stub_message: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_trash_panel.html")]
struct AdminTrashPanel {
    stub_message: String,
    preview_message: String,
}

#[derive(Template)]
#[template(path = "fragments/admin_system_panel.html")]
struct AdminSystemPanel {
    stub_message: String,
}

// ─── Handlers ───────────────────────────────────────────────────

/// `GET /admin` — full page (direct nav) or shell fragment (HTMX).
pub async fn admin_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
    Query(params): Query<AdminQuery>,
) -> Result<Response, AppError> {
    // Preserve the full `?tab=<name>` deep link on Anonymous → /login?next=<encoded>.
    // `OriginalUri` includes the query string (e.g. "/admin?tab=trash") so a
    // post-login bounce lands back on the tab the user originally asked for.
    let return_path = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/admin".to_string());
    session.require_role_with_return(Role::Admin, &return_path)?;

    let tab = AdminTab::from_query_str(params.tab.as_deref());
    render_admin(&state, &session, locale.0, &uri, is_htmx, tab, None).await
}

pub async fn admin_health_panel(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=health")?;
    render_admin(&state, &session, locale.0, &uri, is_htmx, AdminTab::Health, None).await
}

pub async fn admin_users_panel(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
    Query(query): Query<UsersQuery>,
) -> Result<Response, AppError> {
    let return_path = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/admin?tab=users".to_string());
    session.require_role_with_return(Role::Admin, &return_path)?;

    let tab = AdminTab::Users;
    // Extract page from query (1-based, clamp to 1 if 0 or missing)
    let page = query.page.unwrap_or(1).max(1);
    render_admin(&state, &session, locale.0, &uri, is_htmx, tab, Some(page)).await
}

pub async fn admin_users_create_form(
    session: Session,
    Extension(locale): Extension<Locale>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users")?;
    let loc = locale.0;

    let form = AdminUsersFormCreate {
        csrf_token: session.csrf_token.clone(),
        form_label_username: rust_i18n::t!("admin.users.form_label_username", locale = loc)
            .to_string(),
        form_label_password: rust_i18n::t!("admin.users.form_label_password", locale = loc)
            .to_string(),
        form_label_role: rust_i18n::t!("admin.users.form_label_role", locale = loc).to_string(),
        role_librarian: rust_i18n::t!("admin.users.role_librarian", locale = loc).to_string(),
        role_admin: rust_i18n::t!("admin.users.role_admin", locale = loc).to_string(),
        btn_cancel: rust_i18n::t!("admin.users.btn_cancel", locale = loc).to_string(),
        btn_save: rust_i18n::t!("admin.users.btn_save", locale = loc).to_string(),
    };

    let html = form
        .render()
        .map_err(|_| AppError::Internal("admin users create form render failed".to_string()))?;
    Ok(Html(html))
}

pub async fn admin_users_create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<CreateUserForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users")?;
    let loc = locale.0;

    // Validate username (trim whitespace, check not empty)
    let username = form.username.trim().to_string();
    if username.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.user.username_empty", locale = loc).to_string(),
        ));
    }

    // Validate password length (8-72 chars)
    if form.password.len() < 8 {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.user.password_too_short", locale = loc).to_string(),
        ));
    }
    if form.password.len() > 72 {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.user.password_too_long", locale = loc).to_string(),
        ));
    }

    // Validate role
    if form.role != "admin" && form.role != "librarian" {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.user.role_invalid", locale = loc).to_string(),
        ));
    }

    // Hash password
    let password_hash = password::hash_password(&form.password)?;

    // Create user
    UserModel::create(&state.pool, &username, &password_hash, &form.role).await?;

    // Render feedback and updated users list
    let success_msg = rust_i18n::t!("admin.users.success_created", locale = loc, username = &username)
        .to_string();
    let feedback = feedback_html_pub("success", &success_msg, "");

    // Fetch fresh users list for the panel (page 1)
    let users_panel_html = render_users_panel(&state, loc, &session, None).await?;

    Ok(HtmxResponse {
        main: format!("{}{}", feedback, users_panel_html),
        oob: vec![],
    })
}

pub async fn admin_users_edit_form(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users")?;
    let loc = locale.0;

    // Fetch user
    let user = UserModel::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("User not found".to_string()))?;

    let form = AdminUsersFormEdit {
        user,
        csrf_token: session.csrf_token.clone(),
        form_label_username: rust_i18n::t!("admin.users.form_label_username", locale = loc).to_string(),
        form_label_password_edit: rust_i18n::t!("admin.users.form_label_password_edit", locale = loc).to_string(),
        form_label_role: rust_i18n::t!("admin.users.form_label_role", locale = loc).to_string(),
        role_librarian: rust_i18n::t!("admin.users.role_librarian", locale = loc).to_string(),
        role_admin: rust_i18n::t!("admin.users.role_admin", locale = loc).to_string(),
        btn_cancel: rust_i18n::t!("admin.users.btn_cancel", locale = loc).to_string(),
        btn_save: rust_i18n::t!("admin.users.btn_save", locale = loc).to_string(),
    };

    let html = form
        .render()
        .map_err(|_| AppError::Internal("admin users edit form render failed".to_string()))?;
    Ok(Html(html))
}

pub async fn admin_users_row_view(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users")?;
    let loc = locale.0;

    // Fetch user
    let user = UserModel::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("User not found".to_string()))?;

    let html = render_user_row(&state, loc, &session, &user).await?;
    Ok(Html(html))
}

pub async fn admin_users_update(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::Path(id): axum::extract::Path<u64>,
    Form(form): Form<UpdateUserForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users")?;
    let loc = locale.0;

    // Validate username (trim whitespace, check not empty)
    let username = form.username.trim().to_string();
    if username.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.user.username_empty", locale = loc).to_string(),
        ));
    }

    // Validate role
    if form.role != "admin" && form.role != "librarian" {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.user.role_invalid", locale = loc).to_string(),
        ));
    }

    // Validate and hash password (optional)
    let password_hash = if form.password.is_empty() {
        None
    } else {
        if form.password.len() < 8 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.user.password_too_short", locale = loc).to_string(),
            ));
        }
        if form.password.len() > 72 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.user.password_too_long", locale = loc).to_string(),
            ));
        }
        Some(password::hash_password(&form.password)?)
    };

    // Check last-admin demote guard if role is changing
    let acting_admin_id = session.user_id.ok_or_else(|| {
        AppError::Internal("admin session missing user_id".to_string())
    })?;
    if form.role != "admin" {
        // Only check demote guard when changing TO a non-admin role
        if let Err(e) = UserModel::demote_guard(&state.pool, id, &form.role, acting_admin_id).await {
            return match e {
                AppError::Conflict(ref msg) if msg == "last_admin_demote_blocked" => {
                    Err(AppError::BadRequest(
                        rust_i18n::t!("error.user.last_admin_demote", locale = loc).to_string(),
                    ))
                }
                _ => Err(e),
            };
        }
    }

    // Update user
    if let Err(e) = UserModel::update(
        &state.pool,
        id,
        form.version,
        &username,
        &form.role,
        password_hash.as_deref(),
    )
    .await
    {
        return match e {
            AppError::Conflict(ref msg) if msg.contains("username_taken") => {
                Err(AppError::BadRequest(
                    rust_i18n::t!("error.user.username_taken", locale = loc, username = &username).to_string(),
                ))
            }
            _ => Err(e),
        };
    }

    // Fetch updated user and render row
    let user = UserModel::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("User not found".to_string()))?;

    let success_msg = rust_i18n::t!("admin.users.success_updated", locale = loc, username = &username)
        .to_string();
    let feedback = feedback_html_pub("success", &success_msg, "");

    let row_html = render_user_row(&state, loc, &session, &user).await?;

    Ok(HtmxResponse {
        main: format!("{}{}", feedback, row_html),
        oob: vec![],
    })
}

pub async fn admin_users_deactivate(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::Path(id): axum::extract::Path<u64>,
    Form(form): Form<DeactivateForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users")?;
    let loc = locale.0;
    let acting_admin_id = session.user_id.ok_or_else(|| {
        AppError::Internal("admin session missing user_id".to_string())
    })?;

    // Deactivate the user (guards handled by UserModel::deactivate)
    UserModel::deactivate(&state.pool, id, form.version, acting_admin_id).await?;

    // Fetch updated user and render row
    let user = UserModel::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("User not found".to_string()))?;

    let success_msg = rust_i18n::t!("admin.users.success_deactivated", locale = loc, username = &user.username, count = 1)
        .to_string();
    let feedback = feedback_html_pub("success", &success_msg, "");

    let row_html = render_user_row(&state, loc, &session, &user).await?;
    Ok(HtmxResponse {
        main: format!("{}{}", feedback, row_html),
        oob: vec![],
    })
}

pub async fn admin_users_reactivate(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::Path(id): axum::extract::Path<u64>,
    Form(form): Form<ReactivateForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users")?;
    let loc = locale.0;

    // Reactivate the user
    UserModel::reactivate(&state.pool, id, form.version).await?;

    // Fetch updated user and render row
    let user = UserModel::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("User not found".to_string()))?;

    let success_msg = rust_i18n::t!("admin.users.success_reactivated", locale = loc, username = &user.username)
        .to_string();
    let feedback = feedback_html_pub("success", &success_msg, "");

    let row_html = render_user_row(&state, loc, &session, &user).await?;
    Ok(HtmxResponse {
        main: format!("{}{}", feedback, row_html),
        oob: vec![],
    })
}

pub async fn admin_reference_data_panel(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=reference_data")?;
    render_admin(&state, &session, locale.0, &uri, is_htmx, AdminTab::ReferenceData, None).await
}

pub async fn admin_trash_panel(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=trash")?;
    render_admin(&state, &session, locale.0, &uri, is_htmx, AdminTab::Trash, None).await
}

pub async fn admin_system_panel(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=system")?;
    render_admin(&state, &session, locale.0, &uri, is_htmx, AdminTab::System, None).await
}

// ─── Rendering ──────────────────────────────────────────────────

async fn render_admin(
    state: &AppState,
    session: &Session,
    loc: &'static str,
    uri: &axum::http::Uri,
    is_htmx: bool,
    tab: AdminTab,
    page: Option<u32>,
) -> Result<Response, AppError> {
    let pool = &state.pool;

    // Trash badge — always computed so stub panels still show the Trash
    // tab's current count without requiring a round-trip to the real panel.
    let trash_count = admin_health::trash_count(pool).await?;

    let panel_html = render_panel(state, loc, tab, session, page).await?;
    let tabs_html = render_shell(loc, tab, trash_count, panel_html)?;

    if is_htmx {
        return Ok((StatusCode::OK, Html(tabs_html)).into_response());
    }

    let page = AdminPageTemplate {
        lang: loc.to_string(),
        role: session.role.to_string(),
        current_page: "admin",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        session_timeout_secs: state.session_timeout_secs(),
        csrf_token: session.csrf_token.clone(),
        nav_catalog: rust_i18n::t!("nav.catalog", locale = loc).to_string(),
        nav_loans: rust_i18n::t!("nav.loans", locale = loc).to_string(),
        nav_locations: rust_i18n::t!("nav.locations", locale = loc).to_string(),
        nav_series: rust_i18n::t!("nav.series", locale = loc).to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers", locale = loc).to_string(),
        nav_admin: rust_i18n::t!("nav.admin", locale = loc).to_string(),
        nav_login: rust_i18n::t!("nav.login", locale = loc).to_string(),
        nav_logout: rust_i18n::t!("nav.logout", locale = loc).to_string(),
        current_url: current_url(uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = loc).to_string(),
        admin_page_title: rust_i18n::t!("admin.page_title", locale = loc).to_string(),
        shell_html: tabs_html,
    };
    page.render()
        .map(|html| Html(html).into_response())
        .map_err(|_| AppError::Internal("admin page render failed".to_string()))
}

fn render_shell(
    loc: &'static str,
    active: AdminTab,
    trash_count: i64,
    panel_html: String,
) -> Result<String, AppError> {
    let badge_aria = rust_i18n::t!(
        "admin.trash.badge_aria",
        locale = loc,
        count = trash_count
    )
    .to_string();

    let mk = |tab: AdminTab| AdminTabItem {
        name: tab.as_str(),
        hx_path: tab.hx_path(),
        label: match tab {
            AdminTab::Health => rust_i18n::t!("admin.tabs.health", locale = loc).to_string(),
            AdminTab::Users => rust_i18n::t!("admin.tabs.users", locale = loc).to_string(),
            AdminTab::ReferenceData => {
                rust_i18n::t!("admin.tabs.reference_data", locale = loc).to_string()
            }
            AdminTab::Trash => rust_i18n::t!("admin.tabs.trash", locale = loc).to_string(),
            AdminTab::System => rust_i18n::t!("admin.tabs.system", locale = loc).to_string(),
        },
        aria_selected: tab == active,
        badge_count: if tab == AdminTab::Trash {
            trash_count
        } else {
            0
        },
        badge_aria: if tab == AdminTab::Trash {
            badge_aria.clone()
        } else {
            String::new()
        },
    };

    let shell = AdminShellTemplate {
        tabs: vec![
            mk(AdminTab::Health),
            mk(AdminTab::Users),
            mk(AdminTab::ReferenceData),
            mk(AdminTab::Trash),
            mk(AdminTab::System),
        ],
        tabs_aria: rust_i18n::t!("admin.tabs_aria", locale = loc).to_string(),
        active_tab_name: active.as_str(),
        panel_html,
    };
    shell
        .render()
        .map_err(|_| AppError::Internal("admin shell render failed".to_string()))
}

async fn render_users_panel(
    state: &AppState,
    loc: &'static str,
    session: &Session,
    page: Option<u32>,
) -> Result<String, AppError> {
    let pool = &state.pool;
    let current_page = page.unwrap_or(1).max(1);

    let users = crate::models::user::UserModel::list_page(
        pool,
        None,
        crate::models::user::UserStatus::Active,
        (current_page - 1) * 25,
        25,
    )
    .await?;

    let total = crate::models::user::UserModel::count_all(
        pool,
        None,
        crate::models::user::UserStatus::Active,
    )
    .await?;

    let total_pages = if total == 0 { 1 } else { ((total as f64) / 25.0).ceil() as u32 };

    let empty_state = if users.is_empty() && total == 0 {
        rust_i18n::t!("admin.users.empty_state", locale = loc).to_string()
    } else {
        String::new()
    };

    let panel = AdminUsersPanel {
        lang: loc.to_string(),
        csrf_token: session.csrf_token.clone(),
        heading: rust_i18n::t!("admin.users.heading", locale = loc).to_string(),
        pagination_aria: rust_i18n::t!("admin.users.pagination_aria", locale = loc).to_string(),
        empty_state,
        filter_role_label: rust_i18n::t!("admin.users.filter_role_label", locale = loc).to_string(),
        filter_status_label: rust_i18n::t!("admin.users.filter_status_label", locale = loc).to_string(),
        filter_role_all: rust_i18n::t!("admin.users.filter_role_all", locale = loc).to_string(),
        filter_status_active: rust_i18n::t!("admin.users.filter_status_active", locale = loc).to_string(),
        filter_status_deactivated: rust_i18n::t!("admin.users.filter_status_deactivated", locale = loc).to_string(),
        filter_status_all: rust_i18n::t!("admin.users.filter_status_all", locale = loc).to_string(),
        col_username: rust_i18n::t!("admin.users.col_username", locale = loc).to_string(),
        col_role: rust_i18n::t!("admin.users.col_role", locale = loc).to_string(),
        col_status: rust_i18n::t!("admin.users.col_status", locale = loc).to_string(),
        col_created: rust_i18n::t!("admin.users.col_created", locale = loc).to_string(),
        col_last_login: rust_i18n::t!("admin.users.col_last_login", locale = loc).to_string(),
        col_actions: rust_i18n::t!("admin.users.col_actions", locale = loc).to_string(),
        role_librarian: rust_i18n::t!("admin.users.role_librarian", locale = loc).to_string(),
        role_admin: rust_i18n::t!("admin.users.role_admin", locale = loc).to_string(),
        status_active: rust_i18n::t!("admin.users.status_active", locale = loc).to_string(),
        status_deactivated: rust_i18n::t!("admin.users.status_deactivated", locale = loc).to_string(),
        last_login_never: rust_i18n::t!("admin.users.last_login_never", locale = loc).to_string(),
        btn_new: rust_i18n::t!("admin.users.btn_new", locale = loc).to_string(),
        btn_edit: rust_i18n::t!("admin.users.btn_edit", locale = loc).to_string(),
        btn_deactivate: rust_i18n::t!("admin.users.btn_deactivate", locale = loc).to_string(),
        btn_reactivate: rust_i18n::t!("admin.users.btn_reactivate", locale = loc).to_string(),
        users,
        filter_role: None,
        filter_status: "active".to_string(),
        page: current_page,
        total_pages,
        total_items: total as u64,
        acting_admin_id: session.user_id.unwrap_or(0),
        confirm_deactivate: rust_i18n::t!(
            "admin.users.confirm_deactivate",
            locale = loc,
            username = ""
        )
        .to_string(),
    };

    panel
        .render()
        .map_err(|_| AppError::Internal("admin users panel render failed".to_string()))
}

async fn render_user_row(
    _state: &AppState,
    loc: &'static str,
    session: &Session,
    user: &crate::models::user::UserRow,
) -> Result<String, AppError> {
    let row = AdminUsersRow {
        user: user.clone(),
        csrf_token: session.csrf_token.clone(),
        role_admin: rust_i18n::t!("admin.users.role_admin", locale = loc).to_string(),
        role_librarian: rust_i18n::t!("admin.users.role_librarian", locale = loc).to_string(),
        status_active: rust_i18n::t!("admin.users.status_active", locale = loc).to_string(),
        status_deactivated: rust_i18n::t!("admin.users.status_deactivated", locale = loc).to_string(),
        last_login_never: rust_i18n::t!("admin.users.last_login_never", locale = loc).to_string(),
        btn_edit: rust_i18n::t!("admin.users.btn_edit", locale = loc).to_string(),
        btn_deactivate: rust_i18n::t!("admin.users.btn_deactivate", locale = loc).to_string(),
        btn_reactivate: rust_i18n::t!("admin.users.btn_reactivate", locale = loc).to_string(),
        confirm_deactivate: rust_i18n::t!("admin.users.confirm_deactivate", locale = loc, username = &user.username)
            .to_string(),
        acting_admin_id: session.user_id.unwrap_or(0),
    };

    row.render()
        .map_err(|_| AppError::Internal("admin user row render failed".to_string()))
}

async fn render_panel(
    state: &AppState,
    loc: &'static str,
    tab: AdminTab,
    session: &Session,
    page: Option<u32>,
) -> Result<String, AppError> {
    match tab {
        AdminTab::Health => render_health_panel(state, loc).await,
        AdminTab::Users => render_users_panel(state, loc, session, page).await,
        AdminTab::ReferenceData => AdminReferenceDataPanel {
            stub_message: rust_i18n::t!(
                "admin.placeholder.coming_in_story",
                locale = loc,
                story = "8-3"
            )
            .to_string(),
        }
        .render()
        .map_err(|_| AppError::Internal("admin reference-data panel render failed".to_string())),
        AdminTab::Trash => AdminTrashPanel {
            stub_message: rust_i18n::t!(
                "admin.placeholder.coming_in_story",
                locale = loc,
                story = "8-5"
            )
            .to_string(),
            preview_message: rust_i18n::t!("admin.placeholder.trash_preview", locale = loc)
                .to_string(),
        }
        .render()
        .map_err(|_| AppError::Internal("admin trash panel render failed".to_string())),
        AdminTab::System => AdminSystemPanel {
            stub_message: rust_i18n::t!(
                "admin.placeholder.coming_in_story",
                locale = loc,
                story = "8-4"
            )
            .to_string(),
        }
        .render()
        .map_err(|_| AppError::Internal("admin system panel render failed".to_string())),
    }
}

async fn render_health_panel(state: &AppState, loc: &'static str) -> Result<String, AppError> {
    let pool = &state.pool;

    let counts = admin_health::entity_counts(pool).await?;
    let db_version = admin_health::mariadb_version(pool, &state.mariadb_version_cache).await;

    let disk_usage_value = match admin_health::format_disk_usage(admin_health::disk_usage(
        &state.covers_dir,
    )) {
        Some((used, total, pct)) => rust_i18n::t!(
            "admin.health.disk_usage_format",
            locale = loc,
            used = used,
            total = total,
            pct = pct
        )
        .to_string(),
        None => rust_i18n::t!("admin.health.disk_usage_unknown", locale = loc).to_string(),
    };

    let providers = build_provider_rows(&state.registry, &state.provider_health, loc);

    let panel = AdminHealthPanel {
        versions_heading: rust_i18n::t!("admin.health.versions_heading", locale = loc)
            .to_string(),
        app_version_label: rust_i18n::t!("admin.health.app_version", locale = loc).to_string(),
        app_version: env!("CARGO_PKG_VERSION"),
        db_version_label: rust_i18n::t!("admin.health.db_version", locale = loc).to_string(),
        db_version,
        disk_usage_label: rust_i18n::t!("admin.health.disk_usage", locale = loc).to_string(),
        disk_usage_value,
        counts_heading: rust_i18n::t!("admin.health.counts_heading", locale = loc).to_string(),
        count_titles_label: rust_i18n::t!("admin.health.count_titles", locale = loc).to_string(),
        count_titles: counts.titles,
        count_volumes_label: rust_i18n::t!("admin.health.count_volumes", locale = loc).to_string(),
        count_volumes: counts.volumes,
        count_contributors_label: rust_i18n::t!("admin.health.count_contributors", locale = loc)
            .to_string(),
        count_contributors: counts.contributors,
        count_borrowers_label: rust_i18n::t!("admin.health.count_borrowers", locale = loc)
            .to_string(),
        count_borrowers: counts.borrowers,
        count_active_loans_label: rust_i18n::t!("admin.health.count_active_loans", locale = loc)
            .to_string(),
        count_active_loans: counts.active_loans,
        providers_heading: rust_i18n::t!("admin.health.providers_heading", locale = loc)
            .to_string(),
        providers,
    };

    panel
        .render()
        .map_err(|_| AppError::Internal("admin health panel render failed".to_string()))
}

fn build_provider_rows(
    registry: &Arc<ProviderRegistry>,
    map: &ProviderHealthMap,
    loc: &'static str,
) -> Vec<ProviderHealthRow> {
    let guard = map.read().ok();
    registry
        .iter()
        .map(|p| {
            let name = p.name().to_string();
            let (status_key, status_class, last_checked_label) = match guard.as_ref() {
                Some(g) => match g.get(&name) {
                    Some(h) => {
                        let (key, class) = match h.status {
                            ProviderStatus::Reachable => {
                                ("admin.health.provider_status_up", "bg-emerald-500")
                            }
                            ProviderStatus::Unreachable => {
                                ("admin.health.provider_status_down", "bg-red-500")
                            }
                            ProviderStatus::NotApplicable => {
                                ("admin.health.provider_status_na", "bg-stone-300")
                            }
                            ProviderStatus::Unknown => {
                                ("admin.health.provider_status_unknown", "bg-stone-300")
                            }
                        };
                        let label = match h.last_checked {
                            Some(ts) => rust_i18n::t!(
                                "admin.health.last_checked",
                                locale = loc,
                                when = ts.format("%Y-%m-%d %H:%M UTC").to_string()
                            )
                            .to_string(),
                            None => rust_i18n::t!(
                                "admin.health.last_checked_never",
                                locale = loc
                            )
                            .to_string(),
                        };
                        (key, class, label)
                    }
                    None => (
                        "admin.health.provider_status_unknown",
                        "bg-stone-300",
                        rust_i18n::t!("admin.health.last_checked_never", locale = loc).to_string(),
                    ),
                },
                None => (
                    "admin.health.provider_status_unknown",
                    "bg-stone-300",
                    rust_i18n::t!("admin.health.last_checked_never", locale = loc).to_string(),
                ),
            };
            ProviderHealthRow {
                name,
                status_label: rust_i18n::t!(status_key, locale = loc).to_string(),
                status_class,
                last_checked_label,
            }
        })
        .collect()
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::auth::{Role, Session};

    // ─── Tab resolution ─────────────────────────────────────

    #[test]
    fn test_tab_resolution_valid_names() {
        assert_eq!(
            AdminTab::from_query_str(Some("health")),
            AdminTab::Health
        );
        assert_eq!(
            AdminTab::from_query_str(Some("users")),
            AdminTab::Users
        );
        assert_eq!(
            AdminTab::from_query_str(Some("reference_data")),
            AdminTab::ReferenceData
        );
        assert_eq!(
            AdminTab::from_query_str(Some("trash")),
            AdminTab::Trash
        );
        assert_eq!(
            AdminTab::from_query_str(Some("system")),
            AdminTab::System
        );
    }

    #[test]
    fn test_tab_resolution_invalid_falls_back_to_health() {
        // Path-traversal attempt must not escape the enum.
        assert_eq!(
            AdminTab::from_query_str(Some("../../etc/passwd")),
            AdminTab::Health
        );
        // Unknown value.
        assert_eq!(
            AdminTab::from_query_str(Some("bogus")),
            AdminTab::Health
        );
        // Case sensitivity — exact match only.
        assert_eq!(
            AdminTab::from_query_str(Some("Health")),
            AdminTab::Health,
            "capitalized input falls through to default (Health)"
        );
    }

    #[test]
    fn test_tab_resolution_missing_falls_back_to_health() {
        assert_eq!(AdminTab::from_query_str(None), AdminTab::Health);
        assert_eq!(AdminTab::from_query_str(Some("")), AdminTab::Health);
    }

    #[test]
    fn test_tab_as_str_and_hx_path_match_url_conventions() {
        // i18n keys use snake_case (`reference_data`); URL paths use
        // hyphens (`reference-data`). Pinning both prevents a silent drift.
        assert_eq!(AdminTab::Health.as_str(), "health");
        assert_eq!(AdminTab::Health.hx_path(), "health");
        assert_eq!(AdminTab::ReferenceData.as_str(), "reference_data");
        assert_eq!(AdminTab::ReferenceData.hx_path(), "reference-data");
        assert_eq!(AdminTab::Trash.as_str(), "trash");
        assert_eq!(AdminTab::Trash.hx_path(), "trash");
    }

    // ─── Role gating — (role, path) → AppError pin ──────────
    //
    // These tests are *intentionally* limited to pinning the mapping between
    // `(role, /admin*)` and the `AppError` variant the handler's guard returns.
    // They do NOT invoke the `admin_page` / `admin_*_panel` handler functions
    // directly — the full request flow is exercised end-to-end by
    // `tests/role_gating.rs::anonymous_admin_redirects_to_login_with_next`,
    // `tests/role_gating.rs::librarian_admin_returns_403_forbidden`, and by
    // `tests/e2e/specs/journeys/admin-smoke.spec.ts`. Keeping this split means
    // the unit tests stay pool-free (fast; no `#[sqlx::test]`) while the
    // integration layer catches any drift in the handler's first-line guard.

    fn make_session(role: Role) -> Session {
        if role == Role::Anonymous {
            Session::anonymous_with_token(String::new())
        } else {
            Session {
                token: Some("t".to_string()),
                user_id: Some(1),
                role,
                csrf_token: String::new(),
                preferred_language: None,
            }
        }
    }

    #[test]
    fn test_admin_handler_requires_admin_role_for_librarian() {
        match make_session(Role::Librarian).require_role_with_return(Role::Admin, "/admin") {
            Err(AppError::Forbidden) => {}
            other => panic!("librarian on /admin: expected Forbidden, got {other:?}"),
        }
    }

    #[test]
    fn test_admin_handler_anonymous_returns_unauthorized_with_return() {
        match make_session(Role::Anonymous).require_role_with_return(Role::Admin, "/admin") {
            Err(AppError::UnauthorizedWithReturn(next)) => {
                assert_eq!(next, "/admin");
            }
            other => panic!("anonymous on /admin: expected UnauthorizedWithReturn, got {other:?}"),
        }
    }

    #[test]
    fn test_admin_handler_admin_role_passes() {
        assert!(
            make_session(Role::Admin)
                .require_role_with_return(Role::Admin, "/admin")
                .is_ok()
        );
    }
}
