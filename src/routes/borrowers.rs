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
use crate::models::borrower::BorrowerModel;
use crate::models::loan::{LoanModel, LoanWithDetails};
use crate::services::borrowers::BorrowerService;
use crate::utils::current_url;

// ─── List page ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct BorrowerListQuery {
    #[serde(default = "default_page")]
    pub page: u32,
}

fn default_page() -> u32 {
    1
}

#[derive(Template)]
#[template(path = "pages/borrowers.html")]
pub struct BorrowersTemplate {
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
    pub email_label: String,
    pub phone_label: String,
    pub address_label: String,
    pub save_label: String,
    pub cancel_label: String,
    pub empty_heading: String,
    pub empty_body: String,
    pub empty_cta: String,
    pub prev_label: String,
    pub next_label: String,
    pub pagination_aria: String,
    pub borrowers: PaginatedList<BorrowerModel>,
    pub current_url: String,
    pub lang_toggle_aria: String,
    pub email_help: crate::utils::TooltipData,
    pub phone_help: crate::utils::TooltipData,
}

pub async fn borrowers_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(_is_htmx): HxRequest,
    axum::extract::Query(params): axum::extract::Query<BorrowerListQuery>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path())?;
    let pool = &state.pool;
    let loc = locale.0;

    let borrowers = BorrowerModel::list_active(pool, params.page).await?;

    let template = BorrowersTemplate {
        lang: loc.to_string(),
        role: session.role.to_string(),
        current_page: "borrowers",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        connection_status: crate::utils::ConnectionStatusContext::new(loc),
        shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new(loc),
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
        nav_menu_open: rust_i18n::t!("nav.menu_open", locale = loc).to_string(),
        list_title: rust_i18n::t!("borrower.list_title", locale = loc).to_string(),
        add_label: rust_i18n::t!("borrower.add", locale = loc).to_string(),
        name_label: rust_i18n::t!("borrower.name", locale = loc).to_string(),
        email_label: rust_i18n::t!("borrower.email", locale = loc).to_string(),
        phone_label: rust_i18n::t!("borrower.phone", locale = loc).to_string(),
        address_label: rust_i18n::t!("borrower.address", locale = loc).to_string(),
        save_label: rust_i18n::t!("borrower.save", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("borrower.cancel", locale = loc).to_string(),
        empty_heading: rust_i18n::t!("empty.borrowers_heading", locale = loc).to_string(),
        empty_body: rust_i18n::t!("empty.borrowers_body", locale = loc).to_string(),
        empty_cta: rust_i18n::t!("empty.borrowers_cta", locale = loc).to_string(),
        prev_label: rust_i18n::t!("pagination.previous", locale = loc).to_string(),
        next_label: rust_i18n::t!("pagination.next", locale = loc).to_string(),
        pagination_aria: rust_i18n::t!("pagination.aria_label", locale = loc).to_string(),
        borrowers,
        current_url: current_url(&uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = loc).to_string(),
        email_help: crate::utils::TooltipData::with_icon(
            "tip-borrower-email-create",
            &rust_i18n::t!("help.borrower.email_summary", locale = loc),
            &rust_i18n::t!("help.borrower.email_text", locale = loc),
        ),
        phone_help: crate::utils::TooltipData::with_icon(
            "tip-borrower-phone-create",
            &rust_i18n::t!("help.borrower.phone_summary", locale = loc),
            &rust_i18n::t!("help.borrower.phone_text", locale = loc),
        ),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

// ─── Create ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateBorrowerForm {
    pub name: String,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
}

pub async fn create_borrower(
    State(state): State<AppState>,
    session: Session,
    axum::Form(form): axum::Form<CreateBorrowerForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    let borrower =
        BorrowerService::create_borrower(pool, &form.name, form.address, form.email, form.phone)
            .await?;

    tracing::info!(borrower_id = borrower.id, name = %borrower.name, "Borrower created");
    Ok(Redirect::to("/borrowers"))
}

// ─── Detail page ────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/borrower_detail.html")]
pub struct BorrowerDetailTemplate {
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
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub nav_menu_open: String,
    pub borrower: BorrowerModel,
    pub address_label: String,
    pub email_label: String,
    pub phone_label: String,
    pub edit_label: String,
    pub delete_label: String,
    pub active_loans: Vec<LoanWithDetails>,
    pub active_loans_label: String,
    pub no_active_loans_label: String,
    pub overdue_threshold: i64,
    pub days_label: String,
    pub return_label: String,
    pub overdue_label: String,
    pub col_volume: String,
    pub col_title: String,
    pub col_date: String,
    pub col_duration: String,
    pub col_action: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
}

pub async fn borrower_detail(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(_is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path())?;
    let pool = &state.pool;
    let loc = locale.0;

    let borrower = BorrowerModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    let active_loans = LoanModel::list_active_by_borrower(pool, borrower.id).await?;
    let threshold = state.settings.read().unwrap().overdue_threshold_days;

    let template = BorrowerDetailTemplate {
        lang: loc.to_string(),
        role: session.role.to_string(),
        // "borrower-detail" (not "borrowers") so the body[data-page] hook in
        // mybibli.js (initBorrowerDetailReload) only fires here, while the
        // nav highlight stays on /borrowers via the `borrowers` aria-current
        // matcher in nav_bar.html (no other code keys off this string).
        current_page: "borrower-detail",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        connection_status: crate::utils::ConnectionStatusContext::new(loc),
        shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new(loc),
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
        nav_menu_open: rust_i18n::t!("nav.menu_open", locale = loc).to_string(),
        borrower,
        address_label: rust_i18n::t!("borrower.address", locale = loc).to_string(),
        email_label: rust_i18n::t!("borrower.email", locale = loc).to_string(),
        phone_label: rust_i18n::t!("borrower.phone", locale = loc).to_string(),
        edit_label: rust_i18n::t!("borrower.edit", locale = loc).to_string(),
        delete_label: rust_i18n::t!("borrower.delete", locale = loc).to_string(),
        active_loans,
        active_loans_label: rust_i18n::t!("borrower.active_loans", locale = loc).to_string(),
        no_active_loans_label: rust_i18n::t!("borrower.no_active_loans", locale = loc).to_string(),
        overdue_threshold: threshold as i64,
        days_label: rust_i18n::t!("loan.days", locale = loc).to_string(),
        return_label: rust_i18n::t!("loan.return", locale = loc).to_string(),
        overdue_label: rust_i18n::t!("loan.overdue", locale = loc).to_string(),
        col_volume: rust_i18n::t!("loan.col_volume", locale = loc).to_string(),
        col_title: rust_i18n::t!("loan.col_title", locale = loc).to_string(),
        col_date: rust_i18n::t!("loan.col_date", locale = loc).to_string(),
        col_duration: rust_i18n::t!("loan.col_duration", locale = loc).to_string(),
        col_action: rust_i18n::t!("loan.col_action", locale = loc).to_string(),
        current_url: current_url(&uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = loc).to_string(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

// ─── Edit page ──────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/borrower_edit.html")]
pub struct BorrowerEditTemplate {
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
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub nav_menu_open: String,
    pub borrower: BorrowerModel,
    pub edit_title: String,
    pub name_label: String,
    pub email_label: String,
    pub phone_label: String,
    pub address_label: String,
    pub save_label: String,
    pub cancel_label: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
    pub email_help: crate::utils::TooltipData,
    pub phone_help: crate::utils::TooltipData,
}

pub async fn edit_borrower_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(_is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    // Story 7-1 decision 2a: Admin → Librarian.
    session.require_role_with_return(Role::Librarian, uri.path())?;
    let pool = &state.pool;
    let loc = locale.0;

    let borrower = BorrowerModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    let template = BorrowerEditTemplate {
        lang: loc.to_string(),
        role: session.role.to_string(),
        current_page: "borrowers",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        connection_status: crate::utils::ConnectionStatusContext::new(loc),
        shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new(loc),
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
        nav_menu_open: rust_i18n::t!("nav.menu_open", locale = loc).to_string(),
        borrower,
        edit_title: rust_i18n::t!("borrower.edit", locale = loc).to_string(),
        name_label: rust_i18n::t!("borrower.name", locale = loc).to_string(),
        email_label: rust_i18n::t!("borrower.email", locale = loc).to_string(),
        phone_label: rust_i18n::t!("borrower.phone", locale = loc).to_string(),
        address_label: rust_i18n::t!("borrower.address", locale = loc).to_string(),
        save_label: rust_i18n::t!("borrower.save", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("borrower.cancel", locale = loc).to_string(),
        current_url: current_url(&uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = loc).to_string(),
        email_help: crate::utils::TooltipData::with_icon(
            "tip-borrower-email-edit",
            &rust_i18n::t!("help.borrower.email_summary", locale = loc),
            &rust_i18n::t!("help.borrower.email_text", locale = loc),
        ),
        phone_help: crate::utils::TooltipData::with_icon(
            "tip-borrower-phone-edit",
            &rust_i18n::t!("help.borrower.phone_summary", locale = loc),
            &rust_i18n::t!("help.borrower.phone_text", locale = loc),
        ),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

// ─── Update ─────────────────────────────────────────────

#[derive(Deserialize)]
pub struct UpdateBorrowerForm {
    pub version: i32,
    pub name: String,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
}

pub async fn update_borrower(
    State(state): State<AppState>,
    session: Session,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<UpdateBorrowerForm>,
) -> Result<impl IntoResponse, AppError> {
    // Story 7-1 decision 2a: Admin → Librarian.
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    BorrowerService::update_borrower(
        pool,
        id,
        form.version,
        &form.name,
        form.address,
        form.email,
        form.phone,
    )
    .await?;

    tracing::info!(borrower_id = id, "Borrower updated");
    Ok(Redirect::to(&format!("/borrower/{id}")))
}

// ─── Delete confirmation modal (story 9-10) ─────────────

#[derive(Template)]
#[template(path = "fragments/borrower_delete_modal.html")]
pub struct BorrowerDeleteModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
}

/// `GET /borrower/:id/delete-modal` — returns the rendered UX-DR8 Modal
/// fragment for the destructive delete-borrower flow. Admin-only. Direct
/// browser navigation (no `HX-Request` header) returns 405 — the modal
/// fragment is meaningless without the page context.
pub async fn delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<axum::response::Response, AppError> {
    // Preserve the borrower-detail return path so an anonymous user who
    // hits this URL directly (or whose session expired) lands back on the
    // borrower page after login, not on /home.
    session.require_role_with_return(Role::Admin, &format!("/borrower/{id}"))?;
    let pool = &state.pool;
    let loc = locale.0;

    if !is_htmx {
        // The route DOES accept GET — but only via HTMX. 405 + Allow: GET
        // truthfully advertises the method without misleading proxies/
        // conformance tools (the previous Allow: OPTIONS was incorrect).
        return Ok((
            axum::http::StatusCode::METHOD_NOT_ALLOWED,
            [(axum::http::header::ALLOW, "GET")],
            String::new(),
        )
            .into_response());
    }

    let borrower = BorrowerModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
        })?;

    // Title carries the borrower name via `%{name}` interpolation. Pass
    // the RAW name through `t!()` and let Askama's default auto-escape
    // (on `{{ title }}` in the macro) handle HTML safety. Pre-escaping
    // would double-escape (`<` → `&lt;` → `&amp;lt;`).
    let title = rust_i18n::t!(
        "borrower.delete_modal_title",
        locale = loc,
        name = borrower.name.as_str()
    )
    .to_string();
    let body_text = rust_i18n::t!("borrower.delete_modal_body", locale = loc).to_string();
    // Body has no user-supplied interpolation; the i18n value is controlled.
    // Wrap in <p> and ship via `|safe` — no escape needed.
    let body_html = format!("<p>{}</p>", crate::utils::html_escape(&body_text));

    tracing::debug!(borrower_id = id, "delete modal requested");

    let template = BorrowerDeleteModalTemplate {
        title,
        body_html,
        confirm_label: rust_i18n::t!("borrower.delete_modal_confirm", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/borrower/{}", borrower.id),
        csrf_token: session.csrf_token.clone(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => Err(AppError::Internal(format!(
            "borrower delete modal render: {e}"
        ))),
    }
}

// ─── Delete ─────────────────────────────────────────────

pub async fn delete_borrower(
    State(state): State<AppState>,
    session: Session,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Admin)?;
    let pool = &state.pool;

    BorrowerService::delete_borrower(pool, id).await?;

    if is_htmx {
        // HX-Redirect tells HTMX to do a full-page navigation
        Ok((
            axum::http::StatusCode::OK,
            [(
                axum::http::header::HeaderName::from_static("hx-redirect"),
                "/borrowers".to_string(),
            )],
            String::new(),
        )
            .into_response())
    } else {
        Ok(Redirect::to("/borrowers").into_response())
    }
}

// ─── Search (autocomplete) ──────────────────────────────

#[derive(Deserialize)]
pub struct BorrowerSearchQuery {
    pub q: String,
}

pub async fn borrower_search(
    session: Session,
    State(state): State<AppState>,
    uri: axum::http::Uri,
    axum::extract::Query(query): axum::extract::Query<BorrowerSearchQuery>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path())?;

    let q = query.q.trim();
    if q.len() < 2 || q.len() > 255 {
        return Ok(axum::Json(serde_json::json!([])).into_response());
    }

    let results = BorrowerModel::search_by_name(&state.pool, q, 10).await?;

    let json: Vec<serde_json::Value> = results
        .iter()
        .map(|b| serde_json::json!({"id": b.id, "name": b.name}))
        .collect();

    Ok(axum::Json(json).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_page() {
        assert_eq!(default_page(), 1);
    }

    #[test]
    fn test_create_form_fields() {
        let form = CreateBorrowerForm {
            name: "Jean Dupont".to_string(),
            email: Some("jean@example.com".to_string()),
            address: None,
            phone: None,
        };
        assert_eq!(form.name, "Jean Dupont");
        assert_eq!(form.email.as_deref(), Some("jean@example.com"));
        assert!(form.address.is_none());
    }

    #[test]
    fn test_update_form_fields() {
        let form = UpdateBorrowerForm {
            version: 1,
            name: "Marie".to_string(),
            phone: Some("+33612345678".to_string()),
            address: None,
            email: None,
        };
        assert_eq!(form.version, 1);
        assert_eq!(form.name, "Marie");
        assert_eq!(form.phone.as_deref(), Some("+33612345678"));
    }
}
