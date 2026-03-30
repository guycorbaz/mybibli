use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::models::session::SessionModel;
use crate::services::title::{TitleForm, TitleService};
use crate::AppState;

// ─── Feedback entry helpers ───────────────────────────────────────

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn feedback_html(variant: &str, message: &str, suggestion: &str) -> String {
    let (border_color, bg_color, icon_color, icon_path) = match variant {
        "success" => (
            "border-green-500", "bg-green-50 dark:bg-green-900/20", "text-green-600 dark:text-green-400",
            r#"<path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.857-9.809a.75.75 0 00-1.214-.882l-3.483 4.79-1.88-1.88a.75.75 0 10-1.06 1.061l2.5 2.5a.75.75 0 001.137-.089l4-5.5z" clip-rule="evenodd" />"#,
        ),
        "info" => (
            "border-blue-500", "bg-blue-50 dark:bg-blue-900/20", "text-blue-600 dark:text-blue-400",
            r#"<path fill-rule="evenodd" d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7-4a1 1 0 11-2 0 1 1 0 012 0zM9 9a.75.75 0 000 1.5h.253a.25.25 0 01.244.304l-.459 2.066A1.75 1.75 0 0010.747 15H11a.75.75 0 000-1.5h-.253a.25.25 0 01-.244-.304l.459-2.066A1.75 1.75 0 009.253 9H9z" clip-rule="evenodd" />"#,
        ),
        "warning" => (
            "border-amber-500", "bg-amber-50 dark:bg-amber-900/20", "text-amber-600 dark:text-amber-400",
            r#"<path fill-rule="evenodd" d="M8.485 2.495c.673-1.167 2.357-1.167 3.03 0l6.28 10.875c.673 1.167-.17 2.625-1.516 2.625H3.72c-1.347 0-2.189-1.458-1.515-2.625L8.485 2.495zM10 5a.75.75 0 01.75.75v3.5a.75.75 0 01-1.5 0v-3.5A.75.75 0 0110 5zm0 9a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd" />"#,
        ),
        _ => (
            "border-red-500", "bg-red-50 dark:bg-red-900/20", "text-red-600 dark:text-red-400",
            r#"<path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zM8.28 7.22a.75.75 0 00-1.06 1.06L8.94 10l-1.72 1.72a.75.75 0 101.06 1.06L10 11.06l1.72 1.72a.75.75 0 101.06-1.06L11.06 10l1.72-1.72a.75.75 0 00-1.06-1.06L10 8.94 8.28 7.22z" clip-rule="evenodd" />"#,
        ),
    };

    let suggestion_html = if suggestion.is_empty() {
        String::new()
    } else {
        format!(
            r#"<p class="text-sm text-stone-500 dark:text-stone-400 mt-1">{}</p>"#,
            html_escape(suggestion)
        )
    };

    let dismiss_html = if variant == "warning" || variant == "error" {
        r#"<button type="button" class="text-stone-400 hover:text-stone-600 dark:hover:text-stone-200 p-1 min-w-[44px] min-h-[44px] md:min-w-[36px] md:min-h-[36px] flex items-center justify-center" aria-label="Dismiss" onclick="this.closest('.feedback-entry').remove()"><svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z" /></svg></button>"#
    } else {
        ""
    };

    format!(
        r#"<div class="p-3 border-l-4 {} {} rounded-r feedback-entry" role="status" data-feedback-variant="{}">
  <div class="flex items-start gap-2">
    <svg class="{} w-5 h-5 flex-shrink-0 mt-0.5" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">{}</svg>
    <div class="flex-1">
      <p class="text-stone-700 dark:text-stone-300">{}</p>
      {}
    </div>
    {}
  </div>
</div>"#,
        border_color, bg_color, variant, icon_color, icon_path,
        html_escape(message), suggestion_html, dismiss_html
    )
}

fn context_banner_html(title_name: &str, media_type: &str) -> String {
    let label = rust_i18n::t!("title.current_banner", title = title_name).to_string();
    format!(
        r##"<div class="flex items-center gap-2 px-3 py-2 bg-indigo-50 dark:bg-indigo-900/20 border border-indigo-200 dark:border-indigo-800 rounded-md text-sm">
  <img src="/static/icons/{}.svg" alt="" class="w-5 h-5" aria-hidden="true">
  <span class="text-stone-700 dark:text-stone-300">
    <a href="#" class="font-medium text-indigo-600 dark:text-indigo-400 hover:underline">{}</a>
  </span>
</div>"##,
        html_escape(media_type),
        html_escape(&label)
    )
}

// ─── Catalog page ─────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/catalog.html")]
pub struct CatalogTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub catalog_title: String,
    pub scan_label: String,
    pub scan_placeholder: String,
    pub isbn_error: String,
    pub new_title_label: String,
}

impl CatalogTemplate {
    fn from_session(session: &Session) -> Self {
        CatalogTemplate {
            lang: rust_i18n::locale().to_string(),
            role: session.role.to_string(),
            current_page: "catalog",
            skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
            nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
            nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_admin: rust_i18n::t!("nav.admin").to_string(),
            nav_login: rust_i18n::t!("nav.login").to_string(),
            nav_logout: rust_i18n::t!("nav.logout").to_string(),
            catalog_title: rust_i18n::t!("catalog.title").to_string(),
            scan_label: rust_i18n::t!("catalog.scan_label").to_string(),
            scan_placeholder: rust_i18n::t!("catalog.scan_placeholder").to_string(),
            isbn_error: rust_i18n::t!("feedback.isbn_invalid").to_string(),
            new_title_label: rust_i18n::t!("catalog.new_title_button").to_string(),
        }
    }
}

pub async fn catalog_page(session: Session) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let template = CatalogTemplate::from_session(&session);
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render catalog template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

// ─── Scan handler ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ScanForm {
    pub code: String,
}

fn detect_code_type(code: &str) -> &'static str {
    if code.starts_with('V') && code.len() == 5 { return "vcode"; }
    if code.starts_with('L') && code.len() == 5 { return "lcode"; }
    if (code.starts_with("978") || code.starts_with("979")) && code.len() == 13 { return "isbn"; }
    if code.starts_with("977") && code.len() >= 8 && code.len() <= 13 { return "issn"; }
    if code.chars().all(|c| c.is_ascii_digit()) && code.len() >= 8 && code.len() <= 13 { return "upc"; }
    "unknown"
}

pub async fn handle_scan(
    session: Session,
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<ScanForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let code = form.code.trim().to_string();
    if code.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.bad_request").to_string(),
        ));
    }

    let code_type = detect_code_type(&code);
    tracing::info!(code = %code, code_type = code_type, "Processing scan");

    if is_htmx {
        let pool = &state.pool;

        match code_type {
            "isbn" => {
                match TitleService::create_from_isbn(pool, &code, session.token.as_deref()).await {
                    Ok((title, is_new)) => {
                        // Update current title in session
                        if let Some(token) = &session.token
                            && let Err(e) = SessionModel::set_current_title(pool, token, title.id).await
                        {
                            tracing::warn!(error = %e, "Failed to update current title in session");
                        }

                        let (variant, message, suggestion) = if is_new {
                            (
                                "success",
                                rust_i18n::t!("feedback.title_created").to_string(),
                                rust_i18n::t!("feedback.title_created_suggestion").to_string(),
                            )
                        } else {
                            (
                                "info",
                                rust_i18n::t!("feedback.title_exists").to_string(),
                                rust_i18n::t!("feedback.title_exists_suggestion").to_string(),
                            )
                        };

                        let resp = HtmxResponse {
                            main: feedback_html(variant, &message, &suggestion),
                            oob: vec![OobUpdate {
                                target: "context-banner".to_string(),
                                content: context_banner_html(&title.title, &title.media_type),
                            }],
                        };
                        Ok(resp.into_response())
                    }
                    Err(e) => {
                        tracing::error!(error = %e, code = %code, "ISBN scan failed");
                        let message = rust_i18n::t!("error.title.creation_failed").to_string();
                        Ok(Html(feedback_html("error", &message, "")).into_response())
                    }
                }
            }
            "vcode" | "lcode" => {
                // Stub behavior from Story 1-2 — return scan received feedback
                let escaped_code = html_escape(&code);
                let message = format!(
                    "{}: {}",
                    rust_i18n::t!("catalog.scan_received"),
                    escaped_code
                );
                Ok(Html(feedback_html("info", &message, "")).into_response())
            }
            _ => {
                // ISSN, UPC, unknown → amber warning
                let message = rust_i18n::t!("feedback.code_unsupported").to_string();
                Ok(Html(feedback_html("warning", &message, "")).into_response())
            }
        }
    } else {
        let template = CatalogTemplate::from_session(&session);
        match template.render() {
            Ok(html) => Ok(Html(html).into_response()),
            Err(e) => {
                tracing::error!(error = %e, "Failed to render catalog template");
                Err(AppError::Internal("Template rendering failed".to_string()))
            }
        }
    }
}

// ─── Title form routes ────────────────────────────────────────────

struct GenreOption {
    id: u64,
    name: String,
}

#[derive(Template)]
#[template(path = "components/title_form.html")]
struct TitleFormTemplate {
    form_heading: String,
    label_title: String,
    label_media_type: String,
    label_genre: String,
    label_language: String,
    label_subtitle: String,
    label_publisher: String,
    label_publication_date: String,
    label_isbn: String,
    label_issn: String,
    label_upc: String,
    label_submit: String,
    label_cancel: String,
    mt_book: String,
    mt_bd: String,
    mt_cd: String,
    mt_dvd: String,
    mt_magazine: String,
    mt_report: String,
    genres: Vec<GenreOption>,
    required_error: String,
}

pub async fn title_form_page(
    session: Session,
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let pool = &state.pool;
    let genres = load_genres(pool).await?;

    let template = TitleFormTemplate {
        form_heading: rust_i18n::t!("title.form.heading").to_string(),
        label_title: rust_i18n::t!("title.form.title_label").to_string(),
        label_media_type: rust_i18n::t!("title.form.media_type").to_string(),
        label_genre: rust_i18n::t!("title.form.genre").to_string(),
        label_language: rust_i18n::t!("title.form.language").to_string(),
        label_subtitle: rust_i18n::t!("title.form.subtitle").to_string(),
        label_publisher: rust_i18n::t!("title.form.publisher").to_string(),
        label_publication_date: rust_i18n::t!("title.form.publication_date").to_string(),
        label_isbn: rust_i18n::t!("title.form.isbn").to_string(),
        label_issn: rust_i18n::t!("title.form.issn").to_string(),
        label_upc: rust_i18n::t!("title.form.upc").to_string(),
        label_submit: rust_i18n::t!("title.form.submit").to_string(),
        label_cancel: rust_i18n::t!("title.form.cancel").to_string(),
        mt_book: rust_i18n::t!("title.media_types.book").to_string(),
        mt_bd: rust_i18n::t!("title.media_types.bd").to_string(),
        mt_cd: rust_i18n::t!("title.media_types.cd").to_string(),
        mt_dvd: rust_i18n::t!("title.media_types.dvd").to_string(),
        mt_magazine: rust_i18n::t!("title.media_types.magazine").to_string(),
        mt_report: rust_i18n::t!("title.media_types.report").to_string(),
        genres,
        required_error: rust_i18n::t!("validation.required").to_string(),
    };

    match template.render() {
        Ok(html) => {
            if is_htmx {
                Ok(Html(html).into_response())
            } else {
                // Non-HTMX: wrap in full catalog page
                let catalog = CatalogTemplate::from_session(&session);
                match catalog.render() {
                    Ok(page_html) => Ok(Html(page_html).into_response()),
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to render catalog template");
                        Err(AppError::Internal("Template rendering failed".to_string()))
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to render title form template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

pub async fn create_title(
    session: Session,
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<TitleForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let pool = &state.pool;

    match TitleService::create_manual(pool, &form).await {
        Ok(title) => {
            tracing::info!(title_id = title.id, title = %title.title, "Manual title created");

            // Update current title in session
            if let Some(token) = &session.token
                && let Err(e) = SessionModel::set_current_title(pool, token, title.id).await
            {
                tracing::warn!(error = %e, "Failed to update current title in session");
            }

            if is_htmx {
                let message = rust_i18n::t!("feedback.title_created").to_string();
                let suggestion = rust_i18n::t!("feedback.title_created_suggestion").to_string();

                let resp = HtmxResponse {
                    main: feedback_html("success", &message, &suggestion),
                    oob: vec![
                        OobUpdate {
                            target: "context-banner".to_string(),
                            content: context_banner_html(&title.title, &title.media_type),
                        },
                        OobUpdate {
                            target: "title-form-container".to_string(),
                            content: String::new(), // Close the form
                        },
                    ],
                };
                Ok(resp.into_response())
            } else {
                Ok(axum::response::Redirect::to("/catalog").into_response())
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Manual title creation failed");
            let message = match &e {
                AppError::BadRequest(msg) => msg.clone(),
                _ => rust_i18n::t!("error.title.creation_failed").to_string(),
            };
            Ok(Html(feedback_html("error", &message, "")).into_response())
        }
    }
}

// ─── Type-specific fields route ───────────────────────────────────

#[derive(Template)]
#[template(path = "components/type_specific_fields.html")]
struct TypeSpecificFieldsTemplate {
    show_page_count: bool,
    show_track_count: bool,
    show_total_duration: bool,
    show_age_rating: bool,
    show_issue_number: bool,
    label_page_count: String,
    label_track_count: String,
    label_total_duration: String,
    label_age_rating: String,
    label_issue_number: String,
}

pub async fn type_specific_fields(
    session: Session,
    axum::extract::Path(media_type): axum::extract::Path<String>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let template = TypeSpecificFieldsTemplate {
        show_page_count: matches!(media_type.as_str(), "book" | "bd" | "magazine" | "report"),
        show_track_count: media_type == "cd",
        show_total_duration: matches!(media_type.as_str(), "cd" | "dvd"),
        show_age_rating: media_type == "dvd",
        show_issue_number: media_type == "magazine",
        label_page_count: rust_i18n::t!("title.form.page_count").to_string(),
        label_track_count: rust_i18n::t!("title.form.track_count").to_string(),
        label_total_duration: rust_i18n::t!("title.form.total_duration").to_string(),
        label_age_rating: rust_i18n::t!("title.form.age_rating").to_string(),
        label_issue_number: rust_i18n::t!("title.form.issue_number").to_string(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render type-specific fields template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────

async fn load_genres(pool: &crate::db::DbPool) -> Result<Vec<GenreOption>, AppError> {
    let rows: Vec<(u64, String)> = sqlx::query_as(
        "SELECT id, name FROM genres WHERE deleted_at IS NULL ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name)| GenreOption { id, name })
        .collect())
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_code_type_isbn_978() {
        assert_eq!(detect_code_type("9782070360246"), "isbn");
    }

    #[test]
    fn test_detect_code_type_isbn_979() {
        assert_eq!(detect_code_type("9791032305560"), "isbn");
    }

    #[test]
    fn test_detect_code_type_vcode() {
        assert_eq!(detect_code_type("V0042"), "vcode");
    }

    #[test]
    fn test_detect_code_type_lcode() {
        assert_eq!(detect_code_type("L0001"), "lcode");
    }

    #[test]
    fn test_detect_code_type_issn() {
        assert_eq!(detect_code_type("97712345"), "issn");
    }

    #[test]
    fn test_detect_code_type_upc() {
        assert_eq!(detect_code_type("12345678"), "upc");
    }

    #[test]
    fn test_detect_code_type_unknown() {
        assert_eq!(detect_code_type("ABCDEF"), "unknown");
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape("\"hi\""), "&quot;hi&quot;");
        assert_eq!(html_escape("it's"), "it&#x27;s");
    }

    #[test]
    fn test_feedback_html_success() {
        let html = feedback_html("success", "Title created", "Scan next");
        assert!(html.contains("border-green-500"));
        assert!(html.contains("Title created"));
        assert!(html.contains("Scan next"));
        assert!(html.contains(r#"role="status""#));
    }

    #[test]
    fn test_feedback_html_info() {
        let html = feedback_html("info", "Already exists", "");
        assert!(html.contains("border-blue-500"));
        assert!(html.contains("Already exists"));
    }

    #[test]
    fn test_feedback_html_warning() {
        let html = feedback_html("warning", "Unsupported", "");
        assert!(html.contains("border-amber-500"));
    }

    #[test]
    fn test_feedback_html_error() {
        let html = feedback_html("error", "Failed", "");
        assert!(html.contains("border-red-500"));
    }

    #[test]
    fn test_feedback_html_escapes_message() {
        let html = feedback_html("error", "<script>alert('xss')</script>", "");
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>alert"));
    }

    #[test]
    fn test_catalog_template_renders_for_librarian() {
        let session = Session {
            token: Some("test".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
        };
        let template = CatalogTemplate::from_session(&session);
        let rendered = template.render().unwrap();
        assert!(rendered.contains("scan-field"));
        assert!(rendered.contains("feedback-list"));
        assert!(rendered.contains(r#"data-user-role="librarian""#));
        assert!(rendered.contains("context-banner"));
        assert!(rendered.contains("title-form-container"));
    }

    #[test]
    fn test_catalog_template_shows_catalog_nav_for_librarian() {
        let session = Session {
            token: Some("test".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
        };
        let template = CatalogTemplate::from_session(&session);
        let rendered = template.render().unwrap();
        assert!(rendered.contains(r#"aria-current="page""#));
        assert!(rendered.contains("/catalog"));
    }

    #[test]
    fn test_context_banner_html() {
        let html = context_banner_html("L'Étranger", "book");
        assert!(html.contains("/static/icons/book.svg"));
        // The title goes through t!() then html_escape, so the apostrophe
        // in the i18n label gets escaped
        assert!(html.contains("book.svg"));
        assert!(html.contains("bg-indigo-50"));
    }
}
