//! Manual title form and its type-specific fields.
//!
//! Split out of `routes/catalog.rs` by story 16-1 (#597, Foundation Rule #12).

use askama::Template;
use axum::Extension;
use axum::extract::State;
use axum::response::{Html, IntoResponse};

use crate::AppState;
use crate::routes::catalog::{CatalogTemplate, build_highest_codes_line, context_banner_html};
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::session::SessionModel;
use crate::services::title::{TitleForm, TitleService};
use crate::utils::feedback_html;

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
    Extension(locale): Extension<Locale>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    // Use the pre-nest original URI for BOTH the role-gate return path and
    // the toggle's `current_url` so the two stay aligned under any future
    // router nesting.
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let loc = locale.0;

    let pool = &state.pool;
    let genres = load_genres(pool).await?;

    let template = TitleFormTemplate {
        form_heading: rust_i18n::t!("title.form.heading", locale = loc).to_string(),
        label_title: rust_i18n::t!("title.form.title_label", locale = loc).to_string(),
        label_media_type: rust_i18n::t!("title.form.media_type", locale = loc).to_string(),
        label_genre: rust_i18n::t!("title.form.genre", locale = loc).to_string(),
        label_language: rust_i18n::t!("title.form.language", locale = loc).to_string(),
        label_subtitle: rust_i18n::t!("title.form.subtitle", locale = loc).to_string(),
        label_publisher: rust_i18n::t!("title.form.publisher", locale = loc).to_string(),
        label_publication_date: rust_i18n::t!("title.form.publication_date", locale = loc).to_string(),
        label_isbn: rust_i18n::t!("title.form.isbn", locale = loc).to_string(),
        label_issn: rust_i18n::t!("title.form.issn", locale = loc).to_string(),
        label_upc: rust_i18n::t!("title.form.upc", locale = loc).to_string(),
        label_submit: rust_i18n::t!("title.form.submit", locale = loc).to_string(),
        label_cancel: rust_i18n::t!("title.form.cancel", locale = loc).to_string(),
        mt_book: rust_i18n::t!("title.media_types.book", locale = loc).to_string(),
        mt_bd: rust_i18n::t!("title.media_types.bd", locale = loc).to_string(),
        mt_cd: rust_i18n::t!("title.media_types.cd", locale = loc).to_string(),
        mt_dvd: rust_i18n::t!("title.media_types.dvd", locale = loc).to_string(),
        mt_magazine: rust_i18n::t!("title.media_types.magazine", locale = loc).to_string(),
        mt_report: rust_i18n::t!("title.media_types.report", locale = loc).to_string(),
        genres,
        required_error: rust_i18n::t!("validation.required", locale = loc).to_string(),
    };

    match template.render() {
        Ok(html) => {
            if is_htmx {
                Ok(Html(html).into_response())
            } else {
                // Non-HTMX: wrap in full catalog page
                let catalog = CatalogTemplate::new(
                    &session,
                    "",
                    state.page_settings(),
                    loc,
                    crate::utils::current_url(&uri),
                    build_highest_codes_line(&state.pool, loc).await,
                );
                match catalog.render() {
                    Ok(page_html) => Ok(Html(page_html).into_response()),
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to render catalog template");
                        Err(AppError::TemplateRenderFailed { template: "catalog" })
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to render title form template");
            Err(AppError::TemplateRenderFailed { template: "catalog" })
        }
    }
}

pub async fn create_title(
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<TitleForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let loc = locale.0;

    let pool = &state.pool;

    match TitleService::create_manual(pool, &form, loc).await {
        Ok(title) => {
            tracing::info!(title_id = title.id, title = %title.title, "Manual title created");

            // Update current title in session
            if let Some(token) = &session.token
                && let Err(e) = SessionModel::set_current_title(pool, token, title.id).await
            {
                tracing::warn!(error = %e, "Failed to update current title in session");
            }

            // Story 11-10 — an ISBN settles expected tomes as a scan does.
            let banners = crate::routes::catalog_isbn_banners::isbn_banners(
                pool, title.id, title.isbn.as_deref(), loc,
            )
            .await;

            if is_htmx {
                let message = rust_i18n::t!("feedback.title_created", locale = loc).to_string();
                let suggestion =
                    rust_i18n::t!("feedback.title_created_suggestion", locale = loc).to_string();

                let resp = HtmxResponse {
                    main: feedback_html("success", &message, &suggestion)
                        + &banners
                        + &crate::services::tome::manual_title_hint_html(&title.title, &title.media_type, loc),
                    oob: vec![
                        OobUpdate { swap_mode: Default::default(),
                            target: "context-banner".to_string(),
                            content: context_banner_html(&title.title, &title.media_type, 0, None),
                        },
                        OobUpdate { swap_mode: Default::default(),
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
                AppError::BadRequest(msg) | AppError::Conflict(msg) => msg.clone(),
                _ => rust_i18n::t!("error.title.creation_failed", locale = loc).to_string(),
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
    Extension(locale): Extension<Locale>,
    uri: axum::http::Uri,
    axum::extract::Path(media_type): axum::extract::Path<String>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;

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
            Err(AppError::TemplateRenderFailed { template: "catalog" })
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────

async fn load_genres(pool: &crate::db::DbPool) -> Result<Vec<GenreOption>, AppError> {
    let rows: Vec<(u64, String)> =
        sqlx::query_as("SELECT id, name FROM genres WHERE deleted_at IS NULL ORDER BY name")
            .fetch_all(pool)
            .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name)| GenreOption { id, name })
        .collect())
}
