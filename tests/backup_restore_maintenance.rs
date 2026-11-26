//! Story 19-8 (#657, D8, AC6) — maintenance while a restore runs: what a
//! page, an HTMX request, the API, `/health` and `/static` get; a request
//! already running is waited for; the scheduled metadata retry skips.

use crate::common;

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::services::maintenance::Maintenance;
use tower::ServiceExt;

fn app(state: mybibli::AppState) -> axum::Router {
    mybibli::routes::build_router_with_mcp(state, mybibli::mcp::McpConfig::default())
}

async fn send(app: &axum::Router, req: Request<Body>) -> axum::response::Response {
    app.clone().oneshot(req).await.unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn while_active_every_request_gets_the_maintenance_answer(pool: DbPool) {
    let state = common::mcp::state_with_pool(pool.clone());
    let app = app(state.clone());
    seed_admin(&pool, "maint_admin").await;
    let admin = Browser::login(&app, "maint_admin").await;
    let guard = state.backup.maintenance.begin(Duration::from_secs(1)).await.unwrap();

    // A page: 503, Retry-After, the standalone page in the cookie's locale.
    let res = send(
        &app,
        Request::get("/").header("cookie", "lang=en").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(res.headers().get("retry-after").unwrap(), "60");
    assert!(res.headers().get("content-security-policy").is_some(), "CSP still wraps it");
    let page = body_string(res).await;
    assert!(page.contains("<title>Maintenance — mybibli</title>"), "{page}");
    assert!(page.contains("mybibli is restoring a backup. This takes a minute or two"));
    assert!(!page.contains("<script"), "no script on the page");

    // Accept-Language when there is no cookie.
    let res = send(
        &app,
        Request::get("/catalog").header("accept-language", "fr-CH,fr;q=0.9").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(body_string(res).await.contains("mybibli restaure une sauvegarde."));

    // HTMX: the same text as an error FeedbackEntry, retargeted.
    let res = admin.send(&app, "POST", "/admin/backups", "", true).await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(res.headers().get("hx-retarget").unwrap(), "#feedback-list");
    assert_eq!(res.headers().get("retry-after").unwrap(), "60");
    let body = body_string(res).await;
    assert!(body.contains("feedback-entry"), "{body}");
    assert!(body.contains("restaure une sauvegarde") || body.contains("restoring a backup"), "{body}");

    // The API: JSON (agent-facing English).
    let res = send(&app, Request::get("/api/v1/titles").body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let json: serde_json::Value = serde_json::from_str(&body_string(res).await).unwrap();
    assert_eq!(json["error"], "maintenance");
    assert_eq!(json["message"], "mybibli is restoring a backup; try again in a minute.");

    // Covers are refused too (being replaced); /health and /static pass.
    let res = send(&app, Request::get("/covers/1.jpg").body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let res = send(&app, Request::get("/health").body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = send(&app, Request::get("/static/js/modal.js").body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::OK, "static files are served");

    // No session row was created for any refused request.
    drop(guard);
    let res = send(&app, Request::get("/health").body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::OK);
    let (status, _) = admin.get(&app, "/").await;
    assert_eq!(status, StatusCode::OK, "served again once the restore ended");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_stuck_restore_names_the_safety_backup_to_restore_by_hand(pool: DbPool) {
    let state = common::mcp::state_with_pool(pool.clone());
    let app = app(state.clone());
    let guard = state.backup.maintenance.begin(Duration::from_secs(1)).await.unwrap();
    state.backup.maintenance.mark_stuck("mybibli-0123456789abcdef-20261126T080000Z.tar.gz");
    drop(guard);
    let res = send(
        &app,
        Request::get("/").header("cookie", "lang=en").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    let page = body_string(res).await;
    assert!(page.contains("restore the backup mybibli-0123456789abcdef-20261126T080000Z.tar.gz by hand"), "{page}");
}

/// A handler that runs until told to stop, behind the maintenance layer.
#[sqlx::test(migrations = "./migrations")]
async fn a_request_started_before_is_waited_for(pool: DbPool) {
    let mut state = common::mcp::state_with_pool(pool.clone());
    state.backup.maintenance = Arc::new(Maintenance::with_drain(Duration::from_secs(10)));
    let release = Arc::new(tokio::sync::Notify::new());
    let started = Arc::new(tokio::sync::Notify::new());
    let (r, s) = (release.clone(), started.clone());
    let slow = axum::Router::new()
        .route(
            "/slow",
            get(move || {
                let (r, s) = (r.clone(), s.clone());
                async move {
                    s.notify_one();
                    r.notified().await;
                    "done"
                }
            }),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            mybibli::middleware::maintenance::maintenance_middleware,
        ))
        .with_state(state.clone());

    let request = tokio::spawn({
        let slow = slow.clone();
        async move { slow.oneshot(Request::get("/slow").body(Body::empty()).unwrap()).await.unwrap() }
    });
    started.notified().await;
    let m = state.backup.maintenance.clone();
    let begin = tokio::spawn(async move { m.begin_default().await });
    // The restore waits: maintenance is on, new requests are refused.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !state.backup.maintenance.is_active() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let refused = slow.clone().oneshot(Request::get("/slow").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!begin.is_finished(), "the running request is waited for");
    release.notify_one();
    let res = request.await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "the running request ended normally");
    let guard = tokio::time::timeout(Duration::from_secs(5), begin)
        .await
        .unwrap()
        .unwrap()
        .expect("drained");
    drop(guard);
    assert!(!state.backup.maintenance.is_active());
}

#[sqlx::test(migrations = "./migrations")]
async fn the_scheduled_metadata_retry_skips_while_active(pool: DbPool) {
    let state = common::mcp::state_with_pool(pool.clone());
    let title_id = sqlx::query(
        "INSERT INTO titles (title, media_type, genre_id) VALUES ('Maintenance retry', 'book', (SELECT MIN(id) FROM genres))",
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id();
    sqlx::query("INSERT INTO metadata_fetch_retries (title_id, attempts, next_attempt_at) VALUES (?, 1, '2020-01-01 00:00:00')")
        .bind(title_id)
        .execute(&pool)
        .await
        .unwrap();
    let guard = state.backup.maintenance.begin(Duration::from_secs(1)).await.unwrap();
    let stats = mybibli::tasks::metadata_retry::tick(&state).await.unwrap();
    assert_eq!(stats, Default::default(), "nothing looked up, nothing written");
    let (attempts, version): (i32, i32) = sqlx::query_as(
        "SELECT attempts, version FROM metadata_fetch_retries WHERE title_id = ?",
    )
    .bind(title_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((attempts, version), (1, 1), "the queue row is untouched");
    drop(guard);
}
