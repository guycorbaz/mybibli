use askama::Template;
use axum::http::header;
use axum::response::{Html, IntoResponse};

use crate::middleware::auth::Session;

#[derive(Template)]
#[template(path = "pages/home.html")]
pub struct HomeTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub subtitle: String,
}

pub async fn home(session: Session) -> impl IntoResponse {
    let template = HomeTemplate {
        lang: rust_i18n::locale().to_string(),
        role: session.role.to_string(),
        current_page: "home",
        skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
        nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
        nav_loans: rust_i18n::t!("nav.loans").to_string(),
        nav_admin: rust_i18n::t!("nav.admin").to_string(),
        nav_login: rust_i18n::t!("nav.login").to_string(),
        nav_logout: rust_i18n::t!("nav.logout").to_string(),
        subtitle: rust_i18n::t!("home.subtitle").to_string(),
    };
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain")],
            "Template rendering failed",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;

    #[test]
    fn test_home_template_renders() {
        let template = HomeTemplate {
            lang: "en".to_string(),
            role: "anonymous".to_string(),
            current_page: "home",
            skip_label: "Skip to main content".to_string(),
            nav_catalog: "Catalog".to_string(),
            nav_loans: "Loans".to_string(),
            nav_admin: "Admin".to_string(),
            nav_login: "Log in".to_string(),
            nav_logout: "Log out".to_string(),
            subtitle: "Your personal media library".to_string(),
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("mybibli"));
    }

    #[test]
    fn test_home_template_hides_catalog_for_anonymous() {
        let template = HomeTemplate {
            lang: "en".to_string(),
            role: "anonymous".to_string(),
            current_page: "home",
            skip_label: "Skip".to_string(),
            nav_catalog: "Catalog".to_string(),
            nav_loans: "Loans".to_string(),
            nav_admin: "Admin".to_string(),
            nav_login: "Log in".to_string(),
            nav_logout: "Log out".to_string(),
            subtitle: "Test".to_string(),
        };
        let rendered = template.render().unwrap();
        assert!(!rendered.contains(r#"href="/catalog""#));
        assert!(rendered.contains(r#"href="/login""#));
    }
}
