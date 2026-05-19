//! Title lifecycle handlers — delete + (future: archive, merge, …).
//!
//! Split from `routes/titles.rs` per Foundation Rule #12 — that file
//! is already over the 2000-line soft cap. New title-level actions
//! land here.
//!
//! ## CR #271 — Delete a title with no volumes
//!
//! Production-driven (Guy, 2026-05-19 on v1.4.0): a wrong ISBN scan
//! created a title with zero volumes and there was no UI affordance
//! to remove it. The Delete button on the title detail page appears
//! ONLY when `VolumeModel::count_by_title == 0`; the handler enforces
//! the same invariant as defense-in-depth.

use askama::Template;
use axum::Extension;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use crate::AppState;
use crate::error::AppError;
use crate::middleware::auth::{Role, Session};
use crate::middleware::htmx::HxRequest;
use crate::middleware::locale::Locale;
use crate::models::title::TitleModel;
use crate::models::volume::VolumeModel;

#[derive(Template)]
#[template(path = "fragments/title_delete_modal.html")]
pub struct TitleDeleteModalTemplate {
    pub modal_title: String,
    pub body_html: String,
    pub confirm_label: String,
    pub cancel_label: String,
    pub action_url: String,
    pub csrf_token: String,
}

/// `GET /title/{id}/delete-modal` — UX-DR8 destructive-action modal
/// for the title detail page's Delete button. Librarian-gated,
/// HTMX-only (direct nav returns 405). Refuses to render if the title
/// still has any active volume — the guard runs server-side so a
/// stale page that still shows the button can't bypass it.
pub async fn delete_title_modal(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    HxRequest(is_htmx): HxRequest,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    session.require_role_with_return(Role::Librarian, &format!("/title/{id}"), locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    if !is_htmx {
        return Ok(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    // Defense-in-depth: the button is hidden on volumes > 0, but
    // a stale tab + a race could let the modal request through.
    // Refuse with a friendly conflict feedback the UI swaps in.
    let volume_count = VolumeModel::count_by_title(pool, id).await?;
    if volume_count > 0 {
        return Err(AppError::Conflict(
            rust_i18n::t!("title.delete_blocked_has_volumes", locale = loc).to_string(),
        ));
    }

    let modal_title = rust_i18n::t!(
        "title.delete_modal_title",
        locale = loc,
        title = title.title.as_str()
    )
    .to_string();
    let body_text = rust_i18n::t!("title.delete_modal_body", locale = loc).to_string();
    let body_html = format!("<p>{}</p>", crate::utils::html_escape(&body_text));

    let template = TitleDeleteModalTemplate {
        modal_title,
        body_html,
        confirm_label: rust_i18n::t!("title.delete_modal_confirm", locale = loc).to_string(),
        cancel_label: rust_i18n::t!("common.cancel", locale = loc).to_string(),
        action_url: format!("/title/{}", title.id),
        csrf_token: session.csrf_token.clone(),
    };
    template
        .render()
        .map(|html| Html(html).into_response())
        .map_err(|e| AppError::Internal(format!("title delete modal render: {e}")))
}

/// `DELETE /title/{id}` — soft-delete a title that has no active
/// volumes. Librarian-gated, optimistic conflict on the "has volumes"
/// guard. On success: HX-Redirect to `/` (home) so the user lands on
/// a useful page; the catalog list will no longer surface the row.
pub async fn delete_title(
    State(state): State<AppState>,
    session: Session,
    Extension(locale): Extension<Locale>,
    Path(id): Path<u64>,
) -> Result<Response, AppError> {
    session.require_role(Role::Librarian, locale.0)?;
    let pool = &state.pool;
    let loc = locale.0;

    let title = TitleModel::find_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound(rust_i18n::t!("error.not_found", locale = loc).to_string()))?;

    // Same guard as the modal — the modal request and the destructive
    // action are two HTTP round-trips, a volume could land between them.
    let volume_count = VolumeModel::count_by_title(pool, id).await?;
    if volume_count > 0 {
        return Err(AppError::Conflict(
            rust_i18n::t!("title.delete_blocked_has_volumes", locale = loc).to_string(),
        ));
    }

    crate::services::soft_delete::SoftDeleteService::soft_delete(pool, "titles", id).await?;

    // Audit row — attribution travels via session.user_id. Details carry
    // the title + identifiers so an admin reviewing the trail later sees
    // exactly which row went down even after the soft-delete row is
    // permanently purged.
    let details = serde_json::json!({
        "title": title.title,
        "isbn": title.isbn,
        "issn": title.issn,
        "upc": title.upc,
    });
    if let Err(e) = crate::models::admin_audit::AdminAuditModel::create(
        pool,
        session.user_id.unwrap_or(0),
        "title_delete",
        Some("titles"),
        Some(id),
        Some(details),
    )
    .await
    {
        tracing::warn!(
            title_id = id,
            error = %e,
            "title_delete audit insert failed — soft-delete has already committed"
        );
    }

    tracing::info!(title_id = id, "Title soft-deleted (CR #271)");

    Ok((
        StatusCode::OK,
        [(
            axum::http::header::HeaderName::from_static("hx-redirect"),
            "/".to_string(),
        )],
        String::new(),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    //! DB-backed tests for CR #271 — the delete-title route. Each test
    //! seeds a Librarian session cookie + CSRF token, drives a real
    //! request through `tower::oneshot` so middleware (CSRF, session,
    //! locale) all run as in production.

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use sqlx::MySqlPool;
    use tower::ServiceExt;

    async fn seed_librarian(pool: &MySqlPool) -> String {
        let username = "title_lifecycle_librarian";
        // Argon2 hash of "librarian" — same fixture used by other
        // route-integration suites.
        let password_hash = "$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU";
        sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
            .bind(username)
            .bind(password_hash)
            .execute(pool)
            .await
            .unwrap();
        username.to_string()
    }

    async fn seed_title(pool: &MySqlPool, name: &str, isbn: Option<&str>) -> u64 {
        // CLAUDE.md MariaDB gotcha #2: `genres.id` is `BIGINT UNSIGNED`
        // and SQLx can't decode it directly as `i64`. CAST to SIGNED so
        // the type unifier picks the signed shape `query_as` expects.
        let (genre_id,): (i64,) = sqlx::query_as(
            "SELECT CAST(MIN(id) AS SIGNED) FROM genres WHERE deleted_at IS NULL",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let r = sqlx::query(
            "INSERT INTO titles (title, language, genre_id, media_type, isbn) VALUES (?, 'en', ?, 'book', ?)",
        )
        .bind(name)
        .bind(genre_id)
        .bind(isbn)
        .execute(pool)
        .await
        .unwrap();
        r.last_insert_id()
    }

    async fn seed_volume(pool: &MySqlPool, title_id: u64) -> u64 {
        let r = sqlx::query("INSERT INTO volumes (title_id, label) VALUES (?, ?)")
            .bind(title_id)
            .bind(format!("V{title_id:04}"))
            .execute(pool)
            .await
            .unwrap();
        r.last_insert_id()
    }

    fn state_with_pool(pool: MySqlPool) -> crate::AppState {
        use std::sync::{Arc, RwLock};
        crate::AppState {
            pool,
            settings: Arc::new(RwLock::new(crate::config::AppSettings::default())),
            http_client: reqwest::Client::new(),
            registry: Arc::new(crate::metadata::registry::ProviderRegistry::new()),
            covers_dir: std::path::PathBuf::from("/tmp"),
            provider_health: crate::tasks::provider_health::new_provider_health_map(),
            mariadb_version_cache: crate::services::admin_health::new_mariadb_version_cache(),
            setup_gate: Arc::new(RwLock::new(
                crate::middleware::setup_gate::SetupGateState::default(),
            )),
            bulk_cover_fetch: Arc::new(RwLock::new(
                crate::services::bulk_cover_fetch::BulkCoverFetchStatus::default(),
            )),
        }
    }

    async fn login(router: &axum::Router, username: &str) -> (String, String) {
        let body = format!("username={username}&password=librarian");
        let res = router
            .clone()
            .oneshot(
                Request::post("/login")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        let cookie = res
            .headers()
            .get_all(axum::http::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|s| s.starts_with("session="))
            .and_then(|s| s.split(';').next())
            .and_then(|kv| kv.split_once('='))
            .map(|(_, v)| {
                percent_encoding::percent_decode_str(v)
                    .decode_utf8_lossy()
                    .to_string()
            })
            .unwrap();
        let html = {
            let r = router
                .clone()
                .oneshot(
                    Request::get("/")
                        .header("cookie", format!("session={}", cookie))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let bytes = axum::body::to_bytes(r.into_body(), 1024 * 1024)
                .await
                .unwrap();
            String::from_utf8(bytes.to_vec()).unwrap()
        };
        let needle = "name=\"csrf-token\" content=\"";
        let start = html.find(needle).unwrap() + needle.len();
        let rest = &html[start..];
        let end = rest.find('"').unwrap();
        (cookie, rest[..end].to_string())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn delete_title_with_zero_volumes_succeeds(pool: MySqlPool) {
        let username = seed_librarian(&pool).await;
        let id = seed_title(&pool, "Lone title", None).await;
        let router = crate::routes::build_router(state_with_pool(pool.clone()));
        let (cookie, csrf) = login(&router, &username).await;

        let res = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/title/{id}"))
                    .header("cookie", format!("session={}", cookie))
                    .header("hx-request", "true")
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers().get("hx-redirect").is_some(),
            "success path must emit HX-Redirect"
        );

        // CLAUDE.md MariaDB gotcha #4: titles.deleted_at is TIMESTAMP
        // (not DATETIME), so a bare `SELECT deleted_at` can't decode
        // into NaiveDateTime via dynamic query_as. Cast to DATETIME.
        let (deleted_at,): (Option<chrono::NaiveDateTime>,) =
            sqlx::query_as("SELECT CAST(deleted_at AS DATETIME) FROM titles WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(deleted_at.is_some(), "title must be soft-deleted");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn delete_title_with_active_volumes_returns_conflict(pool: MySqlPool) {
        let username = seed_librarian(&pool).await;
        let id = seed_title(&pool, "Title with vol", None).await;
        let _v = seed_volume(&pool, id).await;
        let router = crate::routes::build_router(state_with_pool(pool.clone()));
        let (cookie, csrf) = login(&router, &username).await;

        let res = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/title/{id}"))
                    .header("cookie", format!("session={}", cookie))
                    .header("hx-request", "true")
                    .header("x-csrf-token", csrf)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);

        // Row must NOT be soft-deleted.
        // CLAUDE.md MariaDB gotcha #4: titles.deleted_at is TIMESTAMP
        // (not DATETIME), so a bare `SELECT deleted_at` can't decode
        // into NaiveDateTime via dynamic query_as. Cast to DATETIME.
        let (deleted_at,): (Option<chrono::NaiveDateTime>,) =
            sqlx::query_as("SELECT CAST(deleted_at AS DATETIME) FROM titles WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(deleted_at.is_none());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn delete_modal_renders_for_zero_volume_title(pool: MySqlPool) {
        let username = seed_librarian(&pool).await;
        let id = seed_title(&pool, "Modal title", None).await;
        let router = crate::routes::build_router(state_with_pool(pool.clone()));
        let (cookie, _csrf) = login(&router, &username).await;

        let res = router
            .clone()
            .oneshot(
                Request::get(format!("/title/{id}/delete-modal"))
                    .header("cookie", format!("session={}", cookie))
                    .header("hx-request", "true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 64 * 1024).await.unwrap();
        let html = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(html.contains("Modal title"));
        assert!(html.contains(&format!("/title/{id}")));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn delete_modal_blocked_when_title_has_volumes(pool: MySqlPool) {
        let username = seed_librarian(&pool).await;
        let id = seed_title(&pool, "Blocked title", None).await;
        let _v = seed_volume(&pool, id).await;
        let router = crate::routes::build_router(state_with_pool(pool));
        let (cookie, _csrf) = login(&router, &username).await;

        let res = router
            .clone()
            .oneshot(
                Request::get(format!("/title/{id}/delete-modal"))
                    .header("cookie", format!("session={}", cookie))
                    .header("hx-request", "true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn delete_modal_direct_nav_returns_405(pool: MySqlPool) {
        let username = seed_librarian(&pool).await;
        let id = seed_title(&pool, "Direct nav", None).await;
        let router = crate::routes::build_router(state_with_pool(pool));
        let (cookie, _csrf) = login(&router, &username).await;

        // No hx-request header → direct browser nav. Per the pattern
        // used by delete_volume_modal, this returns 405.
        let res = router
            .clone()
            .oneshot(
                Request::get(format!("/title/{id}/delete-modal"))
                    .header("cookie", format!("session={}", cookie))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
