pub mod catalog;
pub mod contributors;
pub mod home;
pub mod locations;
pub mod titles;

use axum::Router;
use tower_http::services::ServeDir;

use crate::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", axum::routing::get(home::home))
        .route("/catalog", axum::routing::get(catalog::catalog_page))
        .route("/catalog/scan", axum::routing::post(catalog::handle_scan))
        .route(
            "/catalog/title/new",
            axum::routing::get(catalog::title_form_page),
        )
        .route("/catalog/title", axum::routing::post(catalog::create_title))
        .route(
            "/catalog/title/fields/{media_type}",
            axum::routing::get(catalog::type_specific_fields),
        )
        .route(
            "/catalog/contributors/form",
            axum::routing::get(catalog::contributor_form_page),
        )
        .route(
            "/catalog/contributors/search",
            axum::routing::get(catalog::contributor_search),
        )
        .route(
            "/catalog/contributors/add",
            axum::routing::post(catalog::add_contributor),
        )
        .route(
            "/catalog/contributors/remove",
            axum::routing::post(catalog::remove_contributor),
        )
        .route(
            "/catalog/contributors/update",
            axum::routing::post(catalog::update_contributor),
        )
        .route(
            "/catalog/contributors/{id}",
            axum::routing::delete(catalog::delete_contributor),
        )
        // Detail pages
        .route(
            "/title/{id}",
            axum::routing::get(titles::title_detail),
        )
        .route(
            "/contributor/{id}",
            axum::routing::get(contributors::contributor_detail),
        )
        .route(
            "/location/{id}",
            axum::routing::get(locations::location_detail),
        )
        .route("/health", axum::routing::get(health_check))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(state)
}

async fn health_check() -> &'static str {
    "ok"
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_app() -> Router {
        Router::new().route("/health", axum::routing::get(health_check))
    }

    #[tokio::test]
    async fn test_health_check_returns_ok() {
        let app = test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"ok");
    }
}
