//! CR #242 — wish list routes.
//!
//! Surface map:
//!
//! - `GET  /wishlist` — list page (newest-first table + mobile cards)
//! - `GET  /wishlist/new` — add form (radio: by-ISBN | free-form)
//! - `POST /wishlist/preview-isbn` — ISBN preview card (renders the resolved
//!   metadata as a confirmation step; HTMX)
//! - `POST /wishlist` — final insert (both ISBN-confirmed and free-form here)
//! - `GET  /wishlist/:id` — detail page
//! - `GET  /wishlist/:id/delete-modal` — UX-DR8 "Mark as bought" confirmation
//! - `DELETE /wishlist/:id` — soft-delete (Mark as bought + Remove + auto-link)
//! - `GET  /wishlist/print` — print-friendly page (browser → PDF)
//!
//! v1 cover-handling decision: a wishlist row stores the REMOTE URL the
//! provider chain resolved (CSP `img-src` allowlists the common image
//! hosts). Local download via `CoverService::download_and_resize` is a
//! deliberate follow-up — keeps the synchronous insert latency low and
//! the cover-storage layout simple. The AC's "Cover download reuses
//! CoverService" intent is partially honoured via
//! `cover::resolve_cover_url_with_fallback` (the OL fallback resolver).

use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::media_type::{CodeType, MediaType};
use crate::models::wishlist::{WishlistItem, WishlistItemDraft, WishlistItemModel};
use crate::services::title::TitleService;
use crate::utils::current_url;

// ─── List page ───────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/wishlist.html")]
pub struct WishlistPageTemplate {
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
    pub items: Vec<WishlistItem>,
    pub can_edit: bool,
    pub label_list_title: String,
    pub label_add: String,
    pub label_print: String,
    pub label_empty_heading: String,
    pub label_empty_body: String,
    pub label_col_title: String,
    pub label_col_isbn: String,
    pub label_col_authors: String,
    pub label_col_publisher: String,
    pub label_col_added: String,
    pub label_col_actions: String,
    pub label_action_bought: String,
    pub label_placeholder_empty: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
}

pub async fn wishlist_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(_is_htmx): HxRequest,
) -> Result<impl IntoResponse, AppError> {
    // Anonymous can browse the wishlist for v1 (household sharing case).
    // Only mutations require Librarian+.
    let pool = &state.pool;
    let loc = locale.0;

    let items = WishlistItemModel::list_active(pool).await?;

    let template = WishlistPageTemplate {
        lang: loc.to_string(),
        role: session.role.to_string(),
        current_page: "wishlist",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        connection_status: crate::utils::ConnectionStatusContext::new(loc),
        shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new(loc),
        session_timeout_secs: state.session_timeout_secs(),
        csrf_token: session.csrf_token.clone(),
        nav_catalog: rust_i18n::t!("nav.catalog", locale = loc).to_string(),
        nav_loans: rust_i18n::t!("nav.loans", locale = loc).to_string(),
        nav_wishlist: rust_i18n::t!("nav.wishlist", locale = loc).to_string(),
        nav_locations: rust_i18n::t!("nav.locations", locale = loc).to_string(),
        nav_series: rust_i18n::t!("nav.series", locale = loc).to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers", locale = loc).to_string(),
        nav_admin: rust_i18n::t!("nav.admin", locale = loc).to_string(),
        nav_login: rust_i18n::t!("nav.login", locale = loc).to_string(),
        nav_logout: rust_i18n::t!("nav.logout", locale = loc).to_string(),
        nav_menu_open: rust_i18n::t!("nav.menu_open", locale = loc).to_string(),
        items,
        can_edit: session.role >= Role::Librarian,
        label_list_title: rust_i18n::t!("wishlist.list_title", locale = loc).to_string(),
        label_add: rust_i18n::t!("wishlist.add_button", locale = loc).to_string(),
        label_print: rust_i18n::t!("wishlist.print_button", locale = loc).to_string(),
        label_empty_heading: rust_i18n::t!("wishlist.empty_heading", locale = loc).to_string(),
        label_empty_body: rust_i18n::t!("wishlist.empty_body", locale = loc).to_string(),
        label_col_title: rust_i18n::t!("wishlist.col_title", locale = loc).to_string(),
        label_col_isbn: rust_i18n::t!("wishlist.col_isbn", locale = loc).to_string(),
        label_col_authors: rust_i18n::t!("wishlist.col_authors", locale = loc).to_string(),
        label_col_publisher: rust_i18n::t!("wishlist.col_publisher", locale = loc).to_string(),
        label_col_added: rust_i18n::t!("wishlist.col_added", locale = loc).to_string(),
        label_col_actions: rust_i18n::t!("wishlist.col_actions", locale = loc).to_string(),
        label_action_bought: rust_i18n::t!("wishlist.action_bought", locale = loc).to_string(),
        label_placeholder_empty: rust_i18n::t!("title_detail.field_unset", locale = loc).to_string(),
        current_url: current_url(&uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = loc).to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("wishlist render".to_string())),
    }
}

// ─── Add form ────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/wishlist_new.html")]
pub struct WishlistNewTemplate {
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
    pub label_form_title: String,
    pub label_mode_isbn: String,
    pub label_mode_freeform: String,
    pub label_isbn: String,
    pub label_lookup: String,
    pub label_title: String,
    pub label_authors: String,
    pub label_publisher: String,
    pub label_notes: String,
    pub label_add: String,
    pub label_cancel: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
}

pub async fn wishlist_new_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let loc = locale.0;

    let template = WishlistNewTemplate {
        lang: loc.to_string(),
        role: session.role.to_string(),
        current_page: "wishlist",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        connection_status: crate::utils::ConnectionStatusContext::new(loc),
        shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new(loc),
        session_timeout_secs: state.session_timeout_secs(),
        csrf_token: session.csrf_token.clone(),
        nav_catalog: rust_i18n::t!("nav.catalog", locale = loc).to_string(),
        nav_loans: rust_i18n::t!("nav.loans", locale = loc).to_string(),
        nav_wishlist: rust_i18n::t!("nav.wishlist", locale = loc).to_string(),
        nav_locations: rust_i18n::t!("nav.locations", locale = loc).to_string(),
        nav_series: rust_i18n::t!("nav.series", locale = loc).to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers", locale = loc).to_string(),
        nav_admin: rust_i18n::t!("nav.admin", locale = loc).to_string(),
        nav_login: rust_i18n::t!("nav.login", locale = loc).to_string(),
        nav_logout: rust_i18n::t!("nav.logout", locale = loc).to_string(),
        nav_menu_open: rust_i18n::t!("nav.menu_open", locale = loc).to_string(),
        label_form_title: rust_i18n::t!("wishlist.form_title", locale = loc).to_string(),
        label_mode_isbn: rust_i18n::t!("wishlist.mode_isbn", locale = loc).to_string(),
        label_mode_freeform: rust_i18n::t!("wishlist.mode_freeform", locale = loc).to_string(),
        label_isbn: rust_i18n::t!("wishlist.field_isbn", locale = loc).to_string(),
        label_lookup: rust_i18n::t!("wishlist.lookup_button", locale = loc).to_string(),
        label_title: rust_i18n::t!("wishlist.field_title", locale = loc).to_string(),
        label_authors: rust_i18n::t!("wishlist.field_authors", locale = loc).to_string(),
        label_publisher: rust_i18n::t!("wishlist.field_publisher", locale = loc).to_string(),
        label_notes: rust_i18n::t!("wishlist.field_notes", locale = loc).to_string(),
        label_add: rust_i18n::t!("wishlist.add_to_wishlist", locale = loc).to_string(),
        label_cancel: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        current_url: current_url(&uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = loc).to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("wishlist new render".to_string())),
    }
}

// ─── ISBN preview (HTMX fragment) ────────────────────────────────

#[derive(Deserialize)]
pub struct WishlistPreviewForm {
    pub isbn: String,
}

#[derive(Template)]
#[template(path = "fragments/wishlist_isbn_preview.html")]
pub struct WishlistIsbnPreviewTemplate {
    pub isbn: String,
    pub resolved_title: String,
    pub resolved_authors: Option<String>,
    pub resolved_publisher: Option<String>,
    pub resolved_year: Option<u16>,
    pub resolved_cover_url: Option<String>,
    pub csrf_token: String,
    pub label_confirm: String,
    pub label_cancel: String,
    pub label_no_match: String,
    pub label_resolved_intro: String,
    pub label_notes_optional: String,
    pub label_notes_placeholder: String,
    pub label_no_cover: String,
    pub match_found: bool,
}

pub async fn wishlist_preview_isbn(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    axum::Form(form): axum::Form<WishlistPreviewForm>,
) -> Result<Response, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    if !is_htmx {
        return Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response());
    }

    let isbn = form.isbn.trim();
    if !TitleService::validate_isbn13_checksum(isbn) {
        return Err(AppError::BadRequest(
            rust_i18n::t!("wishlist.isbn_invalid", locale = loc).to_string(),
        ));
    }

    let timeout_secs = state
        .settings
        .read()
        .map(|s| s.metadata_fetch_timeout_secs)
        .unwrap_or(30);
    let per_provider_timeout_secs = state.metadata_chain_per_provider_timeout_secs();
    let metadata = crate::metadata::chain::ChainExecutor::execute(
        &state.registry,
        pool,
        isbn,
        &CodeType::Isbn,
        &MediaType::Book,
        timeout_secs,
        per_provider_timeout_secs,
    )
    .await;

    let (match_found, resolved_title, resolved_authors, resolved_publisher, resolved_year, resolved_cover_url) =
        match metadata {
            Some(m) => {
                let cover_url = crate::services::cover::resolve_cover_url_with_fallback(
                    &state.http_client,
                    &m,
                    isbn,
                    &CodeType::Isbn,
                )
                .await;
                let year = m
                    .publication_date
                    .as_deref()
                    .and_then(|d| d.get(..4))
                    .and_then(|y| y.parse::<u16>().ok());
                (
                    true,
                    m.title.unwrap_or_else(|| "—".to_string()),
                    if m.authors.is_empty() {
                        None
                    } else {
                        Some(m.authors.join(", "))
                    },
                    m.publisher,
                    year,
                    cover_url,
                )
            }
            None => (false, String::new(), None, None, None, None),
        };

    let template = WishlistIsbnPreviewTemplate {
        isbn: isbn.to_string(),
        resolved_title,
        resolved_authors,
        resolved_publisher,
        resolved_year,
        resolved_cover_url,
        csrf_token: session.csrf_token.clone(),
        label_confirm: rust_i18n::t!("wishlist.confirm_add", locale = loc).to_string(),
        label_cancel: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        label_no_match: rust_i18n::t!("wishlist.no_metadata", locale = loc).to_string(),
        label_resolved_intro: rust_i18n::t!("wishlist.resolved_intro", locale = loc).to_string(),
        label_notes_optional: rust_i18n::t!("wishlist.field_notes", locale = loc).to_string(),
        label_notes_placeholder: rust_i18n::t!("wishlist.notes_placeholder", locale = loc).to_string(),
        label_no_cover: rust_i18n::t!("cover.no_cover", locale = loc).to_string(),
        match_found,
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("wishlist preview render".to_string())),
    }
}

// ─── Create (final insert) ────────────────────────────────────────

#[derive(Deserialize)]
pub struct WishlistCreateForm {
    pub mode: String,
    pub isbn: Option<String>,
    pub title: String,
    pub authors: Option<String>,
    pub publisher: Option<String>,
    pub publication_year: Option<u16>,
    pub cover_image_url: Option<String>,
    pub notes: Option<String>,
}

pub async fn wishlist_create(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    axum::Form(form): axum::Form<WishlistCreateForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let title = form.title.trim();
    if title.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("wishlist.title_required", locale = loc).to_string(),
        ));
    }
    // ISBN mode: re-validate the carried isbn. Free-form mode: ignore any
    // isbn field even if the form snuck one in.
    let isbn = if form.mode == "isbn" {
        match form.isbn.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(i) if TitleService::validate_isbn13_checksum(i) => Some(i.to_string()),
            Some(_) => {
                return Err(AppError::BadRequest(
                    rust_i18n::t!("wishlist.isbn_invalid", locale = loc).to_string(),
                ));
            }
            None => None,
        }
    } else {
        None
    };

    let id = WishlistItemModel::create(
        pool,
        &WishlistItemDraft {
            isbn: isbn.as_deref(),
            title,
            authors: form.authors.as_deref().map(str::trim).filter(|s| !s.is_empty()),
            publisher: form
                .publisher
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            publication_year: form.publication_year,
            cover_image_url: form
                .cover_image_url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            notes: form.notes.as_deref().map(str::trim).filter(|s| !s.is_empty()),
        },
    )
    .await?;

    // Audit trail (#242 AC): record the add. user_id is None for anonymous
    // (mutations require Librarian+ so user_id is always Some here).
    if let Some(user_id) = session.user_id {
        crate::models::admin_audit::AdminAuditModel::create(
            pool,
            user_id,
            "wishlist_add",
            Some("wishlist_items"),
            Some(id),
            Some(serde_json::json!({"title": title})),
        )
        .await
        .ok();
    }

    Ok(Redirect::to("/wishlist"))
}

// ─── Detail page ─────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/wishlist_detail.html")]
pub struct WishlistDetailTemplate {
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
    pub item: WishlistItem,
    pub can_edit: bool,
    pub label_isbn: String,
    pub label_authors: String,
    pub label_publisher: String,
    pub label_year: String,
    pub label_notes: String,
    pub label_added_at: String,
    pub label_back: String,
    pub label_action_bought: String,
    pub label_placeholder_empty: String,
    pub label_no_cover: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
}

pub async fn wishlist_detail(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;
    let loc = locale.0;
    let item = WishlistItemModel::find_by_id(pool, id).await?.ok_or_else(|| {
        AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
    })?;

    let template = WishlistDetailTemplate {
        lang: loc.to_string(),
        role: session.role.to_string(),
        current_page: "wishlist",
        skip_label: rust_i18n::t!("nav.skip_to_content", locale = loc).to_string(),
        connection_status: crate::utils::ConnectionStatusContext::new(loc),
        shortcuts_cheat_sheet: crate::utils::ShortcutsCheatSheetContext::new(loc),
        session_timeout_secs: state.session_timeout_secs(),
        csrf_token: session.csrf_token.clone(),
        nav_catalog: rust_i18n::t!("nav.catalog", locale = loc).to_string(),
        nav_loans: rust_i18n::t!("nav.loans", locale = loc).to_string(),
        nav_wishlist: rust_i18n::t!("nav.wishlist", locale = loc).to_string(),
        nav_locations: rust_i18n::t!("nav.locations", locale = loc).to_string(),
        nav_series: rust_i18n::t!("nav.series", locale = loc).to_string(),
        nav_borrowers: rust_i18n::t!("nav.borrowers", locale = loc).to_string(),
        nav_admin: rust_i18n::t!("nav.admin", locale = loc).to_string(),
        nav_login: rust_i18n::t!("nav.login", locale = loc).to_string(),
        nav_logout: rust_i18n::t!("nav.logout", locale = loc).to_string(),
        nav_menu_open: rust_i18n::t!("nav.menu_open", locale = loc).to_string(),
        item,
        can_edit: session.role >= Role::Librarian,
        label_isbn: rust_i18n::t!("wishlist.col_isbn", locale = loc).to_string(),
        label_authors: rust_i18n::t!("wishlist.col_authors", locale = loc).to_string(),
        label_publisher: rust_i18n::t!("wishlist.col_publisher", locale = loc).to_string(),
        label_year: rust_i18n::t!("wishlist.field_year", locale = loc).to_string(),
        label_notes: rust_i18n::t!("wishlist.field_notes", locale = loc).to_string(),
        label_added_at: rust_i18n::t!("wishlist.col_added", locale = loc).to_string(),
        label_back: rust_i18n::t!("wishlist.back_to_list", locale = loc).to_string(),
        label_action_bought: rust_i18n::t!("wishlist.action_bought", locale = loc).to_string(),
        label_placeholder_empty: rust_i18n::t!("title_detail.field_unset", locale = loc).to_string(),
        label_no_cover: rust_i18n::t!("cover.no_cover", locale = loc).to_string(),
        current_url: current_url(&uri),
        lang_toggle_aria: rust_i18n::t!("nav.language_toggle_aria", locale = loc).to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("wishlist detail render".to_string())),
    }
}

// ─── Delete modal (UX-DR8) ───────────────────────────────────────

#[derive(Template)]
#[template(path = "fragments/wishlist_delete_modal.html")]
pub struct WishlistDeleteModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
}

pub async fn wishlist_delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Librarian, "/wishlist", locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    if !is_htmx {
        return Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response());
    }

    let item = WishlistItemModel::find_by_id(pool, id).await?.ok_or_else(|| {
        AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
    })?;
    let title = rust_i18n::t!(
        "wishlist.delete_modal_title",
        locale = loc,
        title = item.title.as_str()
    )
    .to_string();
    let body_text = rust_i18n::t!("wishlist.delete_modal_body", locale = loc).to_string();
    let body_html = format!("<p>{}</p>", crate::utils::html_escape(&body_text));

    let template = WishlistDeleteModalTemplate {
        title,
        body_html,
        confirm_label: rust_i18n::t!("wishlist.action_bought", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/wishlist/{}", item.id),
        csrf_token: session.csrf_token.clone(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("wishlist delete modal render".to_string())),
    }
}

// ─── Soft-delete (Mark as bought / Remove) ───────────────────────

pub async fn wishlist_delete(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let item = WishlistItemModel::find_by_id(pool, id).await?.ok_or_else(|| {
        AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string())
    })?;
    let n = WishlistItemModel::soft_delete(pool, item.id, item.version).await?;
    if n == 0 {
        return Err(AppError::Conflict(
            rust_i18n::t!("error.conflict", locale = loc).to_string(),
        ));
    }

    // Audit trail entry (#242 AC).
    if let Some(user_id) = session.user_id {
        crate::models::admin_audit::AdminAuditModel::create(
            pool,
            user_id,
            "wishlist_bought",
            Some("wishlist_items"),
            Some(item.id),
            Some(serde_json::json!({"title": item.title, "isbn": item.isbn})),
        )
        .await
        .ok();
    }

    // HX-Redirect drives a full-page navigation back to /wishlist with
    // the bought item removed from the list. Consistent with the volume-
    // delete redirect pattern (CR #209).
    Ok((
        axum::http::StatusCode::OK,
        [(
            axum::http::header::HeaderName::from_static("hx-redirect"),
            "/wishlist".to_string(),
        )],
        String::new(),
    )
        .into_response())
}

// ─── Auto-link on catalog scan ───────────────────────────────────

/// CR #242 — emit an HTML banner snippet when a freshly-cataloged ISBN
/// matches an active wishlist row. Called from the catalog scan handlers
/// (ISBN + UPC paths). Returns an empty string when no match is found
/// so callers can unconditionally concatenate it onto the success
/// feedback HTML without worrying about layout fallout.
///
/// The banner carries a single action: open the UX-DR8 "Mark as bought"
/// confirmation modal at `/wishlist/:id/delete-modal`. No silent
/// deletion — the user always confirms.
pub async fn autolink_feedback_html(pool: &crate::db::DbPool, isbn: &str, loc: &str) -> String {
    let item = match WishlistItemModel::find_by_isbn(pool, isbn).await {
        Ok(Some(i)) => i,
        _ => return String::new(),
    };
    let title = rust_i18n::t!("wishlist.autolink_banner_title", locale = loc);
    let body = rust_i18n::t!("wishlist.autolink_banner_body", locale = loc);
    let remove = rust_i18n::t!("wishlist.autolink_remove", locale = loc);
    let escaped_item_title = crate::utils::html_escape(&item.title);
    format!(
        r##"<div class="feedback-entry mt-2 border-l-4 border-emerald-500 bg-emerald-50 dark:bg-emerald-900/20 px-4 py-3 rounded-r-lg" role="status" data-wishlist-autolink="{item_id}">
            <p class="text-sm font-semibold text-emerald-700 dark:text-emerald-300">{title}</p>
            <p class="mt-1 text-sm text-emerald-700 dark:text-emerald-300">{body} <span class="font-medium">{item_title}</span></p>
            <div class="mt-2">
                <button type="button"
                        hx-get="/wishlist/{item_id}/delete-modal"
                        hx-target="#modal-slot"
                        hx-swap="innerHTML"
                        class="px-3 py-1 text-xs font-medium text-white bg-emerald-600 rounded hover:bg-emerald-700">
                    {remove}
                </button>
            </div>
        </div>"##,
        title = title,
        body = body,
        item_title = escaped_item_title,
        item_id = item.id,
        remove = remove,
    )
}

// ─── Print page ──────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/wishlist_print.html")]
pub struct WishlistPrintTemplate {
    pub items: Vec<WishlistItem>,
    pub generated_at: String,
    pub label_print_title: String,
    pub label_print_count_one: String,
    pub label_print_count_other: String,
    pub label_col_title: String,
    pub label_col_isbn: String,
    pub label_col_authors: String,
    pub label_col_publisher: String,
    pub label_col_notes: String,
    pub label_placeholder_empty: String,
    pub item_count: i64,
}

/// CR #266 — server-rendered PDF export. Returns a real PDF document
/// (text layer extractable via `pdftotext`, searchable in any viewer,
/// identical rendering everywhere). The Content-Disposition header
/// triggers the browser's download flow.
///
/// The legacy `/wishlist/print` HTML route is kept as a deep-link
/// fallback but no longer surfaced in the UI (the Print button on
/// `/wishlist` now points here).
pub async fn wishlist_export_pdf(
    State(state): State<AppState>,
    Extension(locale): Extension<Locale>,
) -> Result<Response, AppError> {
    let pool = &state.pool;
    let loc = locale.0;

    let items = WishlistItemModel::list_active(pool).await?;
    let bytes = crate::services::wishlist_pdf::render(&items, loc)?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let disposition = format!(r#"attachment; filename="wishlist-{today}.pdf""#);

    Ok((
        axum::http::StatusCode::OK,
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/pdf".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                disposition,
            ),
            // PDFs are deterministic per session-locale + DB state at
            // request time; no-store keeps a stale dump from sitting in
            // a proxy after the user adds entries.
            (
                axum::http::header::CACHE_CONTROL,
                "no-store".to_string(),
            ),
        ],
        bytes,
    )
        .into_response())
}

pub async fn wishlist_print(
    State(state): State<AppState>,
    Extension(locale): Extension<Locale>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;
    let loc = locale.0;
    let items = WishlistItemModel::list_active(pool).await?;
    let item_count = items.len() as i64;
    let generated_at = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();

    let template = WishlistPrintTemplate {
        items,
        generated_at,
        label_print_title: rust_i18n::t!("wishlist.print_title", locale = loc).to_string(),
        label_print_count_one: rust_i18n::t!("wishlist.print_count_one", locale = loc, count = 1).to_string(),
        label_print_count_other: rust_i18n::t!(
            "wishlist.print_count_other",
            locale = loc,
            count = item_count
        )
        .to_string(),
        label_col_title: rust_i18n::t!("wishlist.col_title", locale = loc).to_string(),
        label_col_isbn: rust_i18n::t!("wishlist.col_isbn", locale = loc).to_string(),
        label_col_authors: rust_i18n::t!("wishlist.col_authors", locale = loc).to_string(),
        label_col_publisher: rust_i18n::t!("wishlist.col_publisher", locale = loc).to_string(),
        label_col_notes: rust_i18n::t!("wishlist.col_notes", locale = loc).to_string(),
        label_placeholder_empty: rust_i18n::t!("title_detail.field_unset", locale = loc).to_string(),
        item_count,
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("wishlist print render".to_string())),
    }
}
