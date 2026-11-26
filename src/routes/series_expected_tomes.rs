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
use crate::models::expected_tome::ExpectedTomeModel;
use crate::models::wishlist::WishlistItem;
use crate::services::expected_tomes::{
    ExpectedTomeError, ExpectedTomeInput, ExpectedTomeService, TomeField, normalize_isbn,
    validate_title,
};

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
    /// The localized refusal of the last submit that belongs to no field.
    pub error: Option<String>,
    /// Story 20-2 (AC 7) — refusals under their field, as escaped HTML
    /// (the ISBN one may carry a server-built link, Q2).
    pub position_error_html: Option<String>,
    pub title_error_html: Option<String>,
    pub isbn_error_html: Option<String>,
    /// Story 20-2 (Q1) — the "already on your wish list" entry with its
    /// link, built by `crate::utils::feedback_html_with_link` (escaped).
    pub notice_html: Option<String>,
    /// Story 20-2 — the gap → wish list mode (`wishlist=1`).
    pub wishlist_mode: bool,
    pub wishlist_help: String,
    pub label_position: String,
    pub label_title: String,
    pub label_isbn: String,
    pub isbn_help: String,
    pub label_save: String,
    pub label_cancel: String,
    pub label_back: String,
}

/// The refusals of a submit: one under each field, or one for the form.
#[derive(Default)]
struct FormErrors {
    general: Option<String>,
    position_html: Option<String>,
    title_html: Option<String>,
    isbn_html: Option<String>,
}

impl FormErrors {
    /// A localized `ExpectedTomeError` placed under its field (Rule #3),
    /// or above the form when it belongs to none.
    fn of(err: &ExpectedTomeError, loc: &str) -> Self {
        let message = err.localized(loc);
        let html = || Some(crate::utils::html_escape(&message));
        match err.field() {
            Some(TomeField::Position) => Self { position_html: html(), ..Self::default() },
            Some(TomeField::Title) => Self { title_html: html(), ..Self::default() },
            Some(TomeField::Isbn) => Self { isbn_html: html(), ..Self::default() },
            None => Self { general: Some(message), ..Self::default() },
        }
    }
}

/// What the form shows: typed (or stored) values and optional refusals.
struct FormState {
    tome_id: Option<u64>,
    version: i32,
    position: String,
    title: String,
    isbn: String,
    errors: FormErrors,
    notice_html: Option<String>,
    wishlist_mode: bool,
}

/// Story 20-2 (Rule #20) — the gap → wish list mode is `wishlist=1`
/// only; any other value (or none) is the plain form (pure).
pub fn is_wishlist_mode(raw: Option<&str>) -> bool {
    raw == Some("1")
}

/// Story 20-2 (D6) — the title pre-filled for a gap sent to the wish
/// list: "{series}, tome {position}" in the request locale (pure).
pub fn wishlist_title_prefill(series_name: &str, position: i32, loc: &str) -> String {
    rust_i18n::t!(
        "expected_tome.wishlist_title_prefill",
        locale = loc,
        series = series_name,
        position = position
    )
    .to_string()
}

/// A position field read as a positive integer, else `None`.
fn positive_position(raw: &str) -> Option<i32> {
    raw.trim().parse::<i32>().ok().filter(|p| *p >= 1)
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
    let t = |k: &str| rust_i18n::t!(k, locale = loc).to_string();
    let heading = if form.tome_id.is_some() {
        t("expected_tome.form_title_edit")
    } else if form.wishlist_mode {
        // Q4: without a valid position, the plain "Add to wish list".
        match positive_position(&form.position) {
            Some(position) => rust_i18n::t!(
                "expected_tome.form_title_wishlist",
                locale = loc,
                position = position
            )
            .to_string(),
            None => t("wishlist.add_to_wishlist"),
        }
    } else {
        t("expected_tome.form_title_new")
    };
    let label_save = if form.wishlist_mode {
        t("expected_tome.wishlist_save")
    } else {
        t("expected_tome.save")
    };
    let template = ExpectedTomeFormTemplate {
        base: crate::utils::base_context(session, loc, "series", uri, state.page_settings()),
        heading,
        series_id: series.id,
        series_name: series.name.clone(),
        tome_id: form.tome_id,
        version: form.version,
        position_value: form.position,
        title_value: form.title,
        isbn_value: form.isbn,
        error: form.errors.general,
        position_error_html: form.errors.position_html,
        title_error_html: form.errors.title_html,
        isbn_error_html: form.errors.isbn_html,
        notice_html: form.notice_html,
        wishlist_mode: form.wishlist_mode,
        wishlist_help: t("expected_tome.wishlist_form_help"),
        label_position: t("expected_tome.field_position"),
        label_title: t("expected_tome.field_title"),
        label_isbn: t("expected_tome.field_isbn"),
        isbn_help: t("expected_tome.isbn_help"),
        label_save,
        label_cancel: t("common.cancel"),
        label_back: t("expected_tome.back"),
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
    /// Story 20-2 — `1` opens the gap → wish list mode.
    #[serde(default)]
    pub wishlist: Option<String>,
}

/// `GET /series/{id}/expected-tomes/new[?position=N][&wishlist=1]`.
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
    let position = query.position.as_deref().and_then(positive_position);
    let wishlist_mode = is_wishlist_mode(query.wishlist.as_deref());
    // Story 20-2 (D6): the wish list mode pre-fills an editable title.
    let title = match (wishlist_mode, position) {
        (true, Some(p)) => wishlist_title_prefill(&series.name, p, loc),
        _ => String::new(),
    };
    let form = FormState {
        tome_id: None,
        version: 0,
        position: position.map(|p| p.to_string()).unwrap_or_default(),
        title,
        isbn: String::new(),
        errors: FormErrors::default(),
        notice_html: None,
        wishlist_mode,
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
    // A settled tome is read-only: say so up front rather than on save.
    let error = tome
        .is_settled()
        .then(|| ExpectedTomeError::Settled { id: tome.id }.localized(loc));
    let form = FormState {
        tome_id: Some(tome.id),
        version: tome.version,
        position: tome.position.to_string(),
        title: tome.title,
        isbn: tome.isbn.unwrap_or_default(),
        errors: FormErrors {
            general: error,
            ..FormErrors::default()
        },
        notice_html: None,
        wishlist_mode: false,
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
    /// Story 20-2 — `1` in the gap → wish list mode (hidden field).
    #[serde(default)]
    pub wishlist: Option<String>,
}

/// A position field that is not an integer reads as "position must be
/// 1 or more" (0 is refused by the service with that message).
fn parse_position(raw: &str) -> i32 {
    raw.trim().parse::<i32>().unwrap_or(0)
}

/// Re-render the form after a refusal (422) with the typed values. A
/// version conflict shows the tome as it now stands (with its current
/// version, so a deliberate resubmit can succeed); a vanished tome or
/// series is a 404 and a database error propagates (logged, generic).
async fn refused(
    state: &AppState,
    session: &Session,
    loc: &'static str,
    series: &SeriesModel,
    tome_id: Option<u64>,
    form: TomeForm,
    err: ExpectedTomeError,
) -> Result<Response, AppError> {
    match err {
        ExpectedTomeError::Database(inner) => return Err(inner),
        ExpectedTomeError::NotFound { .. } | ExpectedTomeError::SeriesNotFound { .. } => {
            return Err(err.into_app_error(loc));
        }
        _ => {}
    }
    let wishlist_mode = tome_id.is_none() && is_wishlist_mode(form.wishlist.as_deref());
    let mut state_ = FormState {
        tome_id,
        version: form.version.unwrap_or(0),
        position: form.position,
        title: form.title,
        isbn: form.isbn.unwrap_or_default(),
        errors: FormErrors::of(&err, loc),
        notice_html: None,
        wishlist_mode,
    };
    if let (ExpectedTomeError::VersionConflict { .. }, Some(id)) = (&err, tome_id) {
        match ExpectedTomeService::find(&state.pool, id).await? {
            Some(current) if current.series_id == series.id => {
                state_.version = current.version;
                state_.position = current.position.to_string();
                state_.title = current.title;
                state_.isbn = current.isbn.unwrap_or_default();
            }
            _ => return Err(ExpectedTomeError::NotFound { id }.into_app_error(loc)),
        }
    }
    rerender(state, session, loc, series, state_, StatusCode::UNPROCESSABLE_ENTITY)
}

/// Render the form again after a submit, at its own GET address, so the
/// layout's return links (language toggle, login `next`) never point at
/// a POST-only route.
fn rerender(
    state: &AppState,
    session: &Session,
    loc: &'static str,
    series: &SeriesModel,
    form: FormState,
    status: StatusCode,
) -> Result<Response, AppError> {
    let page = match (form.tome_id, form.wishlist_mode) {
        (Some(t), _) => format!("/series/{}/expected-tomes/{t}/edit", series.id),
        (None, true) => format!("/series/{}/expected-tomes/new?wishlist=1", series.id),
        (None, false) => format!("/series/{}/expected-tomes/new", series.id),
    };
    let uri: axum::http::Uri = page
        .parse()
        .map_err(|e| AppError::Internal(format!("expected tome form uri: {e}")))?;
    render_form(state, session, loc, &uri, series, form, status)
}

/// `POST /series/{id}/expected-tomes` — create (origin `manual`); with
/// `wishlist=1`, the tome and its wish list entry together (story 20-2).
pub async fn create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
    axum::Form(form): axum::Form<TomeForm>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    let series = live_series(&state, id, loc).await?;
    if is_wishlist_mode(form.wishlist.as_deref()) {
        return create_wished(&state, &session, loc, &series, form).await;
    }
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
        Err(e) => refused(&state, &session, loc, &series, None, form, e).await,
    }
}

/// Story 20-2 (D4, AC 3–8) — a gap sent to the wish list: the expected
/// tome and the wish list entry in one transaction. Order: pure checks;
/// the series row locked by primary key (the lock `create_in` takes
/// anyway — no new `FOR UPDATE`, Rule #22); a live title holding the
/// ISBN refuses (before `create_in`, which would attach it); an equivalent
/// entry answers with its link; then the tome, then the entry.
async fn create_wished(
    state: &AppState,
    session: &Session,
    loc: &'static str,
    series: &SeriesModel,
    form: TomeForm,
) -> Result<Response, AppError> {
    let position = parse_position(&form.position);
    let title = match validate_title(&form.title) {
        Ok(t) => t,
        Err(e) => return refused(state, session, loc, series, None, form, e).await,
    };
    let isbn = match normalize_isbn(form.isbn.as_deref()) {
        Ok(i) => i,
        Err(e) => return refused(state, session, loc, series, None, form, e).await,
    };

    let mut tx = state.pool.begin().await?;
    let Some(shape) = ExpectedTomeModel::lock_series_in(&mut tx, series.id).await? else {
        return Err(ExpectedTomeError::SeriesNotFound {
            series_id: series.id,
        }
        .into_app_error(loc));
    };

    // AC 6 (Q2): the "already used" field refusal, the title as a link.
    if let Some(isbn13) = isbn.as_deref()
        && let Some(&owner) = ExpectedTomeModel::live_titles_with_isbn(&mut tx, isbn13)
            .await?
            .first()
    {
        let name = crate::models::title::TitleModel::name_any_state_in(&mut tx, owner)
            .await?
            .unwrap_or_default();
        tx.rollback().await?;
        let message = rust_i18n::t!(
            "error.isbn.already_used",
            locale = loc,
            title = crate::utils::LINK_PLACEHOLDER
        );
        let errors = FormErrors {
            isbn_html: Some(crate::utils::message_with_link_html(
                &message,
                &format!("/title/{owner}"),
                &name,
            )),
            ..FormErrors::default()
        };
        tracing::info!(series_id = series.id, title_id = owner, "Gap to wish list: ISBN already owned");
        let form_state = typed_state(form, errors, None);
        return rerender(state, session, loc, series, form_state, StatusCode::UNPROCESSABLE_ENTITY);
    }

    // AC 5 (Q1): already on the wish list — nothing written, its link.
    let note = wishlist_note(&shape.name, position, loc);
    if let Some(item) = existing_wish(&mut tx, isbn.as_deref(), &title, &note).await? {
        tx.rollback().await?;
        tracing::info!(series_id = series.id, item_id = item.id, "Gap to wish list: already wished");
        let form_state = typed_state(form, FormErrors::default(), Some(wish_already_html(&item, loc)));
        return rerender(state, session, loc, series, form_state, StatusCode::OK);
    }

    let input = ExpectedTomeInput {
        position,
        title: &title,
        isbn: isbn.as_deref(),
    };
    let tome_id = match ExpectedTomeService::create_in(&mut tx, series.id, &input, ExpectedTomeOrigin::Manual).await {
        Ok(id) => id,
        Err(e) => {
            tx.rollback().await?;
            return refused(state, session, loc, series, None, form, e).await;
        }
    };
    let item_id = WishlistItemModel::create_in(
        &mut tx,
        &WishlistItemDraft {
            isbn: isbn.as_deref(),
            title: &title,
            authors: None,
            publisher: None,
            publication_year: None,
            cover_image_url: None,
            notes: Some(&note),
        },
    )
    .await?;
    tx.commit().await?;
    crate::routes::wishlist::audit_wishlist_add(&state.pool, session, item_id, &title).await;
    tracing::info!(
        series_id = series.id,
        tome_id,
        item_id,
        position,
        "Gap sent to the wish list"
    );
    Ok(Redirect::to(&format!("/series/{}?wishlist_item={item_id}", series.id)).into_response())
}

/// The wish list form re-rendered with what was typed.
fn typed_state(form: TomeForm, errors: FormErrors, notice_html: Option<String>) -> FormState {
    FormState {
        tome_id: None,
        version: 0,
        position: form.position,
        title: form.title,
        isbn: form.isbn.unwrap_or_default(),
        errors,
        notice_html,
        wishlist_mode: true,
    }
}

/// `POST /series/{id}/expected-tomes/{tome_id}` — update (optimistic lock).
pub async fn update(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
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
        Err(e) => refused(&state, &session, loc, &series, Some(tome_id), form, e).await,
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
/// tome is already on the wishlist (D-11-11-4). Refusals and errors are
/// rendered into the same region (the series page has no
/// `#feedback-list` for the `AppError` retarget to land in).
pub async fn add_to_wishlist(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path((id, tome_id)): Path<(u64, u64)>,
) -> Result<Response, AppError> {
    let loc = locale.0;
    session.require_role(Role::Librarian, loc)?;
    let html = match wish_tome(&state, &session, id, tome_id, loc).await {
        Ok(html) => html,
        Err(e) => {
            let message = match &e {
                AppError::NotFound(m) | AppError::Conflict(m) | AppError::BadRequest(m) => {
                    m.clone()
                }
                other => {
                    tracing::error!(error = %other, series_id = id, tome_id, "Add to wishlist failed");
                    rust_i18n::t!("error.internal", locale = loc).to_string()
                }
            };
            crate::utils::feedback_html("error", &message, "")
        }
    };
    Ok(Html(html).into_response())
}

async fn wish_tome(
    state: &AppState,
    session: &Session,
    id: u64,
    tome_id: u64,
    loc: &'static str,
) -> Result<String, AppError> {
    let pool = &state.pool;
    let series = live_series(state, id, loc).await?;
    let tome = tome_of_series(state, id, tome_id, loc).await?;
    if tome.is_settled() {
        let message = rust_i18n::t!(
            "expected_tome.wishlist_owned",
            locale = loc,
            title = tome.title.as_str()
        );
        return Ok(crate::utils::feedback_html("info", &message, ""));
    }
    let note = wishlist_note(&series.name, tome.position, loc);

    let mut conn = pool.acquire().await?;
    let existing = existing_wish(&mut conn, tome.isbn.as_deref(), &tome.title, &note).await?;
    drop(conn);
    if let Some(item) = existing {
        return Ok(wish_already_html(&item, loc));
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
    crate::routes::wishlist::audit_wishlist_add(pool, session, item_id, &tome.title).await;
    tracing::info!(
        series_id = id,
        tome_id,
        item_id,
        "Expected tome sent to the wishlist"
    );
    Ok(wish_added_html(&tome.title, item_id, loc))
}

/// D-11-11-4, shared by both paths to the wish list (story 20-2, AC 9):
/// the live entry already standing for this tome — the same ISBN, or,
/// without ISBN, the same title and note. A plain read.
async fn existing_wish(
    conn: &mut sqlx::MySqlConnection,
    isbn: Option<&str>,
    title: &str,
    note: &str,
) -> Result<Option<WishlistItem>, AppError> {
    match isbn {
        Some(isbn) => WishlistItemModel::find_by_isbn_in(conn, isbn).await,
        None => WishlistItemModel::find_without_isbn_in(conn, title, note).await,
    }
}

/// "… is already on your wish list", with the link to that entry.
fn wish_already_html(item: &WishlistItem, loc: &str) -> String {
    let message = rust_i18n::t!(
        "expected_tome.wishlist_already",
        locale = loc,
        title = item.title.as_str()
    );
    crate::utils::feedback_html_with_link(
        "info",
        &message,
        "",
        &format!("/wishlist/{}", item.id),
        &rust_i18n::t!("expected_tome.wishlist_open", locale = loc),
    )
}

/// "… was added to your wish list", with the link to the new entry.
pub fn wish_added_html(title: &str, item_id: u64, loc: &str) -> String {
    let message = rust_i18n::t!("expected_tome.wishlist_added", locale = loc, title = title);
    crate::utils::feedback_html_with_link(
        "success",
        &message,
        "",
        &format!("/wishlist/{item_id}"),
        &rust_i18n::t!("expected_tome.wishlist_open", locale = loc),
    )
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
    fn the_wishlist_mode_is_wishlist_1_only() {
        assert!(is_wishlist_mode(Some("1")));
        for other in [None, Some("true"), Some("0"), Some(""), Some(" 1"), Some("yes")] {
            assert!(!is_wishlist_mode(other), "{other:?}");
        }
    }

    #[test]
    fn the_title_prefill_names_series_and_position_in_each_locale() {
        for loc in ["en", "fr", "de", "it"] {
            let title = wishlist_title_prefill("Astérix", 7, loc);
            assert!(title.contains("Astérix"), "{loc}: {title}");
            assert!(title.contains('7'), "{loc}: {title}");
            assert!(!title.contains("expected_tome."), "{loc}: {title}");
        }
        assert_eq!(wishlist_title_prefill("Astérix", 7, "en"), "Astérix, tome 7");
    }

    #[test]
    fn a_refusal_lands_under_its_field_or_above_the_form() {
        let e = FormErrors::of(&ExpectedTomeError::TitleRequired, "en");
        assert!(e.title_html.is_some() && e.general.is_none() && e.isbn_html.is_none());
        let e = FormErrors::of(&ExpectedTomeError::PositionTaken { position: 2 }, "en");
        assert!(e.position_html.is_some() && e.general.is_none());
        let e = FormErrors::of(&ExpectedTomeError::InvalidIsbn { value: "<b>".into() }, "en");
        assert!(e.isbn_html.is_some());
        let e = FormErrors::of(&ExpectedTomeError::PositionHeld { position: 2, holder: "<b>X</b>".into() }, "en");
        let html = e.position_html.unwrap();
        assert!(html.contains("&lt;b&gt;X"), "escaped: {html}");
        let e = FormErrors::of(&ExpectedTomeError::Settled { id: 1 }, "en");
        assert!(e.general.is_some() && e.title_html.is_none());
    }

    #[test]
    fn positive_position_reads_a_positive_integer_only() {
        assert_eq!(positive_position(" 3 "), Some(3));
        assert_eq!(positive_position("0"), None);
        assert_eq!(positive_position("-1"), None);
        assert_eq!(positive_position("x"), None);
    }

    #[test]
    fn parse_position_reads_integers_and_maps_garbage_to_zero() {
        assert_eq!(parse_position(" 3 "), 3);
        assert_eq!(parse_position(""), 0);
        assert_eq!(parse_position("abc"), 0);
        assert_eq!(parse_position("-2"), -2);
    }
}
