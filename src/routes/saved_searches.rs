//! Saved searches (CR #367) — named, re-runnable bundles of the home
//! browse state (`q`/`filter`/`sort`/`dir`). GLOBAL (single-tenant), so the
//! whole instance shares one set. Surfaced as a dropdown on the home search
//! bar; edit (rename) and delete go through UX-DR8 modals (`#modal-slot` +
//! `static/js/modal.js`). All mutating handlers are Librarian+.
//!
//! Run path is just a link to `/?q=...&filter=...&sort=...&dir=...` — no
//! dedicated route; the home handler validates the criteria as usual.

use askama::Template;
use axum::Form;
use axum::extract::{Path, Query, State};
use axum::response::Html;
use serde::Deserialize;

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::CreateOutcome;
use crate::models::saved_search::SavedSearchModel;
use crate::utils::{feedback_html, html_escape, url_encode};

const RETURN_PATH: &str = "/";

// ─── Form structs ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateSavedSearchForm {
    pub name: String,
    pub q: Option<String>,
    pub filter: Option<String>,
    pub sort: Option<String>,
    pub dir: Option<String>,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct RenameSavedSearchForm {
    pub name: String,
    pub version: i32,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct DeleteSavedSearchForm {
    pub version: i32,
    pub _csrf_token: String,
}

#[derive(Debug, Deserialize)]
pub struct VersionQuery {
    pub version: i32,
}

// ─── Display + template structs ────────────────────────────────────

#[derive(Debug, Clone)]
struct SavedSearchRowDisplay {
    id: u64,
    name: String,
    run_url: String,
    version: i32,
    run_aria: String,
    edit_aria: String,
    delete_aria: String,
}

#[derive(Template)]
#[template(path = "fragments/saved_searches_list.html")]
struct SavedSearchesList {
    rows: Vec<SavedSearchRowDisplay>,
    empty_label: String,
    edit_label: String,
    delete_label: String,
}

/// Full home search-bar control: toggle button + dropdown panel + the
/// "save current search" form + the list (included from the same list
/// fragment so the row markup stays single-sourced). Rendered by the home
/// handler; the save-form's hidden inputs carry the CURRENT browse state.
#[derive(Template)]
#[template(path = "fragments/saved_searches_control.html")]
struct SavedSearchControl {
    csrf_token: String,
    cur_q: String,
    cur_filter: String,
    cur_sort: String,
    cur_dir: String,
    toggle_label: String,
    name_label: String,
    name_placeholder: String,
    save_label: String,
    // Fields below feed the `{% include %}`d saved_searches_list.html.
    rows: Vec<SavedSearchRowDisplay>,
    empty_label: String,
    edit_label: String,
    delete_label: String,
}

#[derive(Template)]
#[template(path = "fragments/saved_search_rename_modal.html")]
struct SavedSearchRenameModal {
    csrf_token: String,
    title: String,
    name_label: String,
    confirm_label: String,
    cancel_label: String,
    action_url: String,
    version: i32,
    current_name: String,
}

#[derive(Template)]
#[template(path = "fragments/saved_search_delete_modal.html")]
struct SavedSearchDeleteModal {
    csrf_token: String,
    title: String,
    body_html: String,
    confirm_label: String,
    cancel_label: String,
    action_url: String,
    version: i32,
}

// ─── Helpers ───────────────────────────────────────────────────────

fn validate_name(name: &str, loc: &'static str) -> Result<String, AppError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.reference_data.name_empty", locale = loc).to_string(),
        ));
    }
    if trimmed.chars().count() > 255 {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.reference_data.name_too_long", locale = loc).to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Normalize an optional form field: trim, then `None` if empty. Keeps the
/// stored criteria clean (no empty-string `q`) so the run URL stays tidy.
fn norm(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Build the home browse URL that re-runs a saved search.
fn build_run_url(s: &SavedSearchModel) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(q) = s.q.as_deref().filter(|v| !v.is_empty()) {
        parts.push(format!("q={}", url_encode(q)));
    }
    if let Some(f) = s.filter.as_deref().filter(|v| !v.is_empty()) {
        parts.push(format!("filter={}", url_encode(f)));
    }
    if let Some(sort) = s.sort.as_deref().filter(|v| !v.is_empty()) {
        parts.push(format!("sort={}", url_encode(sort)));
    }
    if let Some(dir) = s.dir.as_deref().filter(|v| !v.is_empty()) {
        parts.push(format!("dir={}", url_encode(dir)));
    }
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/?{}", parts.join("&"))
    }
}

fn make_row(s: &SavedSearchModel, loc: &'static str) -> SavedSearchRowDisplay {
    SavedSearchRowDisplay {
        id: s.id,
        name: s.name.clone(),
        run_url: build_run_url(s),
        version: s.version,
        run_aria: rust_i18n::t!("saved_searches.run_aria", locale = loc, name = &s.name).to_string(),
        edit_aria: rust_i18n::t!("saved_searches.edit_aria", locale = loc, name = &s.name)
            .to_string(),
        delete_aria: rust_i18n::t!("saved_searches.delete_aria", locale = loc, name = &s.name)
            .to_string(),
    }
}

/// Render the `#saved-searches-list` fragment. Shared by the home handler
/// (initial render) and every mutating handler (refresh after save/rename/
/// delete). `pub(crate)` so `routes::home` can populate `saved_searches_html`.
pub(crate) async fn render_list_html(pool: &crate::db::DbPool, loc: &'static str) -> Result<String, AppError> {
    let searches = SavedSearchModel::list_all(pool).await?;
    let rows = searches.iter().map(|s| make_row(s, loc)).collect();
    SavedSearchesList {
        rows,
        empty_label: rust_i18n::t!("saved_searches.empty", locale = loc).to_string(),
        edit_label: rust_i18n::t!("saved_searches.btn_rename", locale = loc).to_string(),
        delete_label: rust_i18n::t!("saved_searches.btn_delete", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("saved searches list render failed".to_string()))
}

/// Render the whole home search-bar control (toggle + panel + save-form +
/// list). `pub(crate)` so `routes::home` calls it with the current browse
/// state. The list rows are shared with `render_list_html` via the included
/// `saved_searches_list.html` fragment.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn render_control_html(
    pool: &crate::db::DbPool,
    loc: &'static str,
    csrf_token: &str,
    cur_q: &str,
    cur_filter: &str,
    cur_sort: &str,
    cur_dir: &str,
) -> Result<String, AppError> {
    let searches = SavedSearchModel::list_all(pool).await?;
    let rows = searches.iter().map(|s| make_row(s, loc)).collect();
    SavedSearchControl {
        csrf_token: csrf_token.to_string(),
        cur_q: cur_q.to_string(),
        cur_filter: cur_filter.to_string(),
        cur_sort: cur_sort.to_string(),
        cur_dir: cur_dir.to_string(),
        toggle_label: rust_i18n::t!("saved_searches.toggle_label", locale = loc).to_string(),
        name_label: rust_i18n::t!("saved_searches.name_label", locale = loc).to_string(),
        name_placeholder: rust_i18n::t!("saved_searches.name_placeholder", locale = loc)
            .to_string(),
        save_label: rust_i18n::t!("saved_searches.save_label", locale = loc).to_string(),
        rows,
        empty_label: rust_i18n::t!("saved_searches.empty", locale = loc).to_string(),
        edit_label: rust_i18n::t!("saved_searches.btn_rename", locale = loc).to_string(),
        delete_label: rust_i18n::t!("saved_searches.btn_delete", locale = loc).to_string(),
    }
    .render()
    .map_err(|_| AppError::Internal("saved searches control render failed".to_string()))
}

fn success_feedback(loc: &'static str, key: &str, name: &str) -> String {
    let msg = rust_i18n::t!(key, locale = loc, name = name).to_string();
    feedback_html("success", &msg, "")
}

// ─── Handlers ──────────────────────────────────────────────────────

pub async fn create(
    State(state): State<AppState>,
    session: Session,
    axum::Extension(locale): axum::Extension<Locale>,
    Form(form): Form<CreateSavedSearchForm>,
) -> Result<HtmxResponse, AppError> {
    session.require_role_with_return(Role::Librarian, RETURN_PATH, locale.0)?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;
    let q = norm(form.q);
    let filter = norm(form.filter);
    let sort = norm(form.sort);
    let dir = norm(form.dir);

    let outcome = SavedSearchModel::create(
        &state.pool,
        &name,
        q.as_deref(),
        filter.as_deref(),
        sort.as_deref(),
        dir.as_deref(),
    )
    .await?;
    let feedback = match outcome {
        CreateOutcome::Created(_) => success_feedback(loc, "success.saved_search.created", &name),
        CreateOutcome::Reactivated(_) => {
            success_feedback(loc, "success.saved_search.reactivated", &name)
        }
    };
    let list_html = render_list_html(&state.pool, loc).await?;
    Ok(HtmxResponse {
        main: list_html,
        oob: vec![OobUpdate {
            swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    })
}

pub async fn rename_modal(
    State(state): State<AppState>,
    session: Session,
    axum::Extension(locale): axum::Extension<Locale>,
    Path(id): Path<u64>,
    Query(q): Query<VersionQuery>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Librarian, RETURN_PATH, locale.0)?;
    let loc = locale.0;
    let row = SavedSearchModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("saved_search".to_string()))?;
    let html = SavedSearchRenameModal {
        csrf_token: session.csrf_token.clone(),
        title: rust_i18n::t!("saved_searches.rename_modal_heading", locale = loc).to_string(),
        name_label: rust_i18n::t!("saved_searches.name_label", locale = loc).to_string(),
        confirm_label: rust_i18n::t!("saved_searches.btn_rename", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("saved_searches.btn_cancel", locale = loc).to_string(),
        action_url: format!("/saved-searches/{}/rename", id),
        version: q.version,
        current_name: row.name,
    }
    .render()
    .map_err(|_| AppError::Internal("rename modal render failed".to_string()))?;
    Ok(Html(html))
}

pub async fn rename(
    State(state): State<AppState>,
    session: Session,
    axum::Extension(locale): axum::Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<RenameSavedSearchForm>,
) -> Result<axum::response::Response, AppError> {
    session.require_role_with_return(Role::Librarian, RETURN_PATH, locale.0)?;
    let loc = locale.0;
    let name = validate_name(&form.name, loc)?;
    SavedSearchModel::rename(&state.pool, id, form.version, &name).await?;
    let list_html = render_list_html(&state.pool, loc).await?;
    let feedback = success_feedback(loc, "success.saved_search.renamed", &name);
    Ok(HtmxResponse {
        main: list_html,
        oob: vec![OobUpdate {
            swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    }
    .into_response_with_hx_trigger("modal-close"))
}

pub async fn delete_modal(
    State(state): State<AppState>,
    session: Session,
    axum::Extension(locale): axum::Extension<Locale>,
    Path(id): Path<u64>,
    Query(q): Query<VersionQuery>,
) -> Result<Html<String>, AppError> {
    session.require_role_with_return(Role::Librarian, RETURN_PATH, locale.0)?;
    let loc = locale.0;
    let row = SavedSearchModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("saved_search".to_string()))?;
    let body = rust_i18n::t!(
        "saved_searches.delete_modal_body",
        locale = loc,
        name = html_escape(&row.name)
    )
    .to_string();
    let html = SavedSearchDeleteModal {
        csrf_token: session.csrf_token.clone(),
        title: rust_i18n::t!("saved_searches.delete_modal_heading", locale = loc).to_string(),
        body_html: body,
        confirm_label: rust_i18n::t!("saved_searches.btn_delete", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("saved_searches.btn_cancel", locale = loc).to_string(),
        action_url: format!("/saved-searches/{}/delete", id),
        version: q.version,
    }
    .render()
    .map_err(|_| AppError::Internal("delete modal render failed".to_string()))?;
    Ok(Html(html))
}

pub async fn delete(
    State(state): State<AppState>,
    session: Session,
    axum::Extension(locale): axum::Extension<Locale>,
    Path(id): Path<u64>,
    Form(form): Form<DeleteSavedSearchForm>,
) -> Result<axum::response::Response, AppError> {
    session.require_role_with_return(Role::Librarian, RETURN_PATH, locale.0)?;
    let loc = locale.0;
    let row = SavedSearchModel::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("saved_search".to_string()))?;
    SavedSearchModel::soft_delete(&state.pool, id, form.version).await?;
    let list_html = render_list_html(&state.pool, loc).await?;
    let feedback = success_feedback(loc, "success.saved_search.deleted", &row.name);
    Ok(HtmxResponse {
        main: list_html,
        oob: vec![OobUpdate {
            swap_mode: Default::default(),
            target: "feedback-list".to_string(),
            content: feedback,
        }],
    }
    .into_response_with_hx_trigger("modal-close"))
}
