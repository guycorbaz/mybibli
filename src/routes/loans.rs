use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest};
use crate::models::PaginatedList;
use crate::models::loan::{LoanModel, LoanWithDetails};
use crate::models::volume::VolumeModel;
use crate::services::loans::LoanService;

// ─── List page ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoanListQuery {
    #[serde(default = "default_page")]
    pub page: u32,
    pub sort: Option<String>,
    pub dir: Option<String>,
}

fn default_page() -> u32 {
    1
}

#[derive(Template)]
#[template(path = "pages/loans.html")]
pub struct LoansTemplate {
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
    pub new_loan_label: String,
    pub volume_label_label: String,
    pub borrower_label: String,
    pub borrower_search_label: String,
    pub register_label: String,
    pub col_borrower: String,
    pub col_volume: String,
    pub col_title: String,
    pub col_date: String,
    pub col_duration: String,
    pub days_label: String,
    pub scan_placeholder: String,
    pub empty_state: String,
    pub prev_label: String,
    pub next_label: String,
    pub return_label: String,
    pub overdue_label: String,
    pub confirm_label: String,
    pub col_action: String,
    pub overdue_threshold: i64,
    pub current_sort: String,
    pub current_dir: String,
    pub loans: PaginatedList<LoanWithDetails>,
    pub highlight_loan_id: Option<u64>,
}

pub async fn loans_page(
    State(state): State<AppState>,
    session: Session,
    HxRequest(_is_htmx): HxRequest,
    uri: axum::http::Uri,
    axum::extract::Query(params): axum::extract::Query<LoanListQuery>,
) -> Result<impl IntoResponse, AppError> {
    // AC #2: preserve `next` so post-login lands back on /loans.
    session.require_role_with_return(Role::Librarian, uri.path())?;
    let pool = &state.pool;

    let loans = LoanModel::list_active(pool, params.page, &params.sort, &params.dir).await?;
    let threshold = state.settings.read().unwrap().overdue_threshold_days;

    // Resolve current sort/dir for template (matches what list_active actually used)
    let current_sort = loans.sort.clone().unwrap_or_else(|| "date".to_string());
    let current_dir = loans.dir.clone().unwrap_or_else(|| "desc".to_string());

    let template = LoansTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "loans",
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
        list_title: rust_i18n::t!("loan.list_title").to_string(),
        new_loan_label: rust_i18n::t!("loan.new").to_string(),
        volume_label_label: rust_i18n::t!("loan.volume_label").to_string(),
        borrower_label: rust_i18n::t!("loan.borrower").to_string(),
        borrower_search_label: rust_i18n::t!("loan.borrower_search").to_string(),
        register_label: rust_i18n::t!("loan.register").to_string(),
        col_borrower: rust_i18n::t!("loan.col_borrower").to_string(),
        col_volume: rust_i18n::t!("loan.col_volume").to_string(),
        col_title: rust_i18n::t!("loan.col_title").to_string(),
        col_date: rust_i18n::t!("loan.col_date").to_string(),
        col_duration: rust_i18n::t!("loan.col_duration").to_string(),
        days_label: rust_i18n::t!("loan.days").to_string(),
        scan_placeholder: rust_i18n::t!("loan.scan_placeholder").to_string(),
        empty_state: rust_i18n::t!("loan.empty_state").to_string(),
        prev_label: rust_i18n::t!("pagination.previous").to_string(),
        next_label: rust_i18n::t!("pagination.next").to_string(),
        return_label: rust_i18n::t!("loan.return").to_string(),
        overdue_label: rust_i18n::t!("loan.overdue").to_string(),
        confirm_label: rust_i18n::t!("loan.return_confirm").to_string(),
        col_action: rust_i18n::t!("loan.col_action").to_string(),
        overdue_threshold: threshold as i64,
        current_sort,
        current_dir,
        loans,
        highlight_loan_id: None,
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

// ─── Create loan ────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateLoanForm {
    pub volume_label: String,
    pub borrower_id: u64,
}

pub async fn create_loan(
    State(state): State<AppState>,
    session: Session,
    HxRequest(is_htmx): HxRequest,
    axum::Form(form): axum::Form<CreateLoanForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    // Trim volume label to handle whitespace from form input
    let volume_label = form.volume_label.trim().to_uppercase();

    // Look up volume by label
    let volume = match VolumeModel::find_by_label(pool, &volume_label).await? {
        Some(v) => v,
        None if is_htmx => {
            let message = rust_i18n::t!("loan.volume_not_found").to_string();
            let feedback = crate::routes::catalog::feedback_html_pub("error", &message, "");
            return Ok(Html(feedback).into_response());
        }
        None => {
            return Err(AppError::BadRequest(
                rust_i18n::t!("loan.volume_not_found").to_string(),
            ));
        }
    };

    match LoanService::register_loan(pool, volume.id, form.borrower_id).await {
        Ok(loan) => {
            // Get borrower name for success message (HTML-escaped for safe rendering)
            let borrower =
                crate::models::borrower::BorrowerModel::find_by_id(pool, loan.borrower_id)
                    .await?
                    .map(|b| b.name)
                    .unwrap_or_default();
            let escaped_borrower = crate::utils::html_escape(&borrower);
            let escaped_label = crate::utils::html_escape(&volume_label);

            let message = rust_i18n::t!(
                "loan.created",
                label = escaped_label,
                borrower = escaped_borrower
            )
            .to_string();

            if is_htmx {
                let feedback = crate::routes::catalog::feedback_html_pub("success", &message, "");
                Ok(HtmxResponse {
                    main: feedback,
                    oob: vec![],
                }
                .into_response())
            } else {
                Ok(axum::response::Redirect::to("/loans").into_response())
            }
        }
        Err(AppError::BadRequest(msg)) if is_htmx => {
            let feedback = crate::routes::catalog::feedback_html_pub("error", &msg, "");
            Ok(Html(feedback).into_response())
        }
        Err(e) => Err(e),
    }
}

// ─── Return loan ────────────────────────────────────────

pub async fn return_loan_handler(
    State(state): State<AppState>,
    session: Session,
    HxRequest(is_htmx): HxRequest,
    axum::extract::Path(loan_id): axum::extract::Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;
    let pool = &state.pool;

    let (label, path) = LoanService::return_loan(pool, loan_id).await?;

    let message = match path {
        Some(ref p) => rust_i18n::t!("loan.returned", label = label, path = p).to_string(),
        None => rust_i18n::t!("loan.returned_no_location", label = label).to_string(),
    };

    if is_htmx {
        let feedback = crate::routes::catalog::feedback_html_pub("success", &message, "");
        Ok(HtmxResponse {
            main: feedback,
            oob: vec![],
        }
        .into_response())
    } else {
        Ok(axum::response::Redirect::to("/loans").into_response())
    }
}

// ─── Scan V-code on loans page ──────────────────────────

#[derive(Deserialize)]
pub struct ScanQuery {
    pub code: String,
}

pub async fn scan_on_loans(
    State(state): State<AppState>,
    session: Session,
    HxRequest(_is_htmx): HxRequest,
    uri: axum::http::Uri,
    axum::extract::Query(params): axum::extract::Query<ScanQuery>,
) -> Result<impl IntoResponse, AppError> {
    // Strip query string from `next` — no point replaying a failed scan after login,
    // and the user-supplied `?code=` shouldn't be reflected into the login form.
    session.require_role_with_return(Role::Librarian, uri.path())?;
    let pool = &state.pool;
    let code = params.code.trim().to_uppercase();

    // Check if V-code format
    if !crate::services::volume::VolumeService::validate_vcode(&code) {
        let message = rust_i18n::t!("feedback.vcode_invalid").to_string();
        return Ok(Html(crate::routes::catalog::feedback_html_pub(
            "warning", &message, "",
        ))
        .into_response());
    }

    // Check if volume exists
    let volume = VolumeModel::find_by_label(pool, &code).await?;
    if volume.is_none() {
        let message = rust_i18n::t!("loan.volume_not_found").to_string();
        return Ok(Html(crate::routes::catalog::feedback_html_pub(
            "warning", &message, "",
        ))
        .into_response());
    }

    // Check if volume is on loan
    match LoanModel::find_active_by_volume_label(pool, &code).await? {
        Some(loan_detail) => {
            // Return highlighted loan row
            let row_html = loan_row_html(&loan_detail, true);
            Ok(Html(row_html).into_response())
        }
        None => {
            let message = rust_i18n::t!("loan.not_on_loan").to_string();
            Ok(Html(crate::routes::catalog::feedback_html_pub(
                "info", &message, "",
            ))
            .into_response())
        }
    }
}

/// Render a loan match result card (for scan-to-find on /loans page).
fn loan_row_html(loan: &LoanWithDetails, highlight: bool) -> String {
    let bg = if highlight {
        "bg-yellow-50 dark:bg-yellow-900/20 border-yellow-400"
    } else {
        "bg-stone-50 dark:bg-stone-800 border-stone-300 dark:border-stone-600"
    };
    let escaped_borrower = crate::utils::html_escape(&loan.borrower_name);
    let escaped_label = crate::utils::html_escape(&loan.volume_label);
    let escaped_title = crate::utils::html_escape(&loan.title_name);
    let date = loan.loaned_at.format("%Y-%m-%d").to_string();
    let days = rust_i18n::t!("loan.days").to_string();
    let return_label = rust_i18n::t!("loan.return").to_string();
    let confirm_label = rust_i18n::t!("loan.return_confirm").to_string();

    format!(
        r#"<div class="p-3 rounded-md border {bg}" id="scan-loan-{id}">
            <p class="font-medium text-stone-900 dark:text-stone-100">{label} — {title}</p>
            <p class="text-sm text-stone-600 dark:text-stone-400">
                <a href="/borrower/{bid}" class="text-indigo-600 hover:underline dark:text-indigo-400">{borrower}</a>
                · {date} · {duration} {days}
            </p>
            <button hx-post="/loans/{id}/return" hx-confirm="{confirm}" hx-target="{target}"
                    hx-disabled-elt="this"
                    class="mt-2 px-3 py-1 text-sm font-medium text-white bg-indigo-600 rounded hover:bg-indigo-700 disabled:opacity-50">
                {return_label}
            </button>
        </div>"#,
        bg = bg,
        id = loan.id,
        bid = loan.borrower_id,
        borrower = escaped_borrower,
        label = escaped_label,
        title = escaped_title,
        date = date,
        duration = loan.duration_days,
        days = days,
        return_label = return_label,
        confirm = crate::utils::html_escape(&confirm_label),
        target = "#scan-result",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_loan_form_fields() {
        let form = CreateLoanForm {
            volume_label: "V0042".to_string(),
            borrower_id: 5,
        };
        assert_eq!(form.volume_label, "V0042");
        assert_eq!(form.borrower_id, 5);
    }

    #[test]
    fn test_scan_query_fields() {
        let query = ScanQuery {
            code: "V0042".to_string(),
        };
        assert_eq!(query.code, "V0042");
    }

    #[test]
    fn test_default_page() {
        assert_eq!(default_page(), 1);
    }

    #[test]
    fn test_loan_row_html_highlighted() {
        let loan = LoanWithDetails {
            id: 1,
            volume_id: 10,
            borrower_id: 20,
            borrower_name: "Jean".to_string(),
            volume_label: "V0042".to_string(),
            title_name: "Test Book".to_string(),
            loaned_at: chrono::NaiveDate::from_ymd_opt(2026, 4, 1)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            duration_days: 3,
        };
        let html = loan_row_html(&loan, true);
        assert!(html.contains("bg-yellow-50"));
        assert!(html.contains("border-yellow-400"));
        assert!(html.contains("Jean"));
        assert!(html.contains("V0042"));
        assert!(html.contains("Test Book"));
        assert!(html.contains("hx-post=\"/loans/1/return\""));
    }

    #[test]
    fn test_loan_row_html_not_highlighted() {
        let loan = LoanWithDetails {
            id: 2,
            volume_id: 11,
            borrower_id: 21,
            borrower_name: "Marie".to_string(),
            volume_label: "V0001".to_string(),
            title_name: "Another".to_string(),
            loaned_at: chrono::NaiveDate::from_ymd_opt(2026, 4, 1)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            duration_days: 0,
        };
        let html = loan_row_html(&loan, false);
        assert!(!html.contains("bg-yellow-50"));
        assert!(html.contains("bg-stone-50"));
        assert!(html.contains("Marie"));
    }

    #[test]
    fn test_loan_row_html_escapes_special_chars() {
        let loan = LoanWithDetails {
            id: 3,
            volume_id: 12,
            borrower_id: 22,
            borrower_name: "O'Brien <script>".to_string(),
            volume_label: "V0003".to_string(),
            title_name: "Book & Title".to_string(),
            loaned_at: chrono::NaiveDate::from_ymd_opt(2026, 4, 1)
                .unwrap()
                .and_hms_opt(10, 0, 0)
                .unwrap(),
            duration_days: 1,
        };
        let html = loan_row_html(&loan, false);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&amp;"));
    }
}
