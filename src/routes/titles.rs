use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse};

use crate::error::AppError;
use crate::middleware::auth::Session;
use crate::middleware::htmx::HxRequest;
use crate::models::contributor::TitleContributorModel;
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;
use crate::models::genre::GenreModel;
use crate::utils::html_escape;
use crate::AppState;

#[derive(Template)]
#[template(path = "pages/title_detail.html")]
pub struct TitleDetailTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub title: TitleModel,
    pub genre_name: String,
    pub volume_count: u64,
    pub contributors: Vec<TitleContributorModel>,
    pub label_contributors: String,
    pub label_vol: String,
}

pub async fn title_detail(
    State(state): State<AppState>,
    session: Session,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    let volume_count = VolumeModel::count_by_title(pool, title.id).await?;
    let contributors = TitleContributorModel::find_by_title(pool, title.id).await?;

    // Look up genre name (single query, not N+1)
    let genre_name = GenreModel::find_name_by_id(pool, title.genre_id).await?;

    if is_htmx {
        // Return a fragment for HTMX navigation
        let html = title_detail_fragment(&title, &genre_name, volume_count, &contributors, &session);
        Ok(Html(html).into_response())
    } else {
        let template = TitleDetailTemplate {
            lang: rust_i18n::locale().to_string(),
            role: session.role.to_string(),
            current_page: "title",
            skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
            nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
            nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_admin: rust_i18n::t!("nav.admin").to_string(),
            nav_login: rust_i18n::t!("nav.login").to_string(),
            nav_logout: rust_i18n::t!("nav.logout").to_string(),
            title,
            genre_name,
            volume_count,
            contributors,
            label_contributors: rust_i18n::t!("title_detail.contributors").to_string(),
            label_vol: rust_i18n::t!("title_detail.volumes").to_string(),
        };
        match template.render() {
            Ok(html) => Ok(Html(html).into_response()),
            Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
        }
    }
}

fn title_detail_fragment(
    title: &TitleModel,
    genre_name: &str,
    volume_count: u64,
    contributors: &[TitleContributorModel],
    _session: &Session,
) -> String {
    let escaped_title = html_escape(&title.title);
    let escaped_genre = html_escape(genre_name);

    let cover_html = match &title.cover_image_url {
        Some(url) => format!(
            r#"<img src="{}" alt="" class="w-48 h-72 object-cover rounded-lg">"#,
            html_escape(url)
        ),
        None => format!(
            r#"<div class="w-48 h-72 bg-stone-100 dark:bg-stone-800 rounded-lg flex items-center justify-center">
                <img src="/static/icons/{}.svg" alt="" class="w-12 h-12 opacity-50">
            </div>"#,
            html_escape(&title.media_type)
        ),
    };

    let subtitle_html = title
        .subtitle
        .as_ref()
        .map(|s| format!(r#"<p class="text-lg text-stone-500 dark:text-stone-400">{}</p>"#, html_escape(s)))
        .unwrap_or_default();

    let contributor_html = if contributors.is_empty() {
        String::new()
    } else {
        let items: Vec<String> = contributors
            .iter()
            .map(|tc| {
                format!(
                    r#"<a href="/contributor/{}" class="text-indigo-600 dark:text-indigo-400 hover:underline">{}</a> <span class="text-stone-500">({})</span>"#,
                    tc.contributor_id,
                    html_escape(&tc.contributor_name),
                    html_escape(&tc.role_name)
                )
            })
            .collect();
        format!(
            r#"<div class="mt-4"><h2 class="text-lg font-semibold text-stone-800 dark:text-stone-200">{}</h2><ul class="mt-2 space-y-1">{}</ul></div>"#,
            rust_i18n::t!("title_detail.contributors"),
            items.iter().map(|i| format!("<li>{}</li>", i)).collect::<String>()
        )
    };

    format!(
        r#"<div class="max-w-4xl mx-auto px-4 py-8">
            <div class="flex gap-8">
                <div class="flex-shrink-0">{}</div>
                <div>
                    <h1 class="text-2xl font-bold text-stone-900 dark:text-stone-100">{}</h1>
                    {}
                    <div class="mt-4 flex gap-4 text-sm text-stone-600 dark:text-stone-400">
                        <span>{}</span>
                        <span>·</span>
                        <span>{} {}</span>
                    </div>
                    {}
                </div>
            </div>
        </div>"#,
        cover_html, escaped_title, subtitle_html, escaped_genre, volume_count, rust_i18n::t!("title_detail.volumes"), contributor_html
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_detail_template_renders() {
        let title = TitleModel {
            id: 1,
            title: "L'Étranger".to_string(),
            subtitle: Some("Roman".to_string()),
            description: None,
            language: "fr".to_string(),
            media_type: "book".to_string(),
            publication_date: None,
            publisher: Some("Gallimard".to_string()),
            isbn: Some("9782070360246".to_string()),
            issn: None,
            upc: None,
            cover_image_url: None,
            genre_id: 1,
            dewey_code: None,
            page_count: Some(186),
            track_count: None,
            total_duration: None,
            age_rating: None,
            issue_number: None,
            version: 1,
        };
        let template = TitleDetailTemplate {
            lang: "en".to_string(),
            role: "anonymous".to_string(),
            current_page: "title",
            skip_label: "Skip".to_string(),
            nav_catalog: "Catalog".to_string(),
            nav_loans: "Loans".to_string(),
            nav_admin: "Admin".to_string(),
            nav_login: "Log in".to_string(),
            nav_logout: "Log out".to_string(),
            title,
            genre_name: "Roman".to_string(),
            volume_count: 2,
            contributors: vec![],
            label_contributors: "Contributors".to_string(),
            label_vol: "Volumes".to_string(),
        };
        let rendered = template.render().unwrap();
        // Askama auto-escapes with HTML entities
        assert!(rendered.contains("tranger"), "Expected title to appear in rendered output");
    }
}
