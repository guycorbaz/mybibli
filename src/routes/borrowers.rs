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
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let borrowers = BorrowerModel::list_active(pool, params.page).await?;

    // CR #35 (v1.7.11 slice): shared page-template fields built via base_context.
    let base = crate::utils::base_context(&session, loc, "borrowers", &uri, state.session_timeout_secs());
    let template = BorrowersTemplate {
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
        current_url: base.current_url,
        lang_toggle_aria: base.lang_toggle_aria,
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
    Extension(locale): Extension<Locale>,
    axum::Form(form): axum::Form<CreateBorrowerForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
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
    pub nav_wishlist: String,
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
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let borrower = BorrowerModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    let active_loans = LoanModel::list_active_by_borrower(pool, borrower.id).await?;
    let threshold = state.settings.read().unwrap().overdue_threshold_days;

    // "borrower-detail" (not "borrowers") so the body[data-page] hook in
    // mybibli.js (initBorrowerDetailReload) only fires here, while the
    // nav highlight stays on /borrowers via the `borrowers` aria-current
    // matcher in nav_bar.html (no other code keys off this string).
    let base =
        crate::utils::base_context(&session, loc, "borrower-detail", &uri, state.session_timeout_secs());
    let template = BorrowerDetailTemplate {
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
        current_url: base.current_url,
        lang_toggle_aria: base.lang_toggle_aria,
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
    pub nav_wishlist: String,
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
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let borrower = BorrowerModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    let base = crate::utils::base_context(&session, loc, "borrowers", &uri, state.session_timeout_secs());
    let template = BorrowerEditTemplate {
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
        borrower,
        edit_title: rust_i18n::t!("borrower.edit", locale = loc).to_string(),
        name_label: rust_i18n::t!("borrower.name", locale = loc).to_string(),
        email_label: rust_i18n::t!("borrower.email", locale = loc).to_string(),
        phone_label: rust_i18n::t!("borrower.phone", locale = loc).to_string(),
        address_label: rust_i18n::t!("borrower.address", locale = loc).to_string(),
        save_label: rust_i18n::t!("borrower.save", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("borrower.cancel", locale = loc).to_string(),
        current_url: base.current_url,
        lang_toggle_aria: base.lang_toggle_aria,
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
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<UpdateBorrowerForm>,
) -> Result<impl IntoResponse, AppError> {
    // Story 7-1 decision 2a: Admin → Librarian.
    session.require_role(Role::Librarian, locale.0)?;
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
    session.require_role_with_return(Role::Admin, &format!("/borrower/{id}"), locale.0)?;
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

    // CR #136: when the row is gone (another librarian deleted it from
    // a parallel tab), don't 404 silently. The default `AppError::NotFound`
    // retargets to `#feedback-list`, which this page doesn't have. Return
    // 200 + an inline feedback fragment + `HX-Retarget: #borrower-feedback`
    // (the slot this page DOES declare) so the user sees what happened.
    let borrower = match BorrowerModel::find_by_id(pool, id).await? {
        Some(b) => b,
        None => {
            return Ok(crate::routes::build_already_deleted_response(
                loc,
                "#borrower-feedback",
            ));
        }
    };

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
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Admin, locale.0)?;
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
    Extension(locale): Extension<Locale>,
    uri: axum::http::Uri,
    axum::extract::Query(query): axum::extract::Query<BorrowerSearchQuery>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;

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

    /// Story 10-3: the borrowers list page must render both the
    /// mobile-card list (md:hidden) and the desktop table (hidden
    /// md:block) with the same borrower data.
    #[test]
    fn borrowers_template_renders_both_mobile_cards_and_desktop_table() {
        use crate::models::PaginatedList;
        use askama::Template;
        let borrower = BorrowerModel {
            id: 11,
            name: "Mobile-List-User".to_string(),
            email: Some("mobile@example.com".to_string()),
            phone: Some("+33611111111".to_string()),
            address: None,
            version: 1,
        };
        let template = BorrowersTemplate {
            lang: "en".to_string(),
            role: "librarian".to_string(),
            current_page: "borrowers",
            skip_label: "Skip".to_string(),
            connection_status: crate::utils::ConnectionStatusContext::new("en"),
            shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new("en"),
            session_timeout_secs: 1800,
            csrf_token: "tok".to_string(),
            nav_catalog: "Catalog".to_string(),
            nav_loans: "Loans".to_string(),
            nav_wishlist: "Wish list".to_string(),
            nav_locations: "Locations".to_string(),
            nav_series: "Series".to_string(),
            nav_borrowers: "Borrowers".to_string(),
            nav_admin: "Admin".to_string(),
            nav_login: "Log in".to_string(),
            nav_logout: "Log out".to_string(),
            nav_menu_open: "Open menu".to_string(),
            list_title: "Borrowers".to_string(),
            add_label: "Add".to_string(),
            name_label: "Name".to_string(),
            email_label: "Email".to_string(),
            phone_label: "Phone".to_string(),
            address_label: "Address".to_string(),
            save_label: "Save".to_string(),
            cancel_label: "Cancel".to_string(),
            empty_heading: "No borrowers".to_string(),
            empty_body: "Add one".to_string(),
            empty_cta: "Add".to_string(),
            prev_label: "Previous".to_string(),
            next_label: "Next".to_string(),
            pagination_aria: "Pagination".to_string(),
            borrowers: PaginatedList::new(vec![borrower], 1, 1, None, None, None),
            current_url: "/borrowers".to_string(),
            lang_toggle_aria: "Change language".to_string(),
            email_help: crate::utils::TooltipData::placeholder_only("borrower-email-help", "Email"),
            phone_help: crate::utils::TooltipData::placeholder_only("borrower-phone-help", "Phone"),
        };
        let html = template.render().unwrap();

        assert!(html.contains(r#"id="borrowers-cards-mobile""#));
        assert!(html.contains(r#"id="borrower-card-11""#));
        assert!(html.contains(r#"class="md:hidden"#));
        assert!(html.contains(r#"class="hidden md:block overflow-x-auto""#));
        // Same data in both surfaces (the borrower name appears in both)
        assert_eq!(
            html.matches("Mobile-List-User").count(),
            2,
            "borrower name must appear in both card and row"
        );
    }

    /// Story 10-3: borrower-detail page's active-loans section must
    /// render both the mobile-card list and the desktop table.
    #[test]
    fn borrower_detail_active_loans_renders_both_surfaces() {
        use askama::Template;
        let borrower = BorrowerModel {
            id: 22,
            name: "Detail-User".to_string(),
            email: None,
            phone: None,
            address: None,
            version: 1,
        };
        let loan = LoanWithDetails {
            id: 99,
            volume_id: 500,
            borrower_id: 22,
            borrower_name: "Detail-User".to_string(),
            volume_label: "V0099".to_string(),
            title_name: "Detail Test Book".to_string(),
            loaned_at: chrono::NaiveDate::from_ymd_opt(2026, 5, 14)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            duration_days: 1,
        };
        let template = BorrowerDetailTemplate {
            lang: "en".to_string(),
            role: "librarian".to_string(),
            current_page: "borrowers",
            skip_label: "Skip".to_string(),
            connection_status: crate::utils::ConnectionStatusContext::new("en"),
            shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new("en"),
            session_timeout_secs: 1800,
            csrf_token: "tok".to_string(),
            nav_catalog: "Catalog".to_string(),
            nav_loans: "Loans".to_string(),
            nav_wishlist: "Wish list".to_string(),
            nav_locations: "Locations".to_string(),
            nav_series: "Series".to_string(),
            nav_borrowers: "Borrowers".to_string(),
            nav_admin: "Admin".to_string(),
            nav_login: "Log in".to_string(),
            nav_logout: "Log out".to_string(),
            nav_menu_open: "Open menu".to_string(),
            borrower,
            address_label: "Address".to_string(),
            email_label: "Email".to_string(),
            phone_label: "Phone".to_string(),
            edit_label: "Edit".to_string(),
            delete_label: "Delete".to_string(),
            active_loans: vec![loan],
            active_loans_label: "Active loans".to_string(),
            no_active_loans_label: "No active loans".to_string(),
            overdue_threshold: 30,
            days_label: "days".to_string(),
            return_label: "Return".to_string(),
            overdue_label: "Overdue".to_string(),
            col_volume: "V-code".to_string(),
            col_title: "Title".to_string(),
            col_date: "Date".to_string(),
            col_duration: "Duration".to_string(),
            col_action: "Action".to_string(),
            current_url: "/borrower/22".to_string(),
            lang_toggle_aria: "Change language".to_string(),
        };
        let html = template.render().unwrap();

        assert!(html.contains(r#"id="borrower-loans-cards-mobile""#));
        assert!(html.contains(r#"id="borrower-loan-card-99""#));
        assert!(html.contains(r#"class="md:hidden"#));
        assert!(html.contains(r#"class="hidden md:block overflow-x-auto""#));
        // Same V-code appears in both card and table
        assert_eq!(
            html.matches("V0099").count(),
            2,
            "volume label must appear in both card and row"
        );
    }
}
