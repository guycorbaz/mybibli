use askama::Template;
use axum::Extension;
use axum::extract::{OriginalUri, Path, State};
use axum::response::{Html, IntoResponse};

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::contributor::{ContributorModel, ContributorTitleRow};
use crate::utils::html_escape;

#[derive(Template)]
#[template(path = "pages/contributor_detail.html")]
pub struct ContributorDetailTemplate {
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
    pub contributor: ContributorModel,
    pub titles: Vec<ContributorTitleRow>,
    pub label_titles: String,
    pub delete_label: String,
    pub current_url: String,
    pub lang_toggle_aria: String,
}

pub async fn contributor_detail(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    OriginalUri(uri): OriginalUri,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;
    let loc = locale.0;

    let (contributor, titles) = ContributorModel::find_by_id_with_titles(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    if is_htmx {
        let html = contributor_detail_fragment(&contributor, &titles, loc);
        Ok(Html(html).into_response())
    } else {
        let base = crate::utils::base_context(&session, loc, "contributor", &uri, state.session_timeout_secs());
        let template = ContributorDetailTemplate {
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
            contributor,
            titles,
            label_titles: rust_i18n::t!("contributor_detail.titles", locale = loc).to_string(),
            delete_label: rust_i18n::t!("contributor_detail.delete", locale = loc).to_string(),
            current_url: base.current_url,
            lang_toggle_aria: base.lang_toggle_aria,
        };
        match template.render() {
            Ok(html) => Ok(Html(html).into_response()),
            Err(_) => Err(AppError::TemplateRenderFailed { template: "contributors" }),
        }
    }
}

fn contributor_detail_fragment(
    contributor: &ContributorModel,
    titles: &[ContributorTitleRow],
    loc: &str,
) -> String {
    let escaped_name = html_escape(&contributor.name);
    let bio_html = contributor
        .biography
        .as_ref()
        .map(|b| {
            format!(
                r#"<p class="mt-2 text-stone-600 dark:text-stone-400">{}</p>"#,
                html_escape(b)
            )
        })
        .unwrap_or_default();

    let titles_html: String = titles
        .iter()
        .map(|t| {
            format!(
                r#"<li><a href="/title/{}" class="text-indigo-600 dark:text-indigo-400 hover:underline">{}</a> <span class="text-stone-500">({})</span></li>"#,
                t.title_id,
                html_escape(&t.title),
                html_escape(&t.role_name)
            )
        })
        .collect();

    format!(
        r#"<div class="max-w-4xl mx-auto px-4 py-8">
            <h1 class="text-2xl font-bold text-stone-900 dark:text-stone-100">{}</h1>
            {}
            <div class="mt-6">
                <h2 class="text-lg font-semibold text-stone-800 dark:text-stone-200">{}</h2>
                <ul class="mt-2 space-y-1">{}</ul>
            </div>
        </div>"#,
        escaped_name,
        bio_html,
        rust_i18n::t!("contributor_detail.titles", locale = loc),
        titles_html
    )
}

// ─── Delete confirmation modal (story 9-12) ─────────────

#[derive(Template)]
#[template(path = "fragments/contributor_delete_modal.html")]
pub struct ContributorDeleteModalTemplate {
    pub title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
}

/// `GET /contributor/:id/delete-modal` — returns the rendered UX-DR8 Modal
/// fragment for the destructive delete-contributor flow. Librarian-gated
/// (admin > librarian, both pass). Direct browser navigation (no
/// `HX-Request` header) returns 405 — the modal fragment is meaningless
/// without page context. No `Allow:` response header is emitted (per the
/// 9-11 code-review patch — `Allow: GET` self-contradicts 405; we DO
/// support GET, just not without HTMX).
pub async fn delete_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<axum::response::Response, AppError> {
    // Preserve the contributor-detail return path so an anonymous user who
    // hits this URL directly (or whose session expired) lands back on the
    // contributor page after login, not on /home.
    session.require_role_with_return(Role::Librarian, &format!("/contributor/{id}"), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    if !is_htmx {
        return Ok(axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response());
    }

    // CR #136: see borrowers::delete_modal — same concurrent-delete UX
    // gap. The contributor-detail page declares `#contributor-feedback`,
    // not `#feedback-list`, so the AppError::NotFound default retarget
    // would land nowhere.
    let contributor = match ContributorModel::find_by_id(pool, id).await? {
        Some(c) => c,
        None => {
            return Ok(crate::routes::build_already_deleted_response(
                loc,
                "#contributor-feedback",
            ));
        }
    };

    // Title carries the contributor name via `%{name}` interpolation. Pass
    // the RAW name through `t!()` and let Askama's default auto-escape
    // (on `{{ title }}` in the macro) handle HTML safety. Pre-escaping
    // would double-escape (`<` → `&lt;` → `&amp;lt;`).
    let title = rust_i18n::t!(
        "contributor.delete_modal_title",
        locale = loc,
        name = contributor.name.as_str()
    )
    .to_string();
    let body_text = rust_i18n::t!("contributor.delete_modal_body", locale = loc).to_string();
    let body_html = format!("<p>{}</p>", crate::utils::html_escape(&body_text));

    tracing::debug!(contributor_id = id, "delete modal requested");

    let template = ContributorDeleteModalTemplate {
        title,
        body_html,
        confirm_label: rust_i18n::t!("contributor.delete_modal_confirm", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/catalog/contributors/{}", contributor.id),
        csrf_token: session.csrf_token.clone(),
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => Err(AppError::Internal(format!(
            "contributor delete modal render: {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contributor_detail_fragment_renders() {
        let contributor = ContributorModel {
            id: 1,
            name: "Albert Camus".to_string(),
            biography: Some("French-Algerian author".to_string()),
            version: 1,
        };
        let titles = vec![ContributorTitleRow {
            title_id: 42,
            title: "L'Étranger".to_string(),
            media_type: "book".to_string(),
            role_name: "Auteur".to_string(),
        }];
        let html = contributor_detail_fragment(&contributor, &titles, "en");
        assert!(html.contains("Albert Camus"));
        assert!(html.contains("/title/42"));
    }
}
