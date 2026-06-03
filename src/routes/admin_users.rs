//! Admin → Users management (story 8-3; extracted in #391 per Foundation Rule #12).
//!
//! Houses the user-management handlers (list / create / edit / update /
//! deactivate / reactivate + the deactivate-confirmation modal) plus the
//! panel + row renderers the admin shell calls when the active tab is
//! `AdminTab::Users`.
//!
//! Split out of `routes/admin.rs` because Foundation Rule #12 caps source
//! files at 2000 lines and `admin.rs` had drifted past it. Mirrors the
//! `admin_reference_data.rs` extraction (story 8-4): the shared admin shell
//! / dispatch (`render_admin`, `render_panel`, `render_shell`, `AdminTab`)
//! stays in `admin.rs` — this module owns only the Users surface. Every POST
//! handler is CSRF-protected (8-2 middleware) and every handler's first line
//! is `session.require_role_with_return(Role::Admin, …, locale.0)?`.

use askama::Template;
use axum::Extension;
use axum::extract::{Form, OriginalUri, Query, State};
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::user::UserModel;
use crate::routes::admin::{AdminTab, render_admin};
use crate::services::password;
use crate::utils::feedback_html;

// ─── Query / form structs ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UsersQuery {
    pub role: Option<String>,
    pub status: Option<String>,
    pub page: Option<u32>,
}

/// Dispatch-coupling glue: threaded through `admin::render_admin` /
/// `admin::render_panel` (both stay in `admin.rs`) down to
/// `render_users_panel`. `pub(crate)` so `admin.rs` can name it in those
/// signatures and read `.page` to feed the shared pagination arg.
#[derive(Debug, Clone)]
pub(crate) struct UsersFilters {
    pub(crate) role: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) page: Option<u32>,
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
#[template(path = "fragments/admin_users_panel.html")]
struct AdminUsersPanel {
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
    filter_role: String,
    filter_status: String,
    page: u32,
    total_pages: u32,
    acting_admin_id: u64,
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

/// Story 9-14 — admin user deactivate confirmation modal.
/// Final migration in the hx-confirm → UX-DR8 Modal chain (Epic 9).
#[derive(Template)]
#[template(path = "fragments/admin_user_deactivate_modal.html")]
struct AdminUserDeactivateModalTemplate {
    title: String,
    body_html: String,
    confirm_label: String,
    cancel_label: String,
    action_url: String,
    csrf_token: String,
    hx_target: String,
    version: i32,
}

// ─── Handlers ───────────────────────────────────────────────────

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
    session.require_role_with_return(Role::Admin, &return_path, locale.0)?;

    let tab = AdminTab::Users;
    let filters = UsersFilters {
        role: query.role,
        status: query.status,
        page: query.page,
    };
    render_admin(
        &state,
        &session,
        locale.0,
        &uri,
        is_htmx,
        tab,
        Some(filters),
    )
    .await
}

pub async fn admin_users_create_form(
    session: Session,
    Extension(locale): Extension<Locale>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users", locale.0)?;
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

/// Shared username validation for the admin create + update handlers (#55
/// MEDIUM #2). Bounds are in characters: empty → `username_empty`, `<3` →
/// `username_too_short`, `>255` → `username_too_long`. The upper bound matches
/// the `users.username VARCHAR(255)` column, so an over-long value surfaces as
/// a clean localized 400 instead of a MariaDB truncation error (500).
fn validate_username(username: &str, loc: &'static str) -> Result<(), AppError> {
    if username.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.user.username_empty", locale = loc).to_string(),
        ));
    }
    let char_count = username.chars().count();
    if char_count < 3 {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.user.username_too_short", locale = loc).to_string(),
        ));
    }
    if char_count > 255 {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.user.username_too_long", locale = loc).to_string(),
        ));
    }
    Ok(())
}

pub async fn admin_users_create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Form(form): Form<CreateUserForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users", locale.0)?;
    let loc = locale.0;

    // Validate username (trim whitespace; empty / length bounds — #55 MEDIUM #2)
    let username = form.username.trim().to_string();
    validate_username(&username, loc)?;

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
    let feedback = feedback_html("success", &success_msg, "");

    // Fetch fresh users list for the panel (page 1)
    let users_panel_html = render_users_panel(&state, loc, &session, None, None).await?;

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
    session.require_role_with_return(Role::Admin, "/admin?tab=users", locale.0)?;
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

/// Story 9-14 — Render the UX-DR8 Modal fragment for deactivating a user.
///
/// Final migration in the `hx-confirm` → Modal chain (9.10 → 9.14). The
/// trigger button at `templates/fragments/admin_users_row.html` issues
/// `hx-get` to this endpoint; on response the dialog is mounted into
/// `#modal-slot` (`layouts/base.html`). The Confirm form posts to the
/// existing 8-3 handler `admin_users_deactivate` with the same `version`
/// + `_csrf_token` body fields the original `<form hx-confirm>` carried.
///
/// Direct browser navigation (no `HX-Request` header) returns 405 — the
/// modal fragment is meaningless without page context. No `Allow:` response
/// header (per the 9-11 code-review patch — `Allow: GET` self-contradicts
/// 405 when we DO support GET, just not without HTMX).
pub async fn admin_users_deactivate_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<axum::response::Response, AppError> {
    // Admin-only feature (mirror of the trigger's enclosing template gate).
    // _with_return ensures an anonymous direct-URL hitter lands back on
    // /admin?tab=users after login.
    session.require_role_with_return(Role::Admin, "/admin?tab=users", locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    if !is_htmx {
        return Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response());
    }

    // `UserModel::find_by_id` returns deactivated users too (no
    // `deleted_at IS NULL` filter). Add an explicit guard so the modal
    // is never offered for an already-soft-deleted user (audit semantics:
    // "already deactivated; modal is meaningless"). Protects against
    // double-deactivation races.
    let user = UserModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;
    if user.deleted_at.is_some() {
        return Err(AppError::NotFound(
            rust_i18n::t!("error.not_found", locale = loc).to_string(),
        ));
    }

    // Title carries the username via `%{username}` interpolation. Pass
    // the RAW username through `t!()` and let Askama's default auto-escape
    // (on `{{ title }}` in the macro) handle HTML safety. Pre-escaping
    // would double-escape (`<` → `&lt;` → `&amp;lt;`).
    let title = rust_i18n::t!(
        "admin.users.deactivate_modal_title",
        locale = loc,
        username = user.username.as_str()
    )
    .to_string();
    let body_text = rust_i18n::t!("admin.users.deactivate_modal_body", locale = loc).to_string();
    let body_html = format!("<p>{}</p>", crate::utils::html_escape(&body_text));

    tracing::debug!(
        target_user_id = id,
        acting_user_id = ?session.user_id,
        "deactivate modal requested"
    );

    let template = AdminUserDeactivateModalTemplate {
        title,
        body_html,
        confirm_label: rust_i18n::t!("admin.users.deactivate_modal_confirm", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/admin/users/{}/deactivate", user.id),
        csrf_token: session.csrf_token.clone(),
        hx_target: format!("#admin-users-row-{}", user.id),
        version: user.version,
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => Err(AppError::Internal(format!(
            "admin user deactivate modal render: {e}"
        ))),
    }
}

pub async fn admin_users_update(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::Path(id): axum::extract::Path<u64>,
    Form(form): Form<UpdateUserForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users", locale.0)?;
    let loc = locale.0;

    // Validate username (trim whitespace; empty / length bounds — #55 MEDIUM #2)
    let username = form.username.trim().to_string();
    validate_username(&username, loc)?;

    // Validate role
    if form.role != "admin" && form.role != "librarian" {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.user.role_invalid", locale = loc).to_string(),
        ));
    }

    // Validate and hash password (optional)
    let password_trimmed = form.password.trim().to_string();
    let password_hash = if password_trimmed.is_empty() {
        None
    } else {
        if password_trimmed.len() < 8 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.user.password_too_short", locale = loc).to_string(),
            ));
        }
        if password_trimmed.len() > 72 {
            return Err(AppError::BadRequest(
                rust_i18n::t!("error.user.password_too_long", locale = loc).to_string(),
            ));
        }
        Some(password::hash_password(&password_trimmed)?)
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
                    Err(AppError::Conflict(
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
                Err(AppError::Conflict(
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
    let feedback = feedback_html("success", &success_msg, "");

    let row_html = render_user_row(&state, loc, &session, &user).await?;

    Ok(HtmxResponse {
        main: format!("{}{}", feedback, row_html),
        oob: vec![],
    })
}

/// Trigger UX: see GET /admin/users/:id/deactivate-modal (story 9-14).
pub async fn admin_users_deactivate(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::Path(id): axum::extract::Path<u64>,
    Form(form): Form<DeactivateForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users", locale.0)?;
    let loc = locale.0;
    let acting_admin_id = session.user_id.ok_or_else(|| {
        AppError::Internal("admin session missing user_id".to_string())
    })?;

    // Deactivate the user (guards handled by UserModel::deactivate). Map the
    // raw guard conflict keys to localized copy (#55 HIGH #6). VersionMismatch
    // is already localized by AppError::IntoResponse (#370), so it falls
    // through the catch-all unchanged.
    let sessions_killed =
        match UserModel::deactivate(&state.pool, id, form.version, acting_admin_id).await {
            Ok(n) => n,
            Err(AppError::Conflict(ref msg)) if msg == "self_deactivate_blocked" => {
                return Err(AppError::Conflict(
                    rust_i18n::t!("error.user.self_deactivate", locale = loc).to_string(),
                ));
            }
            Err(AppError::Conflict(ref msg)) if msg == "last_admin_blocked" => {
                return Err(AppError::Conflict(
                    rust_i18n::t!("error.user.last_admin", locale = loc).to_string(),
                ));
            }
            Err(e) => return Err(e),
        };
    tracing::info!(user_id = id, sessions_killed, "user deactivated");

    // Fetch updated user and render row
    let user = UserModel::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("User not found".to_string()))?;

    let success_msg = rust_i18n::t!("admin.users.success_deactivated", locale = loc, username = &user.username, count = sessions_killed)
        .to_string();
    let feedback = feedback_html("success", &success_msg, "");

    let row_html = render_user_row(&state, loc, &session, &user).await?;
    Ok(HtmxResponse {
        main: row_html,
        oob: vec![OobUpdate { swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn admin_users_reactivate(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::extract::Path(id): axum::extract::Path<u64>,
    Form(form): Form<ReactivateForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Admin, "/admin?tab=users", locale.0)?;
    let loc = locale.0;

    // Reactivate the user
    UserModel::reactivate(&state.pool, id, form.version).await?;

    // Fetch updated user and render row
    let user = UserModel::find_by_id(&state.pool, id)
        .await?
        .ok_or(AppError::NotFound("User not found".to_string()))?;

    let success_msg = rust_i18n::t!("admin.users.success_reactivated", locale = loc, username = &user.username)
        .to_string();
    let feedback = feedback_html("success", &success_msg, "");

    let row_html = render_user_row(&state, loc, &session, &user).await?;
    Ok(HtmxResponse {
        main: row_html,
        oob: vec![OobUpdate { swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

// ─── Rendering ──────────────────────────────────────────────────

/// Upper bound on the `?page=` query param for the users panel (#55 HIGH #4).
/// The offset is `(page - 1) * 25`, so an unclamped attacker-supplied page
/// (e.g. `?page=4000000000`) overflows the `u32` multiply — a debug panic /
/// release wrap — before the later `min(total_pages)` display clamp can run.
/// 10 000 pages × 25 rows = 250 000 users, far beyond any single-tenant
/// library, so clamping here is purely defensive and never truncates a real
/// result set.
const MAX_USERS_PAGE: u32 = 10_000;

/// Normalize the users-panel `?page=` param to `1..=MAX_USERS_PAGE` (#55 HIGH
/// #4). Pure + testable: `None`/`0` → 1, oversized → `MAX_USERS_PAGE`, so the
/// `(page - 1) * 25` offset can never overflow `u32`.
fn clamp_users_page(page: Option<u32>) -> u32 {
    page.unwrap_or(1).clamp(1, MAX_USERS_PAGE)
}

/// Render the Users panel. `pub(crate)` because the shared `admin::render_panel`
/// dispatch (which stays in `admin.rs`) calls this for `AdminTab::Users`.
pub(crate) async fn render_users_panel(
    state: &AppState,
    loc: &'static str,
    session: &Session,
    page: Option<u32>,
    filters: Option<UsersFilters>,
) -> Result<String, AppError> {
    let pool = &state.pool;
    // Clamp BEFORE computing the offset so a huge `?page=` can't overflow the
    // `(current_page - 1) * 25` multiply (#55 HIGH #4). The display-time
    // `min(total_pages)` clamp below still narrows this to the real last page.
    let current_page = clamp_users_page(page);

    // Extract and normalize filters
    let filters = filters.unwrap_or(UsersFilters { role: None, status: None, page: None });
    let role_filter = filters.role.as_deref();
    let status_filter = match filters.status.as_deref().unwrap_or("active") {
        "active" => crate::models::user::UserStatus::Active,
        "deactivated" => crate::models::user::UserStatus::Deactivated,
        "all" => crate::models::user::UserStatus::All,
        _ => crate::models::user::UserStatus::Active,
    };

    let users = crate::models::user::UserModel::list_page(
        pool,
        role_filter,
        status_filter,
        (current_page - 1) * 25,
        25,
    )
    .await?;

    let total = crate::models::user::UserModel::count_all(
        pool,
        role_filter,
        status_filter,
    )
    .await?;

    let total_pages = if total == 0 { 1 } else { ((total as f64) / 25.0).ceil() as u32 };
    let current_page = current_page.min(total_pages).max(1);

    let empty_state = if users.is_empty() && total == 0 {
        rust_i18n::t!("admin.users.empty_state", locale = loc).to_string()
    } else {
        String::new()
    };

    let panel = AdminUsersPanel {
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
        filter_role: filters.role.clone().unwrap_or_default(),
        filter_status: filters.status.clone().unwrap_or_else(|| "active".to_string()),
        page: current_page,
        total_pages,
        acting_admin_id: session.user_id.unwrap_or(0),
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
        acting_admin_id: session.user_id.unwrap_or(0),
    };

    row.render()
        .map_err(|_| AppError::Internal("admin user row render failed".to_string()))
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── #55: username validation + pagination clamp ────────

    #[test]
    fn validate_username_rejects_empty_short_and_overlong() {
        // Empty, below the 3-char floor, and above the 255-char ceiling all
        // map to a BadRequest (localized copy) rather than reaching the DB.
        assert!(matches!(
            validate_username("", "en"),
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            validate_username("ab", "en"),
            Err(AppError::BadRequest(_))
        ));
        let too_long = "x".repeat(256);
        assert!(matches!(
            validate_username(&too_long, "en"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn validate_username_accepts_in_range() {
        assert!(validate_username("abc", "en").is_ok());
        assert!(validate_username(&"x".repeat(255), "en").is_ok());
    }

    #[test]
    fn clamp_users_page_normalizes_bounds_and_prevents_offset_overflow() {
        assert_eq!(clamp_users_page(None), 1);
        assert_eq!(clamp_users_page(Some(0)), 1);
        assert_eq!(clamp_users_page(Some(5)), 5);
        assert_eq!(clamp_users_page(Some(u32::MAX)), MAX_USERS_PAGE);
        // The whole point of the clamp: the `(page - 1) * 25` offset built in
        // render_users_panel can never overflow u32 for any input.
        let offset = (clamp_users_page(Some(u32::MAX)) - 1).checked_mul(25);
        assert!(offset.is_some(), "clamped offset must not overflow u32");
    }
}
