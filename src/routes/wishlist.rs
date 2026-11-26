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
//! - `GET  /wishlist/:id/delete-modal?action=bought|remove` — UX-DR8
//!   confirmation, "Mark as bought" (default) or "Remove" (story 20-1)
//! - `DELETE /wishlist/:id?action=bought|remove` — soft-delete; the action
//!   names the audit row (`wishlist_bought` / `wishlist_removed`). The scan
//!   banner (auto-link) opens the "bought" modal.
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
use axum::extract::{OriginalUri, Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::media_type::{CodeType, MediaType};
use crate::models::wishlist::{WishlistItem, WishlistItemDraft, WishlistItemModel};
use crate::utils::base_context;

// ─── List page ───────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/wishlist.html")]
pub struct WishlistPageTemplate {
    pub base: crate::utils::BaseContextFields,
    pub items: Vec<WishlistRow>,
    /// Story 15-1 (#602) — the instance zone for the "Added" dates.
    pub time_zone: chrono_tz::Tz,
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
    pub label_action_remove: String,
    pub label_placeholder_empty: String,
}

/// Story 20-1 — one list entry with the accessible names of its two
/// actions (they name the entry's title, WCAG 2.5.3), built in the
/// request locale.
pub struct WishlistRow {
    pub item: WishlistItem,
    pub bought_label: String,
    pub remove_label: String,
}

/// Story 20-1 — the accessible names of "Bought" and "Remove" for one entry.
fn action_labels(title: &str, loc: &str) -> (String, String) {
    (
        rust_i18n::t!("wishlist.action_bought_named", locale = loc, title = title).to_string(),
        rust_i18n::t!("wishlist.action_remove_named", locale = loc, title = title).to_string(),
    )
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

    let items = WishlistItemModel::list_active(pool)
        .await?
        .into_iter()
        .map(|item| {
            let (bought_label, remove_label) = action_labels(&item.title, loc);
            WishlistRow {
                item,
                bought_label,
                remove_label,
            }
        })
        .collect();

    let base = base_context(&session, loc, "wishlist", &uri, state.page_settings());
    let template = WishlistPageTemplate {
        base,
        items,
        time_zone: state.time_zone(),
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
        label_action_remove: rust_i18n::t!("wishlist.action_remove", locale = loc).to_string(),
        label_placeholder_empty: rust_i18n::t!("title_detail.field_unset", locale = loc).to_string(),
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
    pub base: crate::utils::BaseContextFields,
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
}

pub async fn wishlist_new_page(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;
    let loc = locale.0;

    let base = base_context(&session, loc, "wishlist", &uri, state.page_settings());
    let template = WishlistNewTemplate {
        base,
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

    // Story 17-3 (#565): the one ISBN rule; a blank field is a format error.
    // Issue #676 — the hint shows under the ISBN field.
    let isbn = match crate::utils::parse_isbn(&form.isbn) {
        Ok(isbn) => isbn,
        Err(e) => {
            return Ok(crate::middleware::htmx::field_refusal(
                axum::http::StatusCode::BAD_REQUEST,
                "isbn-input-error",
                &e.message(loc),
            ));
        }
    };
    let isbn = isbn.as_str();

    let timeout_secs = state
        .settings
        .read()
        .map(|s| s.metadata_fetch_timeout_secs)
        .unwrap_or(30);
    let per_provider_timeouts = state.metadata_chain_provider_timeouts();
    let metadata = crate::metadata::chain::ChainExecutor::execute(
        &state.registry,
        pool,
        isbn,
        &CodeType::Isbn,
        &MediaType::Book,
        timeout_secs,
        &per_provider_timeouts,
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
        match crate::utils::normalize_isbn_input(form.isbn.as_deref().unwrap_or("")) {
            Ok(isbn) => isbn,
            Err(e) => return Err(AppError::BadRequest(e.message(loc))),
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

    audit_wishlist_add(pool, &session, id, title).await;

    Ok(Redirect::to("/wishlist"))
}

/// Audit trail (#242 AC): record a wishlist add. Shared by the wishlist
/// form and the series page's "Add to wishlist" (story 11-11). `user_id`
/// is `None` only for anonymous sessions, which never reach a mutation.
/// Best effort: a failed audit write never fails the add.
pub(crate) async fn audit_wishlist_add(
    pool: &crate::db::DbPool,
    session: &Session,
    item_id: u64,
    title: &str,
) {
    if let Some(user_id) = session.user_id {
        crate::models::admin_audit::AdminAuditModel::create(
            pool,
            user_id,
            "wishlist_add",
            Some("wishlist_items"),
            Some(item_id),
            Some(serde_json::json!({"title": title})),
        )
        .await
        .ok();
    }
}

// ─── Detail page ─────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "pages/wishlist_detail.html")]
pub struct WishlistDetailTemplate {
    pub base: crate::utils::BaseContextFields,
    pub item: WishlistItem,
    /// Story 15-1 (#602) — the instance zone for the "Added" date.
    pub time_zone: chrono_tz::Tz,
    pub can_edit: bool,
    pub label_isbn: String,
    pub label_authors: String,
    pub label_publisher: String,
    pub label_year: String,
    pub label_notes: String,
    pub label_added_at: String,
    pub label_back: String,
    pub label_action_bought: String,
    pub label_action_remove: String,
    /// Story 20-1 — accessible names of the two actions (name the title).
    pub bought_label: String,
    pub remove_label: String,
    pub label_placeholder_empty: String,
    pub label_no_cover: String,
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

    let (bought_label, remove_label) = action_labels(&item.title, loc);
    let base = base_context(&session, loc, "wishlist", &uri, state.page_settings());
    let template = WishlistDetailTemplate {
        base,
        item,
        time_zone: state.time_zone(),
        can_edit: session.role >= Role::Librarian,
        label_isbn: rust_i18n::t!("wishlist.col_isbn", locale = loc).to_string(),
        label_authors: rust_i18n::t!("wishlist.col_authors", locale = loc).to_string(),
        label_publisher: rust_i18n::t!("wishlist.col_publisher", locale = loc).to_string(),
        label_year: rust_i18n::t!("wishlist.field_year", locale = loc).to_string(),
        label_notes: rust_i18n::t!("wishlist.field_notes", locale = loc).to_string(),
        label_added_at: rust_i18n::t!("wishlist.col_added", locale = loc).to_string(),
        label_back: rust_i18n::t!("wishlist.back_to_list", locale = loc).to_string(),
        label_action_bought: rust_i18n::t!("wishlist.action_bought", locale = loc).to_string(),
        label_action_remove: rust_i18n::t!("wishlist.action_remove", locale = loc).to_string(),
        bought_label,
        remove_label,
        label_placeholder_empty: rust_i18n::t!("title_detail.field_unset", locale = loc).to_string(),
        label_no_cover: rust_i18n::t!("cover.no_cover", locale = loc).to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("wishlist detail render".to_string())),
    }
}

// ─── Delete modal (UX-DR8) ───────────────────────────────────────

/// Story 20-1 (D1, D5) — what a wish list entry leaves the list for:
/// bought (the default, the scan banner's) or removed without buying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WishlistAction {
    Bought,
    Remove,
}

impl WishlistAction {
    /// Positive shape (Rule #20): exactly `remove` is Remove; anything
    /// else — absent, empty, another case, another word — is Bought.
    pub fn from_query(value: Option<&str>) -> Self {
        match value {
            Some("remove") => WishlistAction::Remove,
            _ => WishlistAction::Bought,
        }
    }

    /// The `admin_audit.action` recorded on Confirm.
    pub fn audit_action(self) -> &'static str {
        match self {
            WishlistAction::Bought => "wishlist_bought",
            WishlistAction::Remove => "wishlist_removed",
        }
    }

    /// The `?action=` value that selects this action.
    pub fn query_value(self) -> &'static str {
        match self {
            WishlistAction::Bought => "bought",
            WishlistAction::Remove => "remove",
        }
    }

    /// The modal's title, body text and confirm label for `entry_title`.
    pub fn modal_copy(self, entry_title: &str, loc: &str) -> (String, String, String) {
        let (title_key, body_key, confirm_key) = match self {
            WishlistAction::Bought => (
                "wishlist.delete_modal_title",
                "wishlist.delete_modal_body",
                "wishlist.action_bought",
            ),
            WishlistAction::Remove => (
                "wishlist.remove_modal_title",
                "wishlist.remove_modal_body",
                "wishlist.action_remove",
            ),
        };
        (
            rust_i18n::t!(title_key, locale = loc, title = entry_title).to_string(),
            rust_i18n::t!(body_key, locale = loc).to_string(),
            rust_i18n::t!(confirm_key, locale = loc).to_string(),
        )
    }
}

/// `?action=` of the modal and delete routes. Unknown fields (htmx 2
/// appends `_csrf_token` to a DELETE URL) are ignored.
#[derive(Deserialize)]
pub struct WishlistActionQuery {
    pub action: Option<String>,
}

impl WishlistActionQuery {
    fn action(&self) -> WishlistAction {
        WishlistAction::from_query(self.action.as_deref())
    }
}

/// Story 20-1 (AC5, D7) — the entry is no longer live (removed in another
/// tab or by another librarian).
fn already_removed(loc: &str) -> AppError {
    AppError::NotFound(rust_i18n::t!("wishlist.already_removed", locale = loc).to_string())
}

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
    Query(query): Query<WishlistActionQuery>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Librarian, "/wishlist", locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    if !is_htmx {
        return Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response());
    }

    let item = WishlistItemModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| already_removed(loc))?;
    let action = query.action();
    let (title, body_text, confirm_label) = action.modal_copy(&item.title, loc);
    let body_html = format!("<p>{}</p>", crate::utils::html_escape(&body_text));

    let template = WishlistDeleteModalTemplate {
        title,
        body_html,
        confirm_label,
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/wishlist/{}?action={}", item.id, action.query_value()),
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
    Query(query): Query<WishlistActionQuery>,
) -> Result<Response, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;
    let action = query.action();

    // Story 20-1 (AC5): gone, or soft-deleted between the read and the
    // write (an item's version moves only through `soft_delete`) — both
    // answer "already removed" and write nothing.
    let item = WishlistItemModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| already_removed(loc))?;
    let n = WishlistItemModel::soft_delete(pool, item.id, item.version).await?;
    if n == 0 {
        return Err(already_removed(loc));
    }

    // Audit trail entry (#242 AC; story 20-1: the action names the row).
    if let Some(user_id) = session.user_id {
        crate::models::admin_audit::AdminAuditModel::create(
            pool,
            user_id,
            action.audit_action(),
            Some("wishlist_items"),
            Some(item.id),
            Some(serde_json::json!({"title": item.title, "isbn": item.isbn})),
        )
        .await
        .ok();
    }

    // HX-Redirect drives a full-page navigation back to /wishlist with
    // the entry removed from the list (D3). Consistent with the volume-
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
/// confirmation modal at `/wishlist/:id/delete-modal` (no `?action=`: a
/// scanned ISBN means the book is in hand, story 20-1 D1). No silent
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
    // Story 15-1 (#602): "today" is the instance zone's, never the
    // server's (the container's TZ must not matter).
    let clock = state.local_clock();
    let bytes = crate::services::wishlist_pdf::render(&items, loc, clock.today())?;

    let today = clock.today();
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
    let clock = state.local_clock();
    let generated_at =
        crate::utils::local_time::format_local_date_time(clock.now_utc(), clock.tz());

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

#[cfg(test)]
mod tests {
    use super::*;

    const LOCALES: [&str; 4] = ["en", "fr", "de", "it"];

    #[test]
    fn only_the_exact_word_remove_selects_remove() {
        assert_eq!(WishlistAction::from_query(Some("remove")), WishlistAction::Remove);
        for other in [None, Some(""), Some("bought"), Some("REMOVE"), Some("Remove"), Some(" remove"), Some("delete"), Some("x")] {
            assert_eq!(WishlistAction::from_query(other), WishlistAction::Bought, "{other:?}");
        }
    }

    #[test]
    fn each_action_round_trips_through_its_query_value_and_names_its_audit_row() {
        for action in [WishlistAction::Bought, WishlistAction::Remove] {
            assert_eq!(WishlistAction::from_query(Some(action.query_value())), action);
        }
        assert_eq!(WishlistAction::Bought.audit_action(), "wishlist_bought");
        assert_eq!(WishlistAction::Remove.audit_action(), "wishlist_removed");
    }

    #[test]
    fn the_query_ignores_unknown_fields() {
        let q: WishlistActionQuery =
            serde_urlencoded::from_str("action=remove&_csrf_token=abc").unwrap();
        assert_eq!(q.action(), WishlistAction::Remove);
        let q: WishlistActionQuery = serde_urlencoded::from_str("_csrf_token=abc").unwrap();
        assert_eq!(q.action(), WishlistAction::Bought);
    }

    #[test]
    fn each_action_has_its_own_copy_naming_the_entry_in_every_locale() {
        for loc in LOCALES {
            let (b_title, b_body, b_confirm) = WishlistAction::Bought.modal_copy("Dune", loc);
            let (r_title, r_body, r_confirm) = WishlistAction::Remove.modal_copy("Dune", loc);
            assert!(b_title.contains("Dune") && r_title.contains("Dune"), "{loc}");
            assert_ne!(b_title, r_title, "{loc}");
            assert_ne!(b_body, r_body, "{loc}");
            assert_eq!(b_confirm, rust_i18n::t!("wishlist.action_bought", locale = loc));
            assert_eq!(r_confirm, rust_i18n::t!("wishlist.action_remove", locale = loc));
            for text in [&b_title, &b_body, &r_title, &r_body] {
                assert!(!text.contains("wishlist."), "{loc}: missing key {text}");
                assert!(!text.contains("%{"), "{loc}: unfilled placeholder {text}");
            }
        }
        let (title, body, confirm) = WishlistAction::Remove.modal_copy("Dune", "en");
        assert_eq!(title, "Remove “Dune” from the wish list?");
        assert!(body.contains("cannot be restored"), "{body}");
        assert_eq!(confirm, "Remove");
        let (title, _, confirm) = WishlistAction::Bought.modal_copy("Dune", "fr");
        assert_eq!(title, "Marquer « Dune » comme acheté ?");
        assert_eq!(confirm, "Acheté");
    }

    #[test]
    fn no_modal_claims_a_trash_restore_in_any_locale() {
        for (loc, trash) in [("en", "trash"), ("fr", "corbeille"), ("de", "papierkorb"), ("it", "cestino")] {
            for action in [WishlistAction::Bought, WishlistAction::Remove] {
                let (_, body, _) = action.modal_copy("Dune", loc);
                let lower = body.to_lowercase();
                assert!(!lower.contains(trash), "{loc} {action:?}: {body}");
                assert!(!body.contains("30"), "{loc} {action:?}: {body}");
            }
        }
    }

    #[test]
    fn the_action_names_carry_the_title_and_the_visible_word() {
        for loc in LOCALES {
            let (bought, remove) = action_labels("Dune", loc);
            assert!(bought.contains("Dune") && remove.contains("Dune"), "{loc}");
            // WCAG 2.5.3 — the accessible name starts with the visible label
            // (project rule, decided 2026-11-26, #734).
            let visible_bought = rust_i18n::t!("wishlist.action_bought", locale = loc).to_string();
            let visible_remove = rust_i18n::t!("wishlist.action_remove", locale = loc).to_string();
            assert!(bought.starts_with(&visible_bought), "{loc}: {bought}");
            assert!(remove.starts_with(&visible_remove), "{loc}: {remove}");
        }
    }
}
