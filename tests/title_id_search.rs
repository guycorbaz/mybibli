//! Story 17-5 (#653, FR176) — `#<id>` in the home search field finds the
//! title the MCP agent names by id; a title in the Trash or an unknown id
//! says so; the home scan opens the title; the title page shows its id.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test title_id_search

mod common;

use axum::http::StatusCode;
use common::mcp::{router, seed_title};
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::services::search::{SearchOutcome, SearchService};
use mybibli::services::soft_delete::SoftDeleteService;
use mybibli::services::title_id_search::TitleIdMiss;

enum Found {
    Ids(Vec<u64>),
    Miss(TitleIdMiss),
}

/// The web search, with a genre filter the title does not match.
async fn search(pool: &DbPool, query: &str, genre_id: Option<u64>) -> Found {
    match SearchService::search(pool, query, genre_id, None, &None, &None, 1, false, true, None)
        .await
        .expect("search")
    {
        SearchOutcome::Results(list) => Found::Ids(list.items.iter().map(|r| r.id).collect()),
        SearchOutcome::TitleIdMiss(miss) => Found::Miss(miss),
        SearchOutcome::Redirect(url) => panic!("unexpected redirect to {url}"),
    }
}

async fn genre_of(pool: &DbPool, title_id: u64) -> u64 {
    sqlx::query_scalar("SELECT genre_id FROM titles WHERE id = ?")
        .bind(title_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn setup(pool: &DbPool) -> (axum::Router, Browser) {
    seed_admin(pool, "tid_admin").await;
    sqlx::query("UPDATE users SET preferred_language = 'en' WHERE username = 'tid_admin'")
        .execute(pool)
        .await
        .unwrap();
    let router = router(pool);
    let browser = Browser::login(&router, "tid_admin").await;
    (router, browser)
}

async fn hx_redirect(router: &axum::Router, browser: &Browser, path: &str) -> Option<String> {
    let res = browser.send(router, "GET", path, "", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let target = res
        .headers()
        .get("hx-redirect")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let _ = body_string(res).await;
    target
}

#[sqlx::test(migrations = "./migrations")]
async fn an_id_lists_its_title_ignoring_the_filters(pool: DbPool) {
    let title = seed_title(&pool, "Luc Orient 4").await;
    let other_genre = genre_of(&pool, title).await + 1000;

    let Found::Ids(ids) = search(&pool, &format!("#{title}"), Some(other_genre)).await else {
        panic!("a live title is a hit");
    };
    assert_eq!(ids, vec![title]);
    let Found::Ids(ids) = search(&pool, &format!("  #{title} "), None).await else {
        panic!("a trimmed id is a hit");
    };
    assert_eq!(ids, vec![title]);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_id_in_the_trash_or_unknown_is_a_miss(pool: DbPool) {
    let title = seed_title(&pool, "Luc Orient 4 (doublon)").await;
    SoftDeleteService::soft_delete(&pool, "titles", title).await.unwrap();

    let Found::Miss(miss) = search(&pool, &format!("#{title}"), None).await else {
        panic!("a title in the Trash is a miss");
    };
    assert_eq!(miss, TitleIdMiss { id: title, in_trash: true });

    let unknown = title + 1000;
    let Found::Miss(miss) = search(&pool, &format!("#{unknown}"), Some(1)).await else {
        panic!("an unknown id is a miss");
    };
    assert_eq!(miss, TitleIdMiss { id: unknown, in_trash: false });
}

#[sqlx::test(migrations = "./migrations")]
async fn a_bare_number_stays_a_text_search(pool: DbPool) {
    let title = seed_title(&pool, "Pas de numéro").await;
    let named = seed_title(&pool, &format!("Numéro {title}")).await;
    let Found::Ids(ids) = search(&pool, &title.to_string(), None).await else {
        panic!("a bare number is never an id");
    };
    // The text search finds the title whose name holds the digits, not
    // the title whose id they are.
    assert!(ids.contains(&named), "{ids:?}");
    assert!(!ids.contains(&title), "{ids:?}");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_home_search_explains_a_miss(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let title = seed_title(&pool, "Titre supprimé").await;
    SoftDeleteService::soft_delete(&pool, "titles", title).await.unwrap();

    // HTMX live search, with a filter active.
    let res = browser
        .send(&router, "GET", &format!("/?q=%23{title}&filter=uncategorized"), "", true)
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(body.contains(&format!("Title #{title} is in the Trash")), "{body}");
    assert!(!body.contains("/catalog/title/new"), "no call to action: {body}");

    // Direct navigation (the full page).
    let unknown = title + 1000;
    let (status, body) = browser.get(&router, &format!("/?q=%23{unknown}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(&format!("No title has the ID #{unknown}")), "{body}");
    assert!(!body.contains("Add this title"), "no call to action: {body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_anonymous_visitor_is_not_told_about_the_trash(pool: DbPool) {
    let router = router(&pool);
    let title = seed_title(&pool, "Titre supprimé (anonyme)").await;
    SoftDeleteService::soft_delete(&pool, "titles", title).await.unwrap();

    let browser = Browser::anonymous(&router).await;
    let res = browser.send(&router, "GET", &format!("/?q=%23{title}&lang=en"), "", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_string(res).await;
    assert!(!body.contains(&format!("#{title} is in the Trash")), "{body}");
    assert!(!body.contains(&format!("#{title} est dans la corbeille")), "{body}");
    assert!(body.contains(&format!("#{title}")), "the miss still names the id: {body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_home_scan_opens_a_live_title_and_searches_a_miss(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let title = seed_title(&pool, "Luc Orient 5").await;

    assert_eq!(
        hx_redirect(&router, &browser, &format!("/scan?q=%23{title}")).await.as_deref(),
        Some(format!("/title/{title}").as_str())
    );
    SoftDeleteService::soft_delete(&pool, "titles", title).await.unwrap();
    assert_eq!(
        hx_redirect(&router, &browser, &format!("/scan?q=%23{title}")).await.as_deref(),
        Some(format!("/?q=%23{title}").as_str())
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn the_title_page_shows_its_id(pool: DbPool) {
    let (router, browser) = setup(&pool).await;
    let title = seed_title(&pool, "Luc Orient 6").await;

    let (status, body) = browser.get(&router, &format!("/title/{title}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("id=\"title-id\""), "{body}");
    assert!(body.contains(&format!("<span data-title-id>#{title}</span>")), "{body}");
}
