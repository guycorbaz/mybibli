pub mod admin;
pub mod admin_api_keys;
pub mod audit;
pub mod stats;
pub mod title_lifecycle;
pub mod admin_reference_data;
pub mod admin_system;
pub mod api_v1;
pub mod auth;
pub mod borrowers;
pub mod catalog;
pub mod contributors;
pub mod home;
pub mod home_indicators;
pub mod home_scan;
mod home_search_fragment;
#[cfg(test)]
mod home_indicator_tests;
#[cfg(test)]
mod modal_tests;
#[cfg(test)]
mod status_message_tests;
#[cfg(test)]
mod volume_detail_tests;
pub mod loans;
pub mod locations;
pub mod series;
pub mod setup;
pub mod titles;
pub mod wishlist;

use axum::Router;
use tower_http::services::ServeDir;

/// CR #136 — build a 200 OK response carrying an inline "already
/// deleted by another session" feedback fragment + HX-Retarget so the
/// HTMX swap lands in the caller's chosen feedback slot (rather than
/// the `AppError::NotFound` default `#feedback-list`, which the
/// borrower / contributor / loan detail pages do not declare).
///
/// `retarget` must be a valid CSS selector for HTMX — typically
/// `"#borrower-feedback"`, `"#contributor-feedback"`, or the
/// `?target=` resolution on the return-loan modal handler.
pub(crate) fn build_already_deleted_response(
    locale: &'static str,
    retarget: &str,
) -> axum::response::Response {
    use axum::http::{HeaderValue, StatusCode};
    use axum::response::{Html, IntoResponse};

    let title =
        rust_i18n::t!("error.already_deleted_by_another_session", locale = locale).to_string();
    let html = crate::utils::feedback_html("error", &title, "");
    let mut response: axum::response::Response = (StatusCode::OK, Html(html)).into_response();
    let headers = response.headers_mut();
    if let Ok(v) = HeaderValue::from_str(retarget) {
        headers.insert("HX-Retarget", v);
    }
    headers.insert("HX-Reswap", HeaderValue::from_static("innerHTML"));
    response
}

use crate::AppState;
use crate::middleware::auth::session_resolve_middleware;
use crate::middleware::csp::apply_csp_layer;
use crate::middleware::csrf::csrf_middleware;
use crate::middleware::locale::locale_resolve_middleware;
use crate::middleware::modal_confirm_retarget_guard::modal_confirm_retarget_guard;
use crate::middleware::non_htmx_error_wrapper::non_htmx_error_wrapper;
use crate::middleware::pending_updates::pending_updates_middleware;
use crate::middleware::setup_gate::setup_gate_middleware;

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
        // Fix #318 — title-detail-page entry point for the "Add
        // contributor" form. Same template as the catalog flow but
        // sources title_id from the URL path rather than the
        // session's current_title_id.
        .route(
            "/title/{id}/contributor-form",
            axum::routing::get(catalog::contributor_form_for_title),
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
        .route("/scan", axum::routing::get(home_scan::handle_home_scan))
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
        .route(
            "/debug/seed-overdue-loan",
            axum::routing::post(loans::debug_seed_overdue_loan),
        )
        .merge(catalog_routes)
        // Detail pages
        .route(
            "/title/{id}",
            axum::routing::get(titles::title_detail)
                .post(titles::update_title)
                .delete(title_lifecycle::delete_title),
        )
        .route(
            "/title/{id}/delete-modal",
            axum::routing::get(title_lifecycle::delete_title_modal),
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
            "/title/{id}/cover/upload-modal",
            axum::routing::get(titles::cover_upload_modal),
        )
        .route(
            "/title/{id}/cover",
            axum::routing::post(titles::upload_cover),
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
        .route(
            "/contributor/{id}/delete-modal",
            axum::routing::get(contributors::delete_modal),
        )
        .route(
            "/volume/{id}",
            axum::routing::get(catalog::volume_detail).delete(catalog::delete_volume),
        )
        .route(
            "/volume/{id}/edit",
            axum::routing::get(catalog::volume_edit_page),
        )
        .route(
            "/volume/{id}/update",
            axum::routing::post(catalog::update_volume),
        )
        // CR #209: per-volume table delete flow on /title/:id uses
        // /volume/{id}/delete-modal for the UX-DR8 confirmation
        // fragment, then DELETE /volume/{id} for the destructive
        // action (HX-Redirect to /title/:id on success).
        .route(
            "/volume/{id}/delete-modal",
            axum::routing::get(catalog::delete_volume_modal),
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
            "/series/{id}/delete-modal",
            axum::routing::get(series::delete_modal),
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
        .route(
            "/borrower/{id}/delete-modal",
            axum::routing::get(borrowers::delete_modal),
        )
        // CR #242 — Wish list routes
        .route(
            "/wishlist",
            axum::routing::get(wishlist::wishlist_page).post(wishlist::wishlist_create),
        )
        .route(
            "/wishlist/new",
            axum::routing::get(wishlist::wishlist_new_page),
        )
        .route(
            "/wishlist/preview-isbn",
            axum::routing::post(wishlist::wishlist_preview_isbn),
        )
        .route(
            "/wishlist/print",
            axum::routing::get(wishlist::wishlist_print),
        )
        // CR #266 — server-rendered PDF export.
        .route(
            "/wishlist/export.pdf",
            axum::routing::get(wishlist::wishlist_export_pdf),
        )
        // CR #243 — collection valuation breakdown page (Librarian+).
        .route(
            "/stats/value",
            axum::routing::get(stats::stats_value_page),
        )
        // CR #237 — shelf-audit workflow (Librarian+).
        .route("/audit", axum::routing::get(audit::audit_list_page))
        .route(
            "/audit/clear-all",
            axum::routing::post(audit::clear_all_audit),
        )
        .route(
            "/volume/{id}/mark-audit",
            axum::routing::post(audit::mark_volume_audit),
        )
        .route(
            "/volume/{id}/clear-audit",
            axum::routing::post(audit::clear_volume_audit),
        )
        .route(
            "/location/{id}/mark-audit",
            axum::routing::post(audit::mark_location_audit),
        )
        .route(
            "/wishlist/{id}",
            axum::routing::get(wishlist::wishlist_detail).delete(wishlist::wishlist_delete),
        )
        .route(
            "/wishlist/{id}/delete-modal",
            axum::routing::get(wishlist::wishlist_delete_modal),
        )
        // CR #241 — JSON HTTP API under /api/v1/* (API-key auth).
        // CSRF middleware short-circuits on `/api/*` (see csrf.rs).
        .route(
            "/api/v1/titles",
            axum::routing::get(api_v1::list_titles),
        )
        .route(
            "/api/v1/titles/{id}",
            axum::routing::get(api_v1::get_title).patch(api_v1::patch_title),
        )
        .route(
            "/api/v1/genres",
            axum::routing::get(api_v1::list_genres),
        )
        .route(
            "/api/v1/locations",
            axum::routing::get(api_v1::list_locations),
        )
        .route(
            "/api/v1/series",
            axum::routing::get(api_v1::list_series_endpoint),
        )
        .route(
            "/api/v1/dewey/{prefix}",
            axum::routing::get(api_v1::list_titles_by_dewey_prefix),
        )
        // Loan routes
        .route(
            "/loans",
            axum::routing::get(loans::loans_page).post(loans::create_loan),
        )
        .route("/loans/scan", axum::routing::get(loans::scan_on_loans))
        .route(
            "/loans/{id}/return-modal",
            axum::routing::get(loans::return_modal_handler),
        )
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
        .route(
            "/admin/health/bulk-cover-refetch",
            axum::routing::post(admin::admin_bulk_cover_refetch),
        )
        .route("/admin/users", axum::routing::get(admin::admin_users_panel).post(admin::admin_users_create))
        .route("/admin/users/new", axum::routing::get(admin::admin_users_create_form))
        .route("/admin/users/{id}/edit", axum::routing::get(admin::admin_users_edit_form))
        .route("/admin/users/{id}", axum::routing::post(admin::admin_users_update))
        .route("/admin/users/{id}/deactivate", axum::routing::post(admin::admin_users_deactivate))
        .route(
            "/admin/users/{id}/deactivate-modal",
            axum::routing::get(admin::admin_users_deactivate_modal),
        )
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
        // Admin → System settings (story 8-5). 4 routes — 1 GET panel + 3
        // POST per-form saves. All Admin-gated, all CSRF-protected via the
        // 8-2 middleware (none added to CSRF_EXEMPT_ROUTES).
        .route("/admin/system", axum::routing::get(admin_system::admin_system_panel))
        .route(
            "/admin/system/loans",
            axum::routing::post(admin_system::save_loans_settings),
        )
        .route(
            "/admin/system/providers",
            axum::routing::post(admin_system::save_provider_keys),
        )
        .route(
            "/admin/system/language",
            axum::routing::post(admin_system::save_language_settings),
        )
        // v1.5.1 fix #283 — Library valuation section save endpoint.
        .route(
            "/admin/system/valuation",
            axum::routing::post(admin_system::save_library_valuation_settings),
        )
        // v1.7.1 fix #308 — Logging section save endpoint. Closes the
        // v1.7.0 #301 gap (runtime log-level admin UI).
        .route(
            "/admin/system/log-level",
            axum::routing::post(admin_system::save_log_level),
        )
        // v1.7.9 fix #334 — Metadata-chain + provider-health timeouts
        // save endpoint. Surfaced inside the existing System / Metadata
        // Providers section. Both timeouts hot-reload (chain reads per
        // fetch, provider_health task reads per ping round).
        .route(
            "/admin/system/timeouts",
            axum::routing::post(admin_system::save_metadata_timeouts),
        )
        // CR #241 — Admin → API keys (v1.4.0). 4 routes: 1 GET panel +
        // 1 POST create + 1 GET revoke-modal + 1 POST revoke. All
        // Admin-gated. CSRF-protected via the 8-2 middleware.
        .route(
            "/admin/api-keys",
            axum::routing::get(admin_api_keys::admin_api_keys_panel)
                .post(admin_api_keys::admin_api_keys_create),
        )
        .route(
            "/admin/api-keys/{id}/revoke-modal",
            axum::routing::get(admin_api_keys::admin_api_keys_revoke_modal),
        )
        .route(
            "/admin/api-keys/{id}/revoke",
            axum::routing::post(admin_api_keys::admin_api_keys_revoke),
        )
        // v1.5.1 fix #284 — hard-delete a (revoked) API key.
        .route(
            "/admin/api-keys/{id}/delete-modal",
            axum::routing::get(admin_api_keys::admin_api_keys_delete_modal),
        )
        .route(
            "/admin/api-keys/{id}",
            axum::routing::delete(admin_api_keys::admin_api_keys_delete),
        )
        // Story 8-8 — first-launch setup wizard. Whitelisted by the
        // setup-gate middleware (so this is reachable on a fresh install
        // even though every other route 303s to /setup); each handler
        // self-gates on the wizard predicate so a stale / hostile POST
        // can't mutate `users` or `settings` once the wizard is done.
        .route("/setup", axum::routing::get(setup::setup_page))
        .route("/setup/step-1", axum::routing::post(setup::step_1_submit))
        .route("/setup/step-2", axum::routing::post(setup::step_2_submit))
        .route("/setup/step-3", axum::routing::post(setup::step_3_submit))
        .route("/setup/complete", axum::routing::post(setup::complete_submit))
        .route("/health", axum::routing::get(health_check))
        .nest_service("/static", ServeDir::new("static"))
        .nest_service("/covers", ServeDir::new(&state.covers_dir))
        // Logo assets (SVG icon + horizontal wordmark in light/dark + PNG
        // multi-size + favicon.ico). Lives in `docs/mybibli-logo/` (the
        // source-of-truth dir, with README explaining colors / file role
        // / HTML snippets). Served at /logo/{svg,png,...} so HTML can
        // reference `/logo/svg/mybibli-icon.svg` etc. The Dockerfile
        // copies this directory into the runtime image.
        .nest_service("/logo", ServeDir::new("docs/mybibli-logo"))
        // Layer stack (axum applies layers bottom-up; at request time
        // the request hits the OUTERMOST layer first):
        //
        //   CSP  →  Session-resolve  →  Locale  →  ModalConfirmRetargetGuard
        //   →  CSRF  →  [handler / PendingUpdates on catalog routes]
        //
        //   * Session-resolve must run before CSRF so the CSRF middleware
        //     sees a populated `Session` extension (including anonymous
        //     visitors that just had a row + csrf_token minted on
        //     first-hit).
        //   * Locale runs after session-resolve so the CSRF rejection
        //     body can be localized via the cached session's
        //     preferred_language (Pattern A in story 7-3 still works).
        //   * polish-1 AC4.b: ModalConfirmRetargetGuard sits OUTSIDE
        //     CSRF so it sees the CSRF middleware's response headers
        //     on token-drift 403. The Guard reads `X-Modal-Confirm`
        //     from the request and strips `HX-Retarget`/`HX-Reswap`
        //     from the response — EXCEPT when the response carries
        //     `HX-Trigger: csrf-rejected` (whitelist, so story 8-2's
        //     CSRF rejection UX stays intact for modal Confirms).
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            csrf_middleware,
        ))
        // polish-1 AC4.b — wraps CSRF + handler, so it sees CSRF's
        // response on token-drift 403. No state dep → plain `from_fn`.
        .layer(axum::middleware::from_fn(modal_confirm_retarget_guard))
        // CR #216 / #26: wrap HTML error fragments in a minimal HTML5 shell
        // when the request was a direct browser navigation (no HX-Request),
        // so 4xx/5xx surfaces aren't a styleless fragment on a blank page.
        // Layered outside the modal-confirm guard so it sees the final body
        // after retarget-strip already ran.
        .layer(axum::middleware::from_fn(non_htmx_error_wrapper))
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
        // Story 8-8: setup-wizard gate. Sits OUTERMOST among the
        // application middlewares (only CSP — applied below — wraps
        // around it). Layer order at request time is therefore:
        //
        //   CSP → SetupGate → SessionResolve → Locale → CSRF → handler
        //
        // SetupGate runs before SessionResolve because the gate doesn't
        // need a populated `Session` extension — it only reads the cached
        // `(admin_count == 0) AND (setup_completed_at IS NONE)` boolean.
        // Whitelisted paths (`/static/*`, `/covers/*`, `/health`,
        // `/setup*`) flow straight through; everything else 303s to
        // `/setup` while the wizard is active.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            setup_gate_middleware,
        ))
        .with_state(state);

    apply_csp_layer(app, report_only)
}

/// Liveness + DB-readiness probe (#21). Returns `200 "ok"` only when a
/// `SELECT 1` against the pool succeeds; `503 "db unavailable"` otherwise.
/// Promoting `/health` from a bare process-alive stub to a DB-readiness
/// check lets the deployment's external monitor distinguish a live process
/// from one that has lost its database — the relevant failure mode on the
/// single-tenant NAS where app and MariaDB are separate containers.
async fn health_check(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    if db_is_healthy(&state.pool).await {
        (StatusCode::OK, "ok").into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "db unavailable").into_response()
    }
}

/// Run a cheap `SELECT 1` to confirm the pool can reach the database.
/// Extracted so the readiness decision is unit-testable against a
/// deliberately-unreachable lazy pool without standing up a full router.
async fn db_is_healthy(pool: &crate::db::DbPool) -> bool {
    sqlx::query("SELECT 1").execute(pool).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #21 (DB hardening): the `/health` probe now reports DB readiness, not
    /// just process liveness. A pool that cannot reach its database must make
    /// `db_is_healthy` return `false` (the handler maps that to `503`). We
    /// exercise the failure path against a lazy pool pointed at an
    /// unreachable address — `connect_lazy` never blocks at construction, and
    /// the first `SELECT 1` fails fast with a connection-refused error, so the
    /// test needs no live database and stays in the default `cargo test` run.
    /// The happy path (`200 "ok"` against a real MariaDB) is covered by the
    /// E2E suite, which runs the binary against a live database.
    #[tokio::test]
    async fn db_is_healthy_returns_false_on_unreachable_db() {
        // Short acquire_timeout so the test fails fast: sqlx retries the
        // connection within the acquire window, so the pool default (30s)
        // would otherwise stall this unit test for the full timeout.
        let pool = sqlx::mysql::MySqlPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(300))
            .connect_lazy("mysql://invalid:invalid@127.0.0.1:1/nonexistent")
            .expect("connect_lazy builds a pool without connecting");
        assert!(!db_is_healthy(&pool).await);
    }
}
