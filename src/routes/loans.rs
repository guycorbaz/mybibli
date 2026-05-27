use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest};
use crate::middleware::locale::Locale;
use crate::models::PaginatedList;
use crate::models::loan::{LoanModel, LoanWithDetails};
use crate::models::volume::VolumeModel;
use crate::services::loans::LoanService;
use crate::utils::base_context;

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
    pub empty_heading: String,
    pub empty_body: String,
    pub prev_label: String,
    pub next_label: String,
    pub pagination_aria: String,
    pub return_label: String,
    pub overdue_label: String,
    pub col_action: String,
    pub overdue_threshold: i64,
    pub current_sort: String,
    pub current_dir: String,
    pub loans: PaginatedList<LoanWithDetails>,
    pub highlight_loan_id: Option<u64>,
    pub current_url: String,
    pub lang_toggle_aria: String,
}

pub async fn loans_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(_is_htmx): HxRequest,
    axum::extract::Query(params): axum::extract::Query<LoanListQuery>,
) -> Result<impl IntoResponse, AppError> {
    // AC #2: preserve `next` so post-login lands back on /loans.
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let loans = LoanModel::list_active(pool, params.page, &params.sort, &params.dir).await?;
    let threshold = state.settings.read().unwrap().overdue_threshold_days;

    // Resolve current sort/dir for template (matches what list_active actually used)
    let current_sort = loans.sort.clone().unwrap_or_else(|| "date".to_string());
    let current_dir = loans.dir.clone().unwrap_or_else(|| "desc".to_string());

    // CR #35 (v1.7.11 slice): shared page-template fields built via the
    // base_context helper. The remaining fields are page-specific.
    let base = base_context(&session, loc, "loans", &uri, state.session_timeout_secs());
    let template = LoansTemplate {
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
        list_title: rust_i18n::t!("loan.list_title", locale = loc).to_string(),
        new_loan_label: rust_i18n::t!("loan.new", locale = loc).to_string(),
        volume_label_label: rust_i18n::t!("loan.volume_label", locale = loc).to_string(),
        borrower_label: rust_i18n::t!("loan.borrower", locale = loc).to_string(),
        borrower_search_label: rust_i18n::t!("loan.borrower_search", locale = loc).to_string(),
        register_label: rust_i18n::t!("loan.register", locale = loc).to_string(),
        col_borrower: rust_i18n::t!("loan.col_borrower", locale = loc).to_string(),
        col_volume: rust_i18n::t!("loan.col_volume", locale = loc).to_string(),
        col_title: rust_i18n::t!("loan.col_title", locale = loc).to_string(),
        col_date: rust_i18n::t!("loan.col_date", locale = loc).to_string(),
        col_duration: rust_i18n::t!("loan.col_duration", locale = loc).to_string(),
        days_label: rust_i18n::t!("loan.days", locale = loc).to_string(),
        scan_placeholder: rust_i18n::t!("loan.scan_placeholder", locale = loc).to_string(),
        empty_heading: rust_i18n::t!("empty.loans_heading", locale = loc).to_string(),
        empty_body: rust_i18n::t!("empty.loans_body", locale = loc).to_string(),
        prev_label: rust_i18n::t!("pagination.previous", locale = loc).to_string(),
        next_label: rust_i18n::t!("pagination.next", locale = loc).to_string(),
        pagination_aria: rust_i18n::t!("pagination.aria_label", locale = loc).to_string(),
        return_label: rust_i18n::t!("loan.return", locale = loc).to_string(),
        overdue_label: rust_i18n::t!("loan.overdue", locale = loc).to_string(),
        col_action: rust_i18n::t!("loan.col_action", locale = loc).to_string(),
        overdue_threshold: threshold as i64,
        current_sort,
        current_dir,
        loans,
        highlight_loan_id: None,
        current_url: base.current_url,
        lang_toggle_aria: base.lang_toggle_aria,
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
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    axum::Form(form): axum::Form<CreateLoanForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    // Trim volume label to handle whitespace from form input
    let volume_label = form.volume_label.trim().to_uppercase();

    // Look up volume by label
    let volume = match VolumeModel::find_by_label(pool, &volume_label).await? {
        Some(v) => v,
        None if is_htmx => {
            let message = rust_i18n::t!("loan.volume_not_found", locale = loc).to_string();
            let feedback = crate::routes::catalog::feedback_html_pub("error", &message, "");
            return Ok(Html(feedback).into_response());
        }
        None => {
            return Err(AppError::BadRequest(
                rust_i18n::t!("loan.volume_not_found", locale = loc).to_string(),
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
                locale = loc,
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

/// `POST /loans/:id/return`. Reachable from the loans table button, the
/// borrower-detail active-loans table, and the scan-card — all of which
/// route through the `GET /loans/:id/return-modal` confirmation modal
/// (story 9-11). Server contract is unchanged from pre-9-11; only the
/// trigger UX migrated from `hx-confirm=` to the UX-DR8 Modal.
pub async fn return_loan_handler(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(loan_id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let (label, path) = LoanService::return_loan(pool, loan_id).await?;

    let message = match path {
        Some(ref p) => {
            rust_i18n::t!("loan.returned", locale = loc, label = label, path = p).to_string()
        }
        None => {
            rust_i18n::t!("loan.returned_no_location", locale = loc, label = label).to_string()
        }
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

// ─── Return loan — confirmation modal (story 9-11) ──────

/// Parse `HX-Current-URL` (the document's full URL the modal trigger
/// came from) and extract a same-origin path-and-query suitable for use
/// as a post-login `next=` return target. Returns `None` when the URL
/// can't be parsed or fails the [`crate::error::is_safe_next`] guard
/// (defends against open-redirect when the header is attacker-controlled).
fn extract_safe_path(current_url: &str) -> Option<String> {
    use axum::http::Uri;
    let uri: Uri = current_url.parse().ok()?;
    let p = uri.path_and_query()?.as_str().to_string();
    if crate::error::is_safe_next(&p) {
        Some(p)
    } else {
        None
    }
}

/// Closed allowlist of feedback-target IDs the modal is allowed to render
/// into. Three surfaces today: the `/loans` table feedback area, the
/// `/borrower/:id` active-loans feedback area, and the loans-page V-code
/// scan-card. A future surface adds a single entry. The allowlist is
/// security-load-bearing — without it, a crafted `?target=evil-injected`
/// would let an attacker steer the server's feedback HTML into any DOM
/// node of their choosing.
const FEEDBACK_TARGETS: &[&str] = &["loan-feedback", "borrower-feedback", "scan-result"];
const DEFAULT_FEEDBACK_TARGET: &str = "loan-feedback";

#[derive(Deserialize)]
pub struct ReturnModalQuery {
    pub target: Option<String>,
}

#[derive(Template)]
#[template(path = "fragments/return_loan_modal.html")]
pub struct ReturnLoanModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
    pub hx_target: String,
}

/// `GET /loans/:id/return-modal` — renders the UX-DR8 Modal fragment for
/// the return-loan flow. Librarian or Admin only. Direct browser
/// navigation (no `HX-Request`) returns 405 — the fragment is meaningless
/// without page context.
pub async fn return_modal_handler(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    headers: axum::http::HeaderMap,
    Path(loan_id): Path<u64>,
    Query(query): Query<ReturnModalQuery>,
) -> Result<Response, AppError> {
    // Fix #133: derive the post-login return path from HX-Current-URL when
    // available so anonymous users sent through /login bounce back to the
    // surface they were on (/borrower/:id, /loans, etc.) instead of being
    // hard-coded to /loans. Falls back to /loans on missing/invalid header.
    let return_path = headers
        .get("hx-current-url")
        .and_then(|v| v.to_str().ok())
        .and_then(extract_safe_path)
        .unwrap_or_else(|| "/loans".to_string());
    session.require_role_with_return(Role::Librarian, &return_path, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    if !is_htmx {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }

    // Resolve the feedback target FIRST so we can route both the
    // missing-row (#136) response and the modal template to the right
    // slot (`?target=` query, allowlisted to FEEDBACK_TARGETS).
    let target = match query.target.as_deref() {
        Some(t) if FEEDBACK_TARGETS.contains(&t) => t,
        _ => DEFAULT_FEEDBACK_TARGET,
    };
    let hx_target = format!("#{target}");

    // CR #136: when the loan row is gone (another librarian returned
    // and deleted it from a parallel tab), don't 404 silently. The
    // default `AppError::NotFound` retargets to `#feedback-list`, which
    // /loans + /borrower/:id don't declare. Return 200 + inline
    // feedback + HX-Retarget to the resolved page-specific slot so the
    // user actually sees what happened.
    //
    // 409 Conflict on "already returned" is preserved — the row exists
    // but state forbids the action, AppError::Conflict has its own
    // retarget semantics that DO work on these pages.
    let loan = match LoanModel::find_by_id(pool, loan_id).await? {
        Some(l) => l,
        None => {
            return Ok(crate::routes::build_already_deleted_response(loc, &hx_target));
        }
    };
    if loan.returned_at.is_some() {
        return Err(AppError::Conflict(
            rust_i18n::t!("loan.already_returned", locale = loc).to_string(),
        ));
    }

    let title = rust_i18n::t!("loan.return_modal_title", locale = loc).to_string();
    let body_text = rust_i18n::t!("loan.return_modal_body", locale = loc).to_string();
    let body_html = format!("<p>{}</p>", crate::utils::html_escape(&body_text));

    tracing::debug!(loan_id, target, "return modal requested");

    let template = ReturnLoanModalTemplate {
        title,
        body_html,
        confirm_label: rust_i18n::t!("loan.return_modal_confirm", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/loans/{loan_id}/return"),
        csrf_token: session.csrf_token.clone(),
        hx_target,
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => Err(AppError::Internal(format!(
            "return loan modal render: {e}"
        ))),
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
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(_is_htmx): HxRequest,
    axum::extract::Query(params): axum::extract::Query<ScanQuery>,
) -> Result<impl IntoResponse, AppError> {
    // Strip query string from `next` — no point replaying a failed scan after login,
    // and the user-supplied `?code=` shouldn't be reflected into the login form.
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;
    let code = params.code.trim().to_uppercase();

    // Check if V-code format
    if !crate::services::volume::VolumeService::validate_vcode(&code) {
        let message = rust_i18n::t!("feedback.vcode_invalid", locale = loc).to_string();
        return Ok(Html(crate::routes::catalog::feedback_html_pub(
            "warning", &message, "",
        ))
        .into_response());
    }

    // Check if volume exists
    let volume = VolumeModel::find_by_label(pool, &code).await?;
    if volume.is_none() {
        let message = rust_i18n::t!("loan.volume_not_found", locale = loc).to_string();
        return Ok(Html(crate::routes::catalog::feedback_html_pub(
            "warning", &message, "",
        ))
        .into_response());
    }

    // Check if volume is on loan
    match LoanModel::find_active_by_volume_label(pool, &code).await? {
        Some(loan_detail) => {
            // Return highlighted loan row
            let row_html = loan_row_html(&loan_detail, true, loc);
            Ok(Html(row_html).into_response())
        }
        None => {
            let message = rust_i18n::t!("loan.not_on_loan", locale = loc).to_string();
            Ok(Html(crate::routes::catalog::feedback_html_pub(
                "info", &message, "",
            ))
            .into_response())
        }
    }
}

/// Render a loan match result card (for scan-to-find on /loans page).
/// The Return button routes through the `GET /loans/:id/return-modal`
/// confirmation modal (story 9-11) — the `?target=scan-result` param
/// tells the modal to send the success feedback back into this card's
/// own slot, replacing it with the post-return feedback message.
fn loan_row_html(loan: &LoanWithDetails, highlight: bool, loc: &str) -> String {
    let bg = if highlight {
        "bg-yellow-50 dark:bg-yellow-900/20 border-yellow-400"
    } else {
        "bg-stone-50 dark:bg-stone-800 border-stone-300 dark:border-stone-600"
    };
    let escaped_borrower = crate::utils::html_escape(&loan.borrower_name);
    let escaped_label = crate::utils::html_escape(&loan.volume_label);
    let escaped_title = crate::utils::html_escape(&loan.title_name);
    let date = loan.loaned_at.format("%Y-%m-%d").to_string();
    let days = rust_i18n::t!("loan.days", locale = loc).to_string();
    let return_label = rust_i18n::t!("loan.return", locale = loc).to_string();

    format!(
        r##"<div class="p-3 rounded-md border {bg}" id="scan-loan-{id}">
            <p class="font-medium text-stone-900 dark:text-stone-100">{label} — {title}</p>
            <p class="text-sm text-stone-600 dark:text-stone-400">
                <a href="/borrower/{bid}" class="text-indigo-600 hover:underline dark:text-indigo-400">{borrower}</a>
                · {date} · {duration} {days}
            </p>
            <button hx-get="/loans/{id}/return-modal?target=scan-result"
                    hx-target="#modal-slot"
                    hx-swap="innerHTML"
                    hx-disabled-elt="this"
                    data-modal-trigger
                    aria-haspopup="dialog"
                    aria-expanded="false"
                    class="mt-2 px-3 py-1 text-sm font-medium text-white bg-indigo-600 rounded hover:bg-indigo-700 disabled:opacity-50">
                {return_label}
            </button>
        </div>"##,
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
    )
}

// ─── #340 — TEST_MODE seed-overdue-loan ─────────────────────────
//
// Mirror of `catalog::debug_set_session_timeout` — same TEST_MODE
// gate + Admin role enforcement. Lets the home E2E spec
// (`tests/e2e/specs/journeys/home.spec.ts`) create a loan whose
// `loaned_at` is already backdated past the overdue threshold,
// which UI-only seeding (POST /loans + the catalog scan path)
// cannot do because they always insert `loaned_at = NOW()`.
//
// NEVER enable `TEST_MODE=1` in production: combined with stolen
// admin credentials this lets a caller fabricate past-loan
// history at will.
#[derive(Deserialize)]
pub struct SeedOverdueLoanForm {
    /// Volume label (V-code, e.g. "V0042") — looked up to volume id
    /// so the Playwright caller passes the same label it already used
    /// in `scanTitleAndVolume`, no UI scraping for ids.
    pub volume_label: String,
    /// Borrower name (exact match) — same lookup convenience as above.
    pub borrower_name: String,
    pub days_overdue: u32,
}

pub async fn debug_seed_overdue_loan(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<SeedOverdueLoanForm>,
) -> Result<impl IntoResponse, AppError> {
    if std::env::var("TEST_MODE").as_deref() != Ok("1") {
        return Err(AppError::NotFound("disabled".to_string()));
    }
    session.require_role(Role::Admin, locale.0)?;
    if form.days_overdue == 0 {
        return Err(AppError::BadRequest(
            "days_overdue must be >= 1".to_string(),
        ));
    }
    // Resolve volume + borrower by their label/name. Fail-loud with a
    // clear error so a typo in the test surfaces here instead of as
    // a silent FK violation downstream.
    let volume_id: u64 = sqlx::query_scalar(
        "SELECT id FROM volumes WHERE label = ? AND deleted_at IS NULL LIMIT 1",
    )
    .bind(&form.volume_label)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| {
        AppError::BadRequest(format!("volume not found: {}", form.volume_label))
    })?;
    let borrower_id: u64 = sqlx::query_scalar(
        "SELECT id FROM borrowers WHERE name = ? AND deleted_at IS NULL LIMIT 1",
    )
    .bind(&form.borrower_name)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| {
        AppError::BadRequest(format!("borrower not found: {}", form.borrower_name))
    })?;
    // Direct INSERT bypassing LoanService — we WANT a backdated
    // loaned_at, which `register_loan` cannot produce. Mirror of
    // the pattern used in `models::loan` integration tests
    // (line 749-756) that already exercises this shape.
    let result = sqlx::query(
        "INSERT INTO loans (volume_id, borrower_id, loaned_at) \
         VALUES (?, ?, NOW() - INTERVAL ? DAY)",
    )
    .bind(volume_id)
    .bind(borrower_id)
    .bind(form.days_overdue)
    .execute(&state.pool)
    .await
    .map_err(|e| AppError::Internal(format!("Failed to seed overdue loan: {e}")))?;
    let loan_id = result.last_insert_id();
    tracing::warn!(
        loan_id = loan_id,
        volume_id = volume_id,
        borrower_id = borrower_id,
        days_overdue = form.days_overdue,
        user_id = session.user_id,
        "TEST_MODE seed-overdue-loan inserted"
    );
    Ok(axum::http::StatusCode::OK)
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

    // ─── #133: extract_safe_path coverage ───────────────

    #[test]
    fn extract_safe_path_accepts_absolute_url() {
        assert_eq!(
            extract_safe_path("http://example.com/borrower/42"),
            Some("/borrower/42".to_string())
        );
    }

    #[test]
    fn extract_safe_path_preserves_query() {
        assert_eq!(
            extract_safe_path("https://example.com/loans?page=2&sort=title"),
            Some("/loans?page=2&sort=title".to_string())
        );
    }

    #[test]
    fn extract_safe_path_accepts_bare_path() {
        // HTMX always sends an absolute URL today, but accept a bare path
        // defensively so we don't regress on clients that emit one.
        assert_eq!(
            extract_safe_path("/borrower/7"),
            Some("/borrower/7".to_string())
        );
    }

    #[test]
    fn extract_safe_path_rejects_protocol_relative() {
        assert_eq!(extract_safe_path("//evil.example.com/path"), None);
    }

    #[test]
    fn extract_safe_path_rejects_unparseable() {
        assert_eq!(extract_safe_path(""), None);
        assert_eq!(extract_safe_path("not a url"), None);
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
        let html = loan_row_html(&loan, true, "en");
        assert!(html.contains("bg-yellow-50"));
        assert!(html.contains("border-yellow-400"));
        assert!(html.contains("Jean"));
        assert!(html.contains("V0042"));
        assert!(html.contains("Test Book"));
        // Story 9-11: scan-card Return button now opens the confirmation
        // modal instead of POSTing directly.
        assert!(html.contains("hx-get=\"/loans/1/return-modal?target=scan-result\""));
        assert!(html.contains("data-modal-trigger"));
        assert!(!html.contains("hx-confirm="));
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
        let html = loan_row_html(&loan, false, "en");
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
        let html = loan_row_html(&loan, false, "en");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&amp;"));
    }

    /// Story 10-3: build a minimal LoansTemplate with one loan for
    /// the mobile-cards rendering tests below.
    fn build_loans_template_for_test(loan: LoanWithDetails) -> LoansTemplate {
        use crate::models::PaginatedList;
        LoansTemplate {
            lang: "en".to_string(),
            role: "librarian".to_string(),
            current_page: "loans",
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
            list_title: "Loans".to_string(),
            new_loan_label: "New".to_string(),
            volume_label_label: "V-code".to_string(),
            borrower_label: "Borrower".to_string(),
            borrower_search_label: "Search".to_string(),
            register_label: "Register".to_string(),
            col_borrower: "Borrower".to_string(),
            col_volume: "V-code".to_string(),
            col_title: "Title".to_string(),
            col_date: "Date".to_string(),
            col_duration: "Duration".to_string(),
            days_label: "days".to_string(),
            scan_placeholder: "Scan".to_string(),
            empty_heading: "No active loans".to_string(),
            empty_body: "Create one".to_string(),
            prev_label: "Previous".to_string(),
            next_label: "Next".to_string(),
            pagination_aria: "Pagination".to_string(),
            return_label: "Return".to_string(),
            overdue_label: "Overdue".to_string(),
            col_action: "Action".to_string(),
            overdue_threshold: 30,
            current_sort: "date".to_string(),
            current_dir: "desc".to_string(),
            loans: PaginatedList::new(vec![loan], 1, 1, None, Some("date".to_string()), Some("desc".to_string())),
            highlight_loan_id: None,
            current_url: "/loans".to_string(),
            lang_toggle_aria: "Change language".to_string(),
        }
    }

    /// Story 10-3: the loans page must render BOTH the mobile-card list
    /// (md:hidden) AND the desktop table (hidden md:block), each carrying
    /// the same loan data, so the layout switches purely via the Tailwind
    /// breakpoint — no JS, no duplicated data fetch.
    #[test]
    fn loans_template_renders_both_mobile_cards_and_desktop_table() {
        let loan = LoanWithDetails {
            id: 7,
            volume_id: 100,
            borrower_id: 200,
            borrower_name: "Mobile-Card-User".to_string(),
            volume_label: "V0007".to_string(),
            title_name: "Mobile Test Title".to_string(),
            loaned_at: chrono::NaiveDate::from_ymd_opt(2026, 5, 14)
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap(),
            duration_days: 2,
        };
        let html = build_loans_template_for_test(loan).render().unwrap();

        // Mobile-cards surface
        assert!(
            html.contains(r#"id="loans-cards-mobile""#),
            "expected mobile-cards container"
        );
        assert!(
            html.contains(r#"id="loan-card-7""#),
            "expected per-loan card with id"
        );
        assert!(
            html.contains(r#"class="md:hidden"#),
            "mobile-cards container must be md:hidden"
        );

        // Desktop table surface
        assert!(
            html.contains(r#"class="hidden md:block overflow-x-auto""#),
            "desktop table wrapper must be hidden md:block"
        );
        assert!(
            html.contains(r#"id="loan-row-7""#),
            "expected per-loan row with id (desktop)"
        );

        // Same data in both: borrower name, V-code, title, days
        assert_eq!(
            html.matches("Mobile-Card-User").count(),
            2,
            "borrower name must appear in both card and row"
        );
        assert_eq!(
            html.matches("V0007").count(),
            2,
            "volume label must appear in both surfaces"
        );
    }
}
