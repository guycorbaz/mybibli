use askama::Template;
use axum::response::{Html, IntoResponse};
use serde::Deserialize;

use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;

#[derive(Template)]
#[template(path = "pages/catalog.html")]
pub struct CatalogTemplate {
    pub lang: String,
    pub role: String,
    pub current_page: &'static str,
    pub skip_label: String,
    pub nav_catalog: String,
    pub nav_loans: String,
    pub nav_admin: String,
    pub nav_login: String,
    pub nav_logout: String,
    pub catalog_title: String,
    pub scan_label: String,
    pub scan_placeholder: String,
}

impl CatalogTemplate {
    fn from_session(session: &Session) -> Self {
        CatalogTemplate {
            lang: rust_i18n::locale().to_string(),
            role: session.role.to_string(),
            current_page: "catalog",
            skip_label: rust_i18n::t!("nav.skip_to_content").to_string(),
            nav_catalog: rust_i18n::t!("nav.catalog").to_string(),
            nav_loans: rust_i18n::t!("nav.loans").to_string(),
            nav_admin: rust_i18n::t!("nav.admin").to_string(),
            nav_login: rust_i18n::t!("nav.login").to_string(),
            nav_logout: rust_i18n::t!("nav.logout").to_string(),
            catalog_title: rust_i18n::t!("catalog.title").to_string(),
            scan_label: rust_i18n::t!("catalog.scan_label").to_string(),
            scan_placeholder: rust_i18n::t!("catalog.scan_placeholder").to_string(),
        }
    }
}

pub async fn catalog_page(session: Session) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    let template = CatalogTemplate::from_session(&session);
    match template.render() {
        Ok(html) => Ok(Html(html).into_response()),
        Err(e) => {
            tracing::error!(error = %e, "Failed to render catalog template");
            Err(AppError::Internal("Template rendering failed".to_string()))
        }
    }
}

#[derive(Deserialize)]
pub struct ScanForm {
    pub code: String,
}

pub async fn handle_scan(
    session: Session,
    HxRequest(is_htmx): HxRequest,
    axum::Form(form): axum::Form<ScanForm>,
) -> Result<impl IntoResponse, AppError> {
    session.require_role(Role::Librarian)?;

    if is_htmx {
        let escaped_code = form.code
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;");
        let fragment = format!(
            r#"<div class="p-3 border-l-4 border-green-500 bg-green-50 dark:bg-green-900/20 rounded-r text-stone-700 dark:text-stone-300" role="status">{}: {}</div>"#,
            rust_i18n::t!("catalog.scan_received"),
            escaped_code
        );
        Ok(Html(fragment).into_response())
    } else {
        let template = CatalogTemplate::from_session(&session);
        match template.render() {
            Ok(html) => Ok(Html(html).into_response()),
            Err(e) => {
                tracing::error!(error = %e, "Failed to render catalog template");
                Err(AppError::Internal("Template rendering failed".to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use askama::Template;
    use crate::middleware::auth::Role;

    #[test]
    fn test_catalog_template_renders_for_librarian() {
        let session = Session {
            token: Some("test".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
        };
        let template = CatalogTemplate::from_session(&session);
        let rendered = template.render().unwrap();
        assert!(rendered.contains("scan-field"));
        assert!(rendered.contains("feedback-list"));
        assert!(rendered.contains(r#"data-user-role="librarian""#));
    }

    #[test]
    fn test_catalog_template_shows_catalog_nav_for_librarian() {
        let session = Session {
            token: Some("test".to_string()),
            user_id: Some(1),
            role: Role::Librarian,
        };
        let template = CatalogTemplate::from_session(&session);
        let rendered = template.render().unwrap();
        assert!(rendered.contains(r#"aria-current="page""#));
        assert!(rendered.contains("/catalog"));
    }
}
