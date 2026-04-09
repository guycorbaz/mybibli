use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse};

use crate::error::AppError;
use crate::middleware::auth::Session;
use crate::middleware::htmx::HxRequest;
use crate::models::contributor::{ContributorModel, ContributorTitleRow};
use crate::utils::html_escape;
use crate::AppState;

#[derive(Template)]
#[template(path = "pages/contributor_detail.html")]
pub struct ContributorDetailTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_locations: String,
    pub nav_series: String,
    pub nav_borrowers: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub contributor: ContributorModel,
    pub titles: Vec<ContributorTitleRow>,
    pub label_titles: String,
    pub delete_label: String,
    pub confirm_delete: String,
}

pub async fn contributor_detail(
    State(state): State<AppState>,
    session: Session,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;

    let (contributor, titles) = ContributorModel::find_by_id_with_titles(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    if is_htmx {
        let html = contributor_detail_fragment(&contributor, &titles);
        Ok(Html(html).into_response())
    } else {
        let template = ContributorDetailTemplate {
            lang: rust_i18n::locale().to_string(),
            role: session.role.to_string(),
            current_page: "contributor",
            skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
            nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
            nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_locations: rust_i18n::t!("nav.locations").to_string(),
            nav_series: rust_i18n::t!("nav.series").to_string(),
            nav_borrowers: rust_i18n::t!("nav.borrowers").to_string(),
            nav_admin: rust_i18n::t!("nav.admin").to_string(),
            nav_login: rust_i18n::t!("nav.login").to_string(),
            nav_logout: rust_i18n::t!("nav.logout").to_string(),
            contributor,
            titles,
            label_titles: rust_i18n::t!("contributor_detail.titles").to_string(),
            delete_label: rust_i18n::t!("contributor_detail.delete").to_string(),
            confirm_delete: rust_i18n::t!("contributor_detail.confirm_delete").to_string(),
        };
        match template.render() {
            Ok(html) => Ok(Html(html).into_response()),
            Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
        }
    }
}

fn contributor_detail_fragment(
    contributor: &ContributorModel,
    titles: &[ContributorTitleRow],
) -> String {
    let escaped_name = html_escape(&contributor.name);
    let bio_html = contributor
        .biography
        .as_ref()
        .map(|b| format!(r#"<p class="mt-2 text-stone-600 dark:text-stone-400">{}</p>"#, html_escape(b)))
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
        rust_i18n::t!("contributor_detail.titles"),
        titles_html
    )
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
        let html = contributor_detail_fragment(&contributor, &titles);
        assert!(html.contains("Albert Camus"));
        assert!(html.contains("/title/42"));
    }
}
