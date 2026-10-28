//! Story 11-10 — browser-side helpers for router-level tests: seed an
//! admin, log in through `POST /login`, read the CSRF token from the
//! page, post an HTMX form. Same flow as `tests/scan_title_context.rs`.
#![allow(dead_code)] // each test crate uses a different subset

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use mybibli::db::DbPool;
use tower::ServiceExt;

/// Argon2 hash of the password "librarian" (shared across the suite).
pub const PW_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU";

/// An active admin (clears the Librarian gate and the setup wizard).
pub async fn seed_admin(pool: &DbPool, username: &str) {
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'admin')")
        .bind(username)
        .bind(PW_HASH)
        .execute(pool)
        .await
        .unwrap();
}

fn session_cookie(res: &axum::response::Response) -> Option<String> {
    let header = res
        .headers()
        .get_all(axum::http::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|s| s.starts_with("session="))?;
    let (_, raw) = header.split(';').next()?.split_once('=')?;
    Some(
        percent_encoding::percent_decode_str(raw)
            .decode_utf8_lossy()
            .to_string(),
    )
}

/// A logged-in browser: its session cookie and CSRF token.
pub struct Browser {
    pub cookie: String,
    pub csrf: String,
}

impl Browser {
    pub async fn login(router: &Router, username: &str) -> Self {
        let res = router
            .clone()
            .oneshot(
                Request::post("/login")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "username={username}&password=librarian"
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SEE_OTHER, "login should succeed");
        let cookie = session_cookie(&res).expect("session cookie");
        let res = router
            .clone()
            .oneshot(
                Request::get("/")
                    .header("cookie", format!("session={cookie}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let html = body_string(res).await;
        let needle = "name=\"csrf-token\" content=\"";
        let start = html.find(needle).expect("csrf-token meta tag") + needle.len();
        let end = html[start..].find('"').unwrap();
        let csrf = html[start..start + end].to_string();
        Browser { cookie, csrf }
    }

    /// An HTMX form POST; returns status and body.
    pub async fn post_form(&self, router: &Router, path: &str, body: &str) -> (StatusCode, String) {
        let res = router
            .clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header("cookie", format!("session={}", self.cookie))
                    .header("X-CSRF-Token", &self.csrf)
                    .header("HX-Request", "true")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        (status, body_string(res).await)
    }
}

pub async fn body_string(res: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}
