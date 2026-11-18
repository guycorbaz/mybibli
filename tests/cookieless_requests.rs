//! #696 — requests that cannot use a session leave no row in `sessions`,
//! and an unrouted path answers 404 before CSRF.
//!
//! The whole router runs (`tower::oneshot`), so the session resolver, the
//! CSRF layer and the fallback are exercised in their real order.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mybibli::db::DbPool;
use tower::ServiceExt;

use common::mcp::router;
use common::web::{Browser, seed_admin};

async fn session_rows(pool: &DbPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Send `method path` without any cookie; status, `Set-Cookie: session=`
/// presence and body.
async fn send(pool: &DbPool, method: &str, path: &str, body: &str) -> (StatusCode, bool, String) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = router(pool).oneshot(req).await.unwrap();
    let status = res.status();
    let sets_cookie = res
        .headers()
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|v| v.starts_with("session="));
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, sets_cookie, String::from_utf8_lossy(&bytes).to_string())
}

#[sqlx::test(migrations = "./migrations")]
async fn post_mcp_answers_404_naming_the_endpoint_without_a_session_row(pool: DbPool) {
    let before = session_rows(&pool).await;
    let (status, sets_cookie, body) =
        send(&pool, "POST", "/mcp", r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("/api/mcp"), "{body}");
    assert!(!sets_cookie);
    assert_eq!(session_rows(&pool).await, before);
}

#[sqlx::test(migrations = "./migrations")]
async fn unrouted_and_api_requests_persist_no_session(pool: DbPool) {
    let before = session_rows(&pool).await;
    for (method, path, expected) in [
        ("GET", "/does-not-exist", StatusCode::NOT_FOUND),
        ("POST", "/does-not-exist", StatusCode::NOT_FOUND),
        ("DELETE", "/does-not-exist/42", StatusCode::NOT_FOUND),
        ("GET", "/api/v1/titles", StatusCode::UNAUTHORIZED),
        ("POST", "/api/mcp", StatusCode::UNAUTHORIZED),
    ] {
        let (status, sets_cookie, body) = send(&pool, method, path, "").await;
        assert_eq!(status, expected, "{method} {path}: {body}");
        assert!(!sets_cookie, "{method} {path} set a session cookie");
    }
    assert_eq!(session_rows(&pool).await, before);
}

/// Also what a browser meets once its session cookie expired (Max-Age):
/// the same refusal as before #696 — HTMX gets the 403 "session expired,
/// reload" entry (the page stays), a plain anonymous form the 303 to
/// `/login` — but no row and no cookie.
#[sqlx::test(migrations = "./migrations")]
async fn a_cookieless_post_to_a_route_is_refused_without_a_session_row(pool: DbPool) {
    let before = session_rows(&pool).await;
    let req = Request::post("/catalog/scan")
        .header("hx-request", "true")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("code=9782070360246"))
        .unwrap();
    let res = router(&pool).oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(res.headers().get("hx-trigger").unwrap(), "csrf-rejected");
    assert_eq!(res.headers().get("cache-control").unwrap(), "no-store");
    assert!(res.headers().get(axum::http::header::SET_COOKIE).is_none());

    let req = Request::post("/catalog/scan")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from("code=9782070360246"))
        .unwrap();
    let res = router(&pool).oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(res.headers().get("location").unwrap(), "/login");
    assert_eq!(session_rows(&pool).await, before);
}

/// Two `session=` cookies (anti-shadowing) are not "no cookie": the request
/// keeps the resolver's fresh anonymous row and cookie, as before #696.
#[sqlx::test(migrations = "./migrations")]
async fn two_session_cookies_are_not_treated_as_cookieless(pool: DbPool) {
    let before = session_rows(&pool).await;
    let req = Request::post("/catalog/scan")
        .header("cookie", "session=aaa; session=bbb")
        .header("hx-request", "true")
        .body(Body::from("code=9782070360246"))
        .unwrap();
    let res = router(&pool).oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(res.headers().get(axum::http::header::SET_COOKIE).is_some(), "the resolver still mints");
    assert_eq!(session_rows(&pool).await, before + 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn cookieless_head_and_options_persist_no_session(pool: DbPool) {
    let before = session_rows(&pool).await;
    for method in ["HEAD", "OPTIONS"] {
        let (_, sets_cookie, _) = send(&pool, method, "/catalog", "").await;
        assert!(!sets_cookie, "{method}");
    }
    assert_eq!(session_rows(&pool).await, before);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_cookieless_page_view_still_mints_an_anonymous_session(pool: DbPool) {
    let before = session_rows(&pool).await;
    let (status, sets_cookie, _) = send(&pool, "GET", "/login", "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(sets_cookie);
    assert_eq!(session_rows(&pool).await, before + 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_cookieless_login_still_signs_in(pool: DbPool) {
    seed_admin(&pool, "cookieless").await;
    // `Browser::login` posts `/login` without any cookie, then loads `/`.
    let browser = Browser::login(&router(&pool), "cookieless").await;
    assert!(!browser.cookie.is_empty());
    assert!(!browser.csrf.is_empty());
}
