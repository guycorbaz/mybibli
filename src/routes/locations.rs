use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse};

use crate::error::AppError;
use crate::middleware::auth::Session;
use crate::middleware::htmx::HxRequest;
use crate::models::location::LocationModel;
use crate::AppState;

#[derive(Template)]
#[template(path = "pages/location_detail.html")]
pub struct LocationDetailTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub location: LocationModel,
    pub path: String,
    pub coming_soon: String,
}

pub async fn location_detail(
    State(state): State<AppState>,
    session: Session,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let pool = &state.pool;

    let location = LocationModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found").to_string()))?;

    let path = LocationModel::get_path(pool, location.id).await?;

    if is_htmx {
        let html = format!(
            "<div class=\"max-w-4xl mx-auto px-4 py-8\">\
                <nav class=\"text-sm text-stone-500 dark:text-stone-400 mb-4\" aria-label=\"Breadcrumb\">{}</nav>\
                <h1 class=\"text-2xl font-bold text-stone-900 dark:text-stone-100\">{}</h1>\
                <p class=\"mt-1 text-sm text-stone-400\">{} · {}</p>\
                <div class=\"mt-8 text-center text-stone-500 dark:text-stone-400\"><p>{}</p></div>\
            </div>",
            crate::utils::html_escape(&path),
            crate::utils::html_escape(&location.name),
            crate::utils::html_escape(&location.label),
            crate::utils::html_escape(&location.node_type),
            rust_i18n::t!("feedback.location_stub"),
        );
        return Ok(axum::response::Html(html).into_response());
    }

    let template = LocationDetailTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "location",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        location,
        path,
        coming_soon: rust_i18n::t!("feedback.location_stub").to_string(),
    };
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(_) => Err(AppError::Internal("Template rendering failed".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;

    #[test]
    fn test_location_detail_template_renders() {
        let template = LocationDetailTemplate {
            lang: "en".to_string(),
            role: "anonymous".to_string(),
            current_page: "location",
            skip_label: "Skip".to_string(),
            nav_catalog: "Catalog".to_string(),
            nav_loans: "Loans".to_string(),
            nav_admin: "Admin".to_string(),
            nav_login: "Log in".to_string(),
            nav_logout: "Log out".to_string(),
            location: LocationModel {
                id: 1,
                parent_id: None,
                name: "Salon".to_string(),
                node_type: "room".to_string(),
                label: "L0001".to_string(),
            },
            path: "Maison → Salon".to_string(),
            coming_soon: "Coming soon".to_string(),
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Salon"));
        assert!(rendered.contains("Maison"));
    }
}
