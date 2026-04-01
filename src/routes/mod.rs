pub mod auth;
pub mod catalog;
pub mod contributors;
pub mod home;
pub mod locations;
pub mod titles;

use axum::Router;
use tower_http::services::ServeDir;

use crate::middleware::pending_updates::pending_updates_middleware;
use crate::AppState;

pub fn build_router(state: AppState) -> Router {
    let pool = state.pool.clone();

    // Catalog routes with PendingUpdates middleware for async metadata delivery
    let catalog_routes = Router::new()
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
        .route(
            "/catalog/title/{id}",
            axum::routing::delete(catalog::delete_title),
        )
        .route(
            "/catalog/volume/{id}",
            axum::routing::delete(catalog::delete_volume),
        )
        .layer(axum::Extension(pool))
        .layer(axum::middleware::from_fn(pending_updates_middleware));

    // All routes
    Router::new()
        .route("/", axum::routing::get(home::home))
        .route("/login", axum::routing::get(auth::login_page).post(auth::login))
        .route("/logout", axum::routing::get(auth::logout).post(auth::logout))
        .route(
            "/session/keepalive",
            axum::routing::post(catalog::session_keepalive),
        )
        .merge(catalog_routes)
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
            "/volume/{id}",
            axum::routing::get(catalog::volume_detail),
        )
        .route(
            "/volume/{id}/edit",
            axum::routing::get(catalog::volume_edit_page),
        )
        .route(
            "/volume/{id}/update",
            axum::routing::post(catalog::update_volume),
        )
        .route(
            "/location/{id}",
            axum::routing::get(locations::location_detail),
        )
        // Location management
        .route(
            "/locations",
            axum::routing::get(locations::locations_page).post(locations::create_location),
        )
        .route(
            "/locations/next-lcode",
            axum::routing::get(locations::next_lcode),
        )
        .route(
            "/locations/{id}/edit",
            axum::routing::get(locations::edit_location_page),
        )
        .route(
            "/locations/{id}",
            axum::routing::post(locations::update_location).delete(locations::delete_location),
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
