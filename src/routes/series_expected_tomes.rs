//! Story 11-11 (FR142, FR147, UX-DR34) — expected tomes on the series
//! page: add / edit forms, UX-DR8 delete modal, "Add to wishlist".
//!
//! Every handler here is Librarian-gated; the series page that lists the
//! tomes stays anonymous-readable (`routes::series::series_detail_page`).
//! Refusals use the localized `ExpectedTomeError` messages (D-11-11-5).

use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::expected_tome::{ExpectedTomeOrigin, ExpectedTomeRow};
use crate::models::series::SeriesModel;
use crate::models::wishlist::{WishlistItemDraft, WishlistItemModel};
use crate::services::expected_tomes::{ExpectedTomeError, ExpectedTomeInput, ExpectedTomeService};

// ─── Form page ──────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/expected_tome_form.html")]
pub struct ExpectedTomeFormTemplate {
    pub base: crate::utils::BaseContextFields,
    pub heading: String,
    pub series_id: u64,
    pub series_name: String,
    /// `Some(id)` when editing.
    pub tome_id: Option<u64>,
    pub version: i32,
    pub position_value: String,
    pub title_value: String,
    pub isbn_value: String,
    /// The localized refusal of the last submit.
    pub error: Option<String>,
    pub label_position: String,
    pub label_title: String,
    pub label_isbn: String,
    pub isbn_help: String,
    pub label_save: String,
    pub label_cancel: String,
    pub label_back: String,
}

/// What the form shows: typed (or stored) values and an optional error.
struct FormState {
    tome_id: Option<u64>,
    version: i32,
    position: String,
    title: String,
    isbn: String,
    error: Option<String>,
}

fn render_form(
    state: &AppState,
    session: &Session,
    loc: &'static str,
    uri: &axum::http::Uri,
    series: &SeriesModel,
    form: FormState,
    status: StatusCode,
) -> Result<Response, AppError> {
    let heading_key = if form.tome_id.is_some() {
        "expected_tome.form_title_edit"
    } else {
        "expected_tome.form_title_new"
    };
    let template = ExpectedTomeFormTemplate {
        base: crate::utils::base_context(session, loc, "series", uri, state.session_timeout_secs()),
        heading: rust_i18n::t!(heading_key, locale = loc).to_string(),
        series_id: series.id,
        series_name: series.name.clone(),
        tome_id: form.tome_id,
        version: form.version,
        position_value: form.position,
        title_value: form.title,
        isbn_value: form.isbn,
        error: form.error,
        label_position: rust_i18n::t!("expected_tome.field_position", locale = loc).to_string(),
        label_title: rust_i18n::t!("expected_tome.field_title", locale = loc).to_string(),
        label_isbn: rust_i18n::t!("expected_tome.field_isbn", locale = loc).to_string(),
        isbn_help: rust_i18n::t!("expected_tome.isbn_help", locale = loc).to_string(),
        label_save: rust_i18n::t!("expected_tome.save", locale = loc).to_string(),
        label_cancel: rust_i18n::t!("expected_tome.cancel", locale = loc).to_string(),
        label_back: rust_i18n::t!("expected_tome.back", locale = loc).to_string(),
    };
    match template.render() {
        Ok(html) => Ok((status, Html(html)).into_response()),
        Err(_) => Err(AppError::TemplateRenderFailed {
            template: "expected_tome_form",
        }),
    }
}

async fn live_series(state: &AppState, id: u64, loc: &str) -> Result<SeriesModel, AppError> {
    SeriesModel::active_find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
        })
}

/// A live tome of the series in the path; another series' tome is a 404.
async fn tome_of_series(
    state: &AppState,
    series_id: u64,
    tome_id: u64,
    loc: &str,
) -> Result<ExpectedTomeRow, AppError> {
    match ExpectedTomeService::find(&state.pool, tome_id).await? {
        Some(t) if t.series_id == series_id => Ok(t),
        _ => Err(ExpectedTomeError::NotFound { id: tome_id }.into_app_error(loc)),
    }
}

#[derive(Deserialize, Default)]
pub struct NewTomeQuery {
    #[serde(default)]
    pub position: Option<String>,
}

/// `GET /series/{id}/expected-tomes/new[?position=N]`.
pub async fn new_form(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<u64>,
    Query(query): Query<NewTomeQuery>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role_with_return(Role::Librarian, uri.path(), loc)?;
    let series = live_series(&state, id, loc).await?;
    // Only a positive integer pre-fills the field (the link from a gap).
    let position = query
        .position
        .as_deref()
        .and_then(|p| p.trim().parse::<i32>().ok())
        .filter(|p| *p >= 1)
        .map(|p| p.to_string())
        .unwrap_or_default();
    let form = FormState {
        tome_id: None,
        version: 0,
        position,
        title: String::new(),
        isbn: String::new(),
        error: None,
    };
    render_form(&state, &session, loc, &uri, &series, form, StatusCode::OK)
}

/// `GET /series/{id}/expected-tomes/{tome_id}/edit`.
pub async fn edit_form(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path((id, tome_id)): Path<(u64, u64)>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role_with_return(Role::Librarian, uri.path(), loc)?;
    let series = live_series(&state, id, loc).await?;
    let tome = tome_of_series(&state, id, tome_id, loc).await?;
    let form = FormState {
        tome_id: Some(tome.id),
        version: tome.version,
        position: tome.position.to_string(),
        title: tome.title,
        isbn: tome.isbn.unwrap_or_default(),
        error: None,
    };
    render_form(&state, &session, loc, &uri, &series, form, StatusCode::OK)
}

#[derive(Deserialize)]
pub struct TomeForm {
    #[serde(default)]
    pub position: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub isbn: Option<String>,
    #[serde(default)]
    pub version: Option<i32>,
}

/// A position field that is not an integer reads as "position must be
/// 1 or more" (0 is refused by the service with that message).
fn parse_position(raw: &str) -> i32 {
    raw.trim().parse::<i32>().unwrap_or(0)
}

/// Re-render the form after a refusal (422), or propagate a database
/// error (logged, generic page).
#[allow(clippy::too_many_arguments)]
fn refused(
    state: &AppState,
    session: &Session,
    loc: &'static str,
    uri: &axum::http::Uri,
    series: &SeriesModel,
    tome_id: Option<u64>,
    form: TomeForm,
    err: ExpectedTomeError,
) -> Result<Response, AppError> {
    if let ExpectedTomeError::Database(inner) = err {
        return Err(inner);
    }
    let state_ = FormState {
        tome_id,
        version: form.version.unwrap_or(0),
        position: form.position,
        title: form.title,
        isbn: form.isbn.unwrap_or_default(),
        error: Some(err.localized(loc)),
    };
    render_form(
        state,
        session,
        loc,
        uri,
        series,
        state_,
        StatusCode::UNPROCESSABLE_ENTITY,
    )
}

/// `POST /series/{id}/expected-tomes` — create (origin `manual`).
pub async fn create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<TomeForm>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    let series = live_series(&state, id, loc).await?;
    let input = ExpectedTomeInput {
        position: parse_position(&form.position),
        title: &form.title,
        isbn: form.isbn.as_deref(),
    };
    match ExpectedTomeService::create(&state.pool, id, &input, ExpectedTomeOrigin::Manual).await {
        Ok(row) => {
            tracing::info!(
                series_id = id,
                tome_id = row.id,
                position = row.position,
                "Expected tome created"
            );
            Ok(Redirect::to(&format!("/series/{id}")).into_response())
        }
        Err(e) => refused(&state, &session, loc, &uri, &series, None, form, e),
    }
}

/// `POST /series/{id}/expected-tomes/{tome_id}` — update (optimistic lock).
pub async fn update(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path((id, tome_id)): Path<(u64, u64)>,
    axum::Form(form): axum::Form<TomeForm>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    let series = live_series(&state, id, loc).await?;
    tome_of_series(&state, id, tome_id, loc).await?;
    let input = ExpectedTomeInput {
        position: parse_position(&form.position),
        title: &form.title,
        isbn: form.isbn.as_deref(),
    };
    let version = form.version.unwrap_or(-1);
    match ExpectedTomeService::update(&state.pool, tome_id, version, &input).await {
        Ok(_) => {
            tracing::info!(series_id = id, tome_id, "Expected tome updated");
            Ok(Redirect::to(&format!("/series/{id}")).into_response())
        }
        Err(e) => refused(&state, &session, loc, &uri, &series, Some(tome_id), form, e),
    }
}

// ─── Delete (UX-DR8 modal) ──────────────────────────────

#[derive(Template)]
#[template(path = "fragments/expected_tome_delete_modal.html")]
pub struct ExpectedTomeDeleteModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
    pub version: i32,
}

/// `GET /series/{id}/expected-tomes/{tome_id}/delete-modal` — HTMX only
/// (405 otherwise, like the other modal fragments).
pub async fn delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path((id, tome_id)): Path<(u64, u64)>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role_with_return(Role::Librarian, &format!("/series/{id}"), loc)?;
    if !is_htmx {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    let tome = tome_of_series(&state, id, tome_id, loc).await?;
    // Raw title through t!(): the macro auto-escapes `{{ title }}`.
    let title = rust_i18n::t!(
        "expected_tome.delete_modal_title",
        locale = loc,
        title = tome.title.as_str()
    )
    .to_string();
    let body = rust_i18n::t!("expected_tome.delete_modal_body", locale = loc);
    let template = ExpectedTomeDeleteModalTemplate {
        title,
        body_html: format!("<p>{}</p>", crate::utils::html_escape(&body)),
        confirm_label: rust_i18n::t!("expected_tome.delete_modal_confirm", locale = loc)
            .to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/series/{id}/expected-tomes/{tome_id}/delete"),
        csrf_token: session.csrf_token.clone(),
        version: tome.version,
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => Err(AppError::Internal(format!(
            "expected tome delete modal render: {e}"
        ))),
    }
}

#[derive(Deserialize)]
pub struct DeleteForm {
    /// Absent when the tome's version is 0 (the modal macro only emits
    /// a non-zero version).
    #[serde(default)]
    pub version: i32,
}

/// `POST /series/{id}/expected-tomes/{tome_id}/delete` — soft delete,
/// then back to the series page.
pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path((id, tome_id)): Path<(u64, u64)>,
    axum::Form(form): axum::Form<DeleteForm>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    tome_of_series(&state, id, tome_id, loc).await?;
    ExpectedTomeService::delete(&state.pool, tome_id, form.version)
        .await
        .map_err(|e| e.into_app_error(loc))?;
    tracing::info!(series_id = id, tome_id, "Expected tome deleted");
    let to = format!("/series/{id}");
    if is_htmx {
        Ok((
            StatusCode::OK,
            [(
                axum::http::header::HeaderName::from_static("hx-redirect"),
                to,
            )],
            String::new(),
        )
            .into_response())
    } else {
        Ok(Redirect::to(&to).into_response())
    }
}

// ─── Add to wishlist (FR142) ────────────────────────────

/// The wishlist note naming the series and the position (pure).
pub fn wishlist_note(series_name: &str, position: i32, loc: &str) -> String {
    rust_i18n::t!(
        "expected_tome.wishlist_note",
        locale = loc,
        series = series_name,
        position = position
    )
    .to_string()
}

/// `POST /series/{id}/expected-tomes/{tome_id}/wishlist` — HTMX fragment
/// for `#series-feedback`: the new item, or the existing one when the
/// tome is already on the wishlist (D-11-11-4).
pub async fn add_to_wishlist(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path((id, tome_id)): Path<(u64, u64)>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    let pool = &state.pool;
    let series = live_series(&state, id, loc).await?;
    let tome = tome_of_series(&state, id, tome_id, loc).await?;
    let note = wishlist_note(&series.name, tome.position, loc);

    let existing = match tome.isbn.as_deref() {
        Some(isbn) => WishlistItemModel::find_by_isbn(pool, isbn).await?,
        None => WishlistItemModel::find_without_isbn(pool, &tome.title, &note).await?,
    };
    let open_label = rust_i18n::t!("expected_tome.wishlist_open", locale = loc);
    if let Some(item) = existing {
        let message = rust_i18n::t!(
            "expected_tome.wishlist_already",
            locale = loc,
            title = item.title.as_str()
        );
        return Ok(Html(crate::utils::feedback_html_with_link(
            "info",
            &message,
            "",
            &format!("/wishlist/{}", item.id),
            &open_label,
        ))
        .into_response());
    }

    let item_id = WishlistItemModel::create(
        pool,
        &WishlistItemDraft {
            isbn: tome.isbn.as_deref(),
            title: &tome.title,
            authors: None,
            publisher: None,
            publication_year: None,
            cover_image_url: None,
            notes: Some(&note),
        },
    )
    .await?;
    crate::routes::wishlist::audit_wishlist_add(pool, &session, item_id, &tome.title).await;
    tracing::info!(
        series_id = id,
        tome_id,
        item_id,
        "Expected tome sent to the wishlist"
    );
    let message = rust_i18n::t!(
        "expected_tome.wishlist_added",
        locale = loc,
        title = tome.title.as_str()
    );
    Ok(Html(crate::utils::feedback_html_with_link(
        "success",
        &message,
        "",
        &format!("/wishlist/{item_id}"),
        &open_label,
    ))
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wishlist_note_names_series_and_position_in_each_locale() {
        for loc in ["en", "fr", "de", "it"] {
            let note = wishlist_note("Astérix", 7, loc);
            assert!(note.contains("Astérix"), "{loc}: {note}");
            assert!(note.contains('7'), "{loc}: {note}");
            assert!(!note.contains("expected_tome."), "{loc}: {note}");
        }
    }

    #[test]
    fn parse_position_reads_integers_and_maps_garbage_to_zero() {
        assert_eq!(parse_position(" 3 "), 3);
        assert_eq!(parse_position(""), 0);
        assert_eq!(parse_position("abc"), 0);
        assert_eq!(parse_position("-2"), -2);
    }
}
