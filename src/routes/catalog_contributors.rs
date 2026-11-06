//! Contributor routes of `/catalog` (search, add, remove, update, delete, forms).
//!
//! Split out of `routes/catalog.rs` by story 16-1 (#597, Foundation Rule #12).

use askama::Template;
use axum::Extension;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Redirect};
use serde::Deserialize;

use crate::AppState;
use crate::routes::catalog::context_banner_html;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::{HtmxResponse, HxRequest, OobUpdate};
use crate::middleware::locale::Locale;
use crate::models::contributor::{ContributorModel, ContributorRoleModel, TitleContributorModel};
use crate::models::session::SessionModel;
use crate::models::volume::VolumeModel;
use crate::services::contributor::ContributorService;
use crate::utils::feedback_html;
use crate::utils::html_escape;

#[derive(Deserialize)]
pub struct ContributorSearchQuery {
    pub q: String,
}

pub async fn contributor_search(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    uri: axum::http::Uri,
    axum::extract::Query(query): axum::extract::Query<ContributorSearchQuery>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;

    let q = query.q.trim();
    if q.len() < 2 || q.len() > 255 {
        return Ok(axum::Json(serde_json::json!([])).into_response());
    }

    let pool = &state.pool;
    let results = ContributorModel::search_by_name(pool, q, 10).await?;

    let json: Vec<serde_json::Value> = results
        .iter()
        .map(|c| serde_json::json!({"id": c.id, "name": c.name}))
        .collect();

    Ok(axum::Json(json).into_response())
}

#[derive(Deserialize)]
pub struct AddContributorForm {
    pub title_id: u64,
    pub contributor_name: String,
    pub role_id: u64,
}

pub async fn add_contributor(
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(_is_htmx): HxRequest,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<AddContributorForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;

    let pool = &state.pool;

    match ContributorService::add_to_title(
        pool,
        form.title_id,
        &form.contributor_name,
        form.role_id,
    )
    .await
    {
        Ok((contributor, role_name)) => {
            // Build contributor list OOB
            let contributors = TitleContributorModel::find_by_title(pool, form.title_id).await?;
            let list_html = contributor_list_html(&contributors);

            // Update banner with author
            let vol_count = VolumeModel::count_by_title(pool, form.title_id)
                .await
                .unwrap_or(0);
            let title = crate::models::title::TitleModel::find_by_id(pool, form.title_id).await?;
            let title_name = title.as_ref().map(|t| t.title.as_str()).unwrap_or("?");

            let message = rust_i18n::t!(
                "contributor.added",
                name = &contributor.name,
                role = &role_name,
                title = title_name
            )
            .to_string();
            let author = TitleContributorModel::get_primary_contributor(pool, form.title_id)
                .await
                .unwrap_or(None);

            let mut oob = vec![OobUpdate { swap_mode: Default::default(),
                target: "contributor-list".to_string(),
                content: list_html,
            }];

            if let Some(t) = &title {
                oob.push(OobUpdate { swap_mode: Default::default(),
                    target: "context-banner".to_string(),
                    content: context_banner_html(
                        &t.title,
                        &t.media_type,
                        vol_count,
                        author.as_deref(),
                    ),
                });
            }

            let resp = HtmxResponse {
                main: feedback_html("success", &message, ""),
                oob,
            };
            Ok(resp.into_response())
        }
        Err(e) => {
            let message = match &e {
                AppError::BadRequest(msg) => msg.clone(),
                _ => rust_i18n::t!("error.internal").to_string(),
            };
            Ok(Html(feedback_html("error", &message, "")).into_response())
        }
    }
}

#[derive(Deserialize)]
pub struct RemoveContributorForm {
    pub junction_id: u64,
    pub title_id: u64,
}

pub async fn remove_contributor(
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(_is_htmx): HxRequest,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<RemoveContributorForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;

    let pool = &state.pool;
    ContributorService::remove_from_title(pool, form.junction_id).await?;

    let message = rust_i18n::t!("contributor.removed").to_string();

    let contributors = TitleContributorModel::find_by_title(pool, form.title_id).await?;
    let list_html = contributor_list_html(&contributors);

    // Update banner (author may have changed)
    let vol_count = VolumeModel::count_by_title(pool, form.title_id)
        .await
        .unwrap_or(0);
    let title = crate::models::title::TitleModel::find_by_id(pool, form.title_id).await?;
    let author = TitleContributorModel::get_primary_contributor(pool, form.title_id)
        .await
        .unwrap_or(None);

    let mut oob = vec![OobUpdate { swap_mode: Default::default(),
        target: "contributor-list".to_string(),
        content: list_html,
    }];

    if let Some(t) = &title {
        oob.push(OobUpdate { swap_mode: Default::default(),
            target: "context-banner".to_string(),
            content: context_banner_html(&t.title, &t.media_type, vol_count, author.as_deref()),
        });
    }

    let resp = HtmxResponse {
        main: feedback_html("success", &message, ""),
        oob,
    };
    Ok(resp.into_response())
}

#[derive(Deserialize)]
pub struct UpdateContributorForm {
    pub id: u64,
    pub name: String,
    pub biography: Option<String>,
    #[serde(default)]
    pub title_id: Option<u64>,
}

pub async fn update_contributor(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    axum::Form(form): axum::Form<UpdateContributorForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;

    let pool = &state.pool;
    let bio = form.biography.as_deref();

    match ContributorService::update_details(pool, form.id, &form.name, bio).await {
        Ok(()) => {
            let message = rust_i18n::t!("contributor.updated").to_string();

            // Refresh contributor list if title_id provided
            if let Some(title_id) = form.title_id {
                let contributors = TitleContributorModel::find_by_title(pool, title_id).await?;
                let list_html = contributor_list_html(&contributors);
                let resp = HtmxResponse {
                    main: feedback_html("success", &message, ""),
                    oob: vec![OobUpdate { swap_mode: Default::default(),
                        target: "contributor-list".to_string(),
                        content: list_html,
                    }],
                };
                return Ok(resp.into_response());
            }

            Ok(Html(feedback_html("success", &message, "")).into_response())
        }
        Err(e) => {
            let message = match &e {
                AppError::BadRequest(msg) => msg.clone(),
                _ => rust_i18n::t!("error.internal").to_string(),
            };
            Ok(Html(feedback_html("error", &message, "")).into_response())
        }
    }
}

/// `DELETE /catalog/contributors/:id` — soft-deletes a contributor row,
/// gated by the FR54 active-titles guard (returns 200 + inline feedback
/// if the contributor still has at least one `title_contributors` row).
///
/// Trigger UX: see `GET /contributor/:id/delete-modal` (story 9-12) —
/// the destructive-confirm UX migrated from `hx-confirm=` to the UX-DR8
/// Modal component. This handler's contract is unchanged across that
/// migration: HX-Redirect to `/catalog` on soft-delete success, inline
/// `feedback_html` on FR54 Conflict.
pub async fn delete_contributor(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    HxRequest(is_htmx): HxRequest,
    axum::extract::Path(id): axum::extract::Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian, locale.0)?;

    let pool = &state.pool;

    match ContributorService::delete_contributor(pool, id).await {
        Ok(()) => {
            if is_htmx {
                Ok((
                    axum::http::StatusCode::OK,
                    [(
                        axum::http::header::HeaderName::from_static("hx-redirect"),
                        "/catalog".to_string(),
                    )],
                    String::new(),
                )
                    .into_response())
            } else {
                Ok(Redirect::to("/catalog").into_response())
            }
        }
        Err(e) => {
            let message = match &e {
                AppError::Conflict(msg) => msg.clone(),
                AppError::NotFound(msg) => msg.clone(),
                _ => rust_i18n::t!("error.internal").to_string(),
            };
            Ok(Html(feedback_html("error", &message, "")).into_response())
        }
    }
}

#[derive(Template)]
#[template(path = "components/contributor_form.html")]
struct ContributorFormTemplate {
    form_heading: String,
    label_name: String,
    label_role: String,
    label_submit: String,
    title_id: u64,
    roles: Vec<(u64, String)>,
}

pub async fn contributor_form_page(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    uri: axum::http::Uri,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;

    let pool = &state.pool;

    // Get current title from session
    let title_id = match &session.token {
        Some(token) => SessionModel::get_current_title_id(pool, token)
            .await?
            .unwrap_or(0),
        None => 0,
    };

    let roles = ContributorRoleModel::find_all(pool).await?;

    let template = ContributorFormTemplate {
        form_heading: rust_i18n::t!("contributor.form.add_button").to_string(),
        label_name: rust_i18n::t!("contributor.form.name").to_string(),
        label_role: rust_i18n::t!("contributor.form.role").to_string(),
        label_submit: rust_i18n::t!("contributor.form.submit").to_string(),
        title_id,
        roles,
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render contributor form");
            Err(AppError::TemplateRenderFailed { template: "catalog" })
        }
    }
}

/// Fix #318 — render the contributor "Add" form for a specific
/// title taken from the URL path (NOT the catalog-flow session).
/// Used by the title detail page so a librarian can add a
/// contributor to an already-cataloged title without going
/// through the /catalog scan flow. Shares the form template
/// (`templates/components/contributor_form.html`) — same
/// `hx-post="/catalog/contributors/add"` + OOB-into-`#contributor-list`
/// contract.
pub async fn contributor_form_for_title(
    session: Session,
    Extension(locale): Extension<Locale>,
    State(state): State<AppState>,
    uri: axum::http::Uri,
    axum::extract::Path(title_id): axum::extract::Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role_with_return(Role::Librarian, uri.path(), locale.0)?;

    let pool = &state.pool;

    // Guard: the title must exist (no soft-delete) — otherwise an
    // admin-typed deep link to a deleted title's slot would silently
    // render a form for a phantom id and the subsequent POST would
    // surface a confusing FK error.
    if crate::models::title::TitleModel::find_by_id(pool, title_id)
        .await?
        .is_none()
    {
        return Err(AppError::NotFound(
            rust_i18n::t!("error.not_found", locale = locale.0).to_string(),
        ));
    }

    let roles = ContributorRoleModel::find_all(pool).await?;

    let template = ContributorFormTemplate {
        form_heading: rust_i18n::t!("contributor.form.add_button").to_string(),
        label_name: rust_i18n::t!("contributor.form.name").to_string(),
        label_role: rust_i18n::t!("contributor.form.role").to_string(),
        label_submit: rust_i18n::t!("contributor.form.submit").to_string(),
        title_id,
        roles,
    };

    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render contributor form for title");
            Err(AppError::TemplateRenderFailed { template: "catalog" })
        }
    }
}

fn contributor_list_html(contributors: &[TitleContributorModel]) -> String {
    if contributors.is_empty() {
        return String::new();
    }

    // Group by contributor to merge roles
    type ContributorGroup<'a> = (u64, &'a str, Vec<(&'a str, u64)>);
    let mut grouped: Vec<ContributorGroup<'_>> = Vec::new();
    for tc in contributors {
        if let Some(entry) = grouped
            .iter_mut()
            .find(|(cid, _, _)| *cid == tc.contributor_id)
        {
            entry.2.push((&tc.role_name, tc.id));
        } else {
            grouped.push((
                tc.contributor_id,
                &tc.contributor_name,
                vec![(&tc.role_name, tc.id)],
            ));
        }
    }

    let mut html = String::from(
        r#"<ul role="list" aria-label="Contributors" class="flex flex-wrap gap-1 text-sm text-stone-700 dark:text-stone-300">"#,
    );

    for (i, (cid, name, roles)) in grouped.iter().enumerate() {
        if i > 0 {
            // #424 — the only child is `aria-hidden`, so this carries no text
            // for a contrast rule to evaluate; it is a decorative separator,
            // not content. Left at stone-400 on purpose.
            html.push_str(r#"<li class="text-stone-400"><span aria-hidden="true"> · </span></li>"#);
        }

        let role_names: Vec<&str> = roles.iter().map(|(r, _)| *r).collect();
        let roles_str = role_names.join(", ");
        let escaped_name = html_escape(name);
        let escaped_roles = html_escape(&roles_str);

        html.push_str(&format!(
            r##"<li><a href="/contributor/{}" class="text-indigo-600 dark:text-indigo-400 hover:underline" aria-label="{}, {}">{}</a> <span class="text-stone-500" aria-hidden="true">({})</span>"##,
            cid, escaped_name, escaped_roles, escaped_name, escaped_roles
        ));

        // Add remove buttons for each role assignment
        for (role, junction_id) in roles {
            let title_id = contributors.first().map(|c| c.title_id).unwrap_or(0);
            html.push_str(&format!(
                r##" <button type="button" class="text-red-400 hover:text-red-600 text-xs" aria-label="{}" hx-post="/catalog/contributors/remove" hx-vals='{{"junction_id":{},"title_id":{}}}' hx-target="#feedback-list" hx-swap="afterbegin">&times;</button>"##,
                html_escape(&format!("Remove {} as {}", name, role)),
                junction_id,
                title_id
            ));
        }

        html.push_str("</li>");
    }

    html.push_str("</ul>");
    html
}
