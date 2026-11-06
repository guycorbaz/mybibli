//! Story 16-5 (#638, FR170) — a scanned code of no known barcode shape asks
//! first, even with a media-type preference; nothing is created or rewritten.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test scan_irregular_code

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::mcp::router;
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use tower::ServiceExt;

async fn scan(app: &Router, who: &Browser, code: &str, prefer_book: bool) -> (StatusCode, String) {
    let cookie = if prefer_book {
        format!("session={}; media_type_preference=book", who.cookie)
    } else {
        format!("session={}", who.cookie)
    };
    let res = app
        .clone()
        .oneshot(
            Request::post("/catalog/scan")
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", cookie)
                .header("X-CSRF-Token", &who.csrf)
                .header("HX-Request", "true")
                .body(Body::from(format!("code={code}")))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    (status, body_string(res).await)
}

async fn titles_named(pool: &DbPool, code: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM titles WHERE title = ? OR upc = ?")
        .bind(code)
        .bind(code)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn an_irregular_code_asks_first_even_with_a_preference(pool: DbPool) {
    let app = router(&pool);
    seed_admin(&pool, "sc-admin").await;
    let who = Browser::login(&app, "sc-admin").await;
    for (code, needle) in [
        ("220500218", "9"),
        ("2205025062", "10"),
        ("12345678901", "11"),
        ("12345678", "code-warning"),
        ("0093624738627", "code-warning"),
    ] {
        for prefer in [false, true] {
            let (status, body) = scan(&app, &who, code, prefer).await;
            assert_eq!(status, StatusCode::OK, "{code}");
            assert!(body.contains("data-code-warning"), "{code} ({prefer}): {body}");
            assert!(body.contains(needle), "{code}: {body}");
            assert!(body.contains("role=\"radio\""), "{code}: the choice is offered");
            assert_eq!(titles_named(&pool, code).await, 0, "{code}: nothing created");
        }
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_valid_barcode_behaves_as_before(pool: DbPool) {
    let app = router(&pool);
    seed_admin(&pool, "sc-admin").await;
    let who = Browser::login(&app, "sc-admin").await;
    let code = "0093624738626";
    let (_, body) = scan(&app, &who, code, false).await;
    assert!(!body.contains("data-code-warning"), "{body}");
    assert!(body.contains("role=\"radio\""), "{body}");
    assert_eq!(titles_named(&pool, code).await, 0);
    // With a preference it is created straight away.
    let (_, body) = scan(&app, &who, code, true).await;
    assert!(!body.contains("data-code-warning"), "{body}");
    assert_eq!(titles_named(&pool, code).await, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_confirmed_irregular_code_is_kept_as_entered(pool: DbPool) {
    let app = router(&pool);
    seed_admin(&pool, "sc-admin").await;
    let who = Browser::login(&app, "sc-admin").await;
    let code = "220500218";
    let (status, _) = who
        .post_form(
            &app,
            "/catalog/scan-with-type",
            &format!("code={code}&media_type=bd"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (upc,): (String,) = sqlx::query_as("SELECT upc FROM titles WHERE upc = ?")
        .bind(code)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(upc, code);

    // Rescanned (a second copy): its title is known, no warning, no new title.
    let (_, body) = scan(&app, &who, code, true).await;
    assert!(!body.contains("data-code-warning"), "{body}");
    assert_eq!(titles_named(&pool, code).await, 1);
}
