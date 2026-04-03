use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Redirect};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::models::borrower::BorrowerModel;
use crate::models::loan::{LoanModel, LoanWithDetails};
use crate::models::PaginatedList;
use crate::services::borrowers::BorrowerService;
use crate::AppState;

// ─── List page ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct BorrowerListQuery {
    #[serde(default = "default_page")]
    pub page: u32,
}

fn default_page() -> u32 { 1 }

#[derive(Template)]
#[template(path = "pages/borrowers.html")]
pub struct BorrowersTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub list_title: String,
    pub add_label: String,
    pub name_label: String,
    pub email_label: String,
    pub phone_label: String,
    pub address_label: String,
    pub save_label: String,
    pub cancel_label: String,
    pub empty_state: String,
    pub prev_label: String,
    pub next_label: String,
    pub borrowers: PaginatedList<BorrowerModel>,
}

pub async fn borrowers_page(
    State(state): State<AppState>,
    session: Session,
    HxRequest(_is_htmx): HxRequest,
    axum::extract::Query(params): axum::extract::Query<BorrowerListQuery>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    let borrowers = BorrowerModel::list_active(pool, params.page).await?;

    let template = BorrowersTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "borrowers",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
        nav_locations: rust_i18n::t!("nav.locations").to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        list_title: rust_i18n::t!("borrower.list_title").to_string(),
        add_label: rust_i18n::t!("borrower.add").to_string(),
        name_label: rust_i18n::t!("borrower.name").to_string(),
        email_label: rust_i18n::t!("borrower.email").to_string(),
        phone_label: rust_i18n::t!("borrower.phone").to_string(),
        address_label: rust_i18n::t!("borrower.address").to_string(),
        save_label: rust_i18n::t!("borrower.save").to_string(),
        cancel_label: rust_i18n::t!("borrower.cancel").to_string(),
        empty_state: rust_i18n::t!("borrower.empty_state").to_string(),
        prev_label: rust_i18n::t!("pagination.previous").to_string(),
        next_label: rust_i18n::t!("pagination.next").to_string(),
        borrowers,
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

    let borrower = BorrowerService::create_borrower(pool, &form.name, form.address, form.email, form.phone).await?;

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
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub borrower: BorrowerModel,
    pub address_label: String,
    pub email_label: String,
    pub phone_label: String,
    pub edit_label: String,
    pub delete_label: String,
    pub confirm_delete: String,
    pub active_loans: Vec<LoanWithDetails>,
    pub active_loans_label: String,
    pub no_active_loans_label: String,
    pub overdue_threshold: i64,
    pub days_label: String,
    pub return_label: String,
    pub overdue_label: String,
    pub confirm_label: String,
    pub col_volume: String,
    pub col_title: String,
    pub col_date: String,
    pub col_duration: String,
    pub col_action: String,
}

pub async fn borrower_detail(
    State(state): State<AppState>,
    session: Session,
    HxRequest(_is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    let borrower = BorrowerModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    let active_loans = LoanModel::list_active_by_borrower(pool, borrower.id).await?;
    let threshold = state.settings.read().unwrap().overdue_threshold_days;

    let template = BorrowerDetailTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "borrowers",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
        nav_locations: rust_i18n::t!("nav.locations").to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        borrower,
        address_label: rust_i18n::t!("borrower.address").to_string(),
        email_label: rust_i18n::t!("borrower.email").to_string(),
        phone_label: rust_i18n::t!("borrower.phone").to_string(),
        edit_label: rust_i18n::t!("borrower.edit").to_string(),
        delete_label: rust_i18n::t!("borrower.delete").to_string(),
        confirm_delete: rust_i18n::t!("borrower.confirm_delete").to_string(),
        active_loans,
        active_loans_label: rust_i18n::t!("borrower.active_loans").to_string(),
        no_active_loans_label: rust_i18n::t!("borrower.no_active_loans").to_string(),
        overdue_threshold: threshold as i64,
        days_label: rust_i18n::t!("loan.days").to_string(),
        return_label: rust_i18n::t!("loan.return").to_string(),
        overdue_label: rust_i18n::t!("loan.overdue").to_string(),
        confirm_label: rust_i18n::t!("loan.return_confirm").to_string(),
        col_volume: rust_i18n::t!("loan.col_volume").to_string(),
        col_title: rust_i18n::t!("loan.col_title").to_string(),
        col_date: rust_i18n::t!("loan.col_date").to_string(),
        col_duration: rust_i18n::t!("loan.col_duration").to_string(),
        col_action: rust_i18n::t!("loan.col_action").to_string(),
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
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub borrower: BorrowerModel,
    pub edit_title: String,
    pub name_label: String,
    pub email_label: String,
    pub phone_label: String,
    pub address_label: String,
    pub save_label: String,
    pub cancel_label: String,
}

pub async fn edit_borrower_page(
    State(state): State<AppState>,
    session: Session,
    HxRequest(_is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Admin)?;
    let pool = &state.pool;

    let borrower = BorrowerModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    let template = BorrowerEditTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "borrowers",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
        nav_locations: rust_i18n::t!("nav.locations").to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        borrower,
        edit_title: rust_i18n::t!("borrower.edit").to_string(),
        name_label: rust_i18n::t!("borrower.name").to_string(),
        email_label: rust_i18n::t!("borrower.email").to_string(),
        phone_label: rust_i18n::t!("borrower.phone").to_string(),
        address_label: rust_i18n::t!("borrower.address").to_string(),
        save_label: rust_i18n::t!("borrower.save").to_string(),
        cancel_label: rust_i18n::t!("borrower.cancel").to_string(),
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
    session.require_role(Role::Admin)?;
    let pool = &state.pool;

    BorrowerService::update_borrower(pool, id, form.version, &form.name, form.address, form.email, form.phone).await?;

    tracing::info!(borrower_id = id, "Borrower updated");
    Ok(Redirect::to(&format!("/borrower/{id}")))
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
            [(axum::http::header::HeaderName::from_static("hx-redirect"), "/borrowers".to_string())],
            String::new(),
        ).into_response())
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
    axum::extract::Query(query): axum::extract::Query<BorrowerSearchQuery>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

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
