pub mod admin;
pub mod admin_reference_data;
pub mod auth;
pub mod borrowers;
pub mod catalog;
pub mod contributors;
pub mod home;
pub mod loans;
pub mod locations;
pub mod series;
pub mod titles;

use axum::Router;
use tower_http::services::ServeDir;

use crate::AppState;
use crate::middleware::auth::session_resolve_middleware;
use crate::middleware::csp::apply_csp_layer;
use crate::middleware::csrf::csrf_middleware;
use crate::middleware::locale::locale_resolve_middleware;
use crate::middleware::pending_updates::pending_updates_middleware;

pub fn build_router(state: AppState) -> Router {
    let pool = state.pool.clone();

    // Catalog routes with PendingUpdates middleware for async metadata delivery
    let catalog_routes = Router::new()
        .route("/catalog", axum::routing::get(catalog::catalog_page))
        .route("/catalog/scan", axum::routing::post(catalog::handle_scan))
        .route(
            "/catalog/scan-with-type",
            axum::routing::post(catalog::handle_scan_with_type),
        )
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

    // CSP + hardening headers — wrapped outermost so EVERY response (incl.
    // /static/*, /covers/*, /health, redirects, 4xx/5xx) carries the
    // headers. Per AR16: Logging → Auth → [Handler] → PendingUpdates → CSP.
    // Read mode once at startup (AR26 — no dotenvy).
    let report_only = crate::config::csp_report_only();
    let app = Router::new()
        .route("/", axum::routing::get(home::home))
        .route(
            "/login",
            axum::routing::get(auth::login_page).post(auth::login),
        )
        // Logout is POST-only (story 8-2). GET /logout returns 405 so a
        // cross-origin `<img src="/logout">` or mistyped anchor cannot
        // end a session without a CSRF-bound submission.
        .route("/logout", axum::routing::post(auth::logout))
        .route("/language", axum::routing::post(auth::change_language))
        .route(
            "/session/keepalive",
            axum::routing::post(catalog::session_keepalive),
        )
        .route(
            "/debug/session-timeout",
            axum::routing::post(catalog::debug_set_session_timeout),
        )
        .merge(catalog_routes)
        // Detail pages
        .route(
            "/title/{id}",
            axum::routing::get(titles::title_detail).post(titles::update_title),
        )
        .route(
            "/title/{id}/edit",
            axum::routing::get(titles::title_edit_form),
        )
        .route(
            "/title/{id}/metadata",
            axum::routing::get(titles::title_metadata_fragment),
        )
        .route(
            "/title/{id}/redownload",
            axum::routing::post(titles::redownload_metadata),
        )
        .route(
            "/title/{id}/confirm-metadata",
            axum::routing::post(titles::confirm_metadata),
        )
        .route(
            "/title/{id}/series",
            axum::routing::post(titles::assign_to_series),
        )
        .route(
            "/title/{id}/series/{assignment_id}/remove",
            axum::routing::post(titles::unassign_from_series),
        )
        .route(
            "/title/{id}/series-remove",
            axum::routing::post(titles::unassign_omnibus_from_series),
        )
        .route(
            "/contributor/{id}",
            axum::routing::get(contributors::contributor_detail),
        )
        .route("/volume/{id}", axum::routing::get(catalog::volume_detail))
        .route(
            "/volume/{id}/edit",
            axum::routing::get(catalog::volume_edit_page),
        )
        .route(
            "/volume/{id}/update",
            axum::routing::post(catalog::update_volume),
        )
        // Series routes
        .route(
            "/series",
            axum::routing::get(series::series_list_page).post(series::create_series),
        )
        .route(
            "/series/new",
            axum::routing::get(series::create_series_form),
        )
        .route(
            "/series/{id}",
            axum::routing::get(series::series_detail_page)
                .post(series::update_series)
                .delete(series::delete_series),
        )
        .route(
            "/series/{id}/edit",
            axum::routing::get(series::edit_series_form),
        )
        // Borrower routes
        .route(
            "/borrowers",
            axum::routing::get(borrowers::borrowers_page).post(borrowers::create_borrower),
        )
        .route(
            "/borrowers/search",
            axum::routing::get(borrowers::borrower_search),
        )
        .route(
            "/borrower/{id}",
            axum::routing::get(borrowers::borrower_detail)
                .post(borrowers::update_borrower)
                .delete(borrowers::delete_borrower),
        )
        .route(
            "/borrower/{id}/edit",
            axum::routing::get(borrowers::edit_borrower_page),
        )
        // Loan routes
        .route(
            "/loans",
            axum::routing::get(loans::loans_page).post(loans::create_loan),
        )
        .route("/loans/scan", axum::routing::get(loans::scan_on_loans))
        .route(
            "/loans/{id}/return",
            axum::routing::post(loans::return_loan_handler),
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
        // Admin (story 8-1) — 5-tab shell. Every handler's first line is
        // `require_role(Role::Admin)?`; librarians → 403, anonymous → 303
        // → /login?next=%2Fadmin. Routes live at the top level (not under
        // the catalog sub-router) so they skip `pending_updates_middleware`.
        .route("/admin", axum::routing::get(admin::admin_page))
        .route("/admin/health", axum::routing::get(admin::admin_health_panel))
        .route("/admin/users", axum::routing::get(admin::admin_users_panel).post(admin::admin_users_create))
        .route("/admin/users/new", axum::routing::get(admin::admin_users_create_form))
        .route("/admin/users/{id}/edit", axum::routing::get(admin::admin_users_edit_form))
        .route("/admin/users/{id}", axum::routing::post(admin::admin_users_update))
        .route("/admin/users/{id}/deactivate", axum::routing::post(admin::admin_users_deactivate))
        .route("/admin/users/{id}/reactivate", axum::routing::post(admin::admin_users_reactivate))
        // Admin → Reference data CRUD (story 8-4). 20 routes across 4
        // sub-sections (genres, volume_states, contributor_roles, node_types):
        // 1 panel + 4 sections × (list / create / rename / delete-modal /
        // delete) + volume_states extras (loanable / loanable-confirm / row).
        // All Admin-gated, all CSRF-protected via the 8-2 middleware.
        .route(
            "/admin/reference-data",
            axum::routing::get(admin_reference_data::admin_reference_data_panel),
        )
        // Genres
        .route(
            "/admin/reference-data/genres",
            axum::routing::get(admin_reference_data::genres_section)
                .post(admin_reference_data::genres_create),
        )
        .route(
            "/admin/reference-data/genres/{id}/rename",
            axum::routing::post(admin_reference_data::genres_rename),
        )
        .route(
            "/admin/reference-data/genres/{id}/delete-modal",
            axum::routing::get(admin_reference_data::genres_delete_modal),
        )
        .route(
            "/admin/reference-data/genres/{id}/delete",
            axum::routing::post(admin_reference_data::genres_delete),
        )
        // Volume States
        .route(
            "/admin/reference-data/volume-states",
            axum::routing::get(admin_reference_data::volume_states_section)
                .post(admin_reference_data::volume_states_create),
        )
        .route(
            "/admin/reference-data/volume-states/{id}/rename",
            axum::routing::post(admin_reference_data::volume_states_rename),
        )
        .route(
            "/admin/reference-data/volume-states/{id}/delete-modal",
            axum::routing::get(admin_reference_data::volume_states_delete_modal),
        )
        .route(
            "/admin/reference-data/volume-states/{id}/delete",
            axum::routing::post(admin_reference_data::volume_states_delete),
        )
        .route(
            "/admin/reference-data/volume-states/{id}/loanable",
            axum::routing::post(admin_reference_data::volume_states_loanable_toggle),
        )
        .route(
            "/admin/reference-data/volume-states/{id}/loanable/confirm",
            axum::routing::post(admin_reference_data::volume_states_loanable_confirm),
        )
        .route(
            "/admin/reference-data/volume-states/{id}/row",
            axum::routing::get(admin_reference_data::volume_states_row_view),
        )
        // Contributor Roles
        .route(
            "/admin/reference-data/contributor-roles",
            axum::routing::get(admin_reference_data::roles_section)
                .post(admin_reference_data::roles_create),
        )
        .route(
            "/admin/reference-data/contributor-roles/{id}/rename",
            axum::routing::post(admin_reference_data::roles_rename),
        )
        .route(
            "/admin/reference-data/contributor-roles/{id}/delete-modal",
            axum::routing::get(admin_reference_data::roles_delete_modal),
        )
        .route(
            "/admin/reference-data/contributor-roles/{id}/delete",
            axum::routing::post(admin_reference_data::roles_delete),
        )
        // Location Node Types
        .route(
            "/admin/reference-data/node-types",
            axum::routing::get(admin_reference_data::node_types_section)
                .post(admin_reference_data::node_types_create),
        )
        .route(
            "/admin/reference-data/node-types/{id}/rename",
            axum::routing::post(admin_reference_data::node_types_rename),
        )
        .route(
            "/admin/reference-data/node-types/{id}/delete-modal",
            axum::routing::get(admin_reference_data::node_types_delete_modal),
        )
        .route(
            "/admin/reference-data/node-types/{id}/delete",
            axum::routing::post(admin_reference_data::node_types_delete),
        )
        .route("/admin/trash", axum::routing::get(admin::admin_trash_panel))
        .route("/admin/trash/{table}/{id}/permanent-delete", axum::routing::get(admin::admin_trash_permanent_delete_confirm).post(admin::admin_trash_permanent_delete))
        .route("/admin/system", axum::routing::get(admin::admin_system_panel))
        .route("/health", axum::routing::get(health_check))
        .nest_service("/static", ServeDir::new("static"))
        .nest_service("/covers", ServeDir::new(&state.covers_dir))
        // Layer stack (axum applies layers bottom-up; at request time
        // the request hits the OUTERMOST layer first):
        //
        //   CSP  →  Session-resolve  →  Locale  →  CSRF  →  [handler /
        //   PendingUpdates on catalog routes]
        //
        //   * Session-resolve must run before CSRF so the CSRF middleware
        //     sees a populated `Session` extension (including anonymous
        //     visitors that just had a row + csrf_token minted on
        //     first-hit).
        //   * Locale runs after session-resolve so the CSRF rejection
        //     body can be localized via the cached session's
        //     preferred_language (Pattern A in story 7-3 still works).
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            csrf_middleware,
        ))
        // Locale middleware runs on every request (before the state-consuming
        // `.with_state(state)` call) so handlers can read `Extension<Locale>`
        // without per-route wiring. Registered here after route mounting —
        // axum applies layers bottom-up, so this wraps the whole router.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            locale_resolve_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            session_resolve_middleware,
        ))
        .with_state(state);

    apply_csp_layer(app, report_only)
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
