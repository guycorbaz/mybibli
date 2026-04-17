//! Regression test for the home-page "duplicated page" bug (2026-04-17).
//!
//! Reproduced manually by clicking a genre pill on `/` with an empty query —
//! the response rendered the full page template (nav, hero, search, pills,
//! sort-by) which HTMX swapped into `#browse-results`, duplicating the layout.
//!
//! Root cause: `SearchService::search` early-returned empty results whenever
//! `query.trim().is_empty()`, even if a genre filter was set. Combined with
//! `home.rs` gating the HTMX fragment on non-empty query, filter-only HTMX
//! requests fell through to the full-page render branch.
//!
//! This test locks in the "empty query + genre filter returns filtered
//! results" contract at the service layer. The complementary E2E test lives
//! in `tests/e2e/specs/journeys/home-search.spec.ts`.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test search_filter_browse

use mybibli::services::search::{SearchOutcome, SearchService};
use sqlx::MySqlPool;

async fn seed_title(pool: &MySqlPool, title: &str, genre_id: u64) -> u64 {
    let result = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id) \
         VALUES (?, 'fr', 'book', ?)",
    )
    .bind(title)
    .bind(genre_id)
    .execute(pool)
    .await
    .expect("insert title");
    result.last_insert_id()
}

async fn first_genre_id(pool: &MySqlPool) -> u64 {
    sqlx::query_scalar::<_, u64>(
        "SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("bootstrap migration must seed at least one genre")
}

async fn other_genre_id(pool: &MySqlPool, first: u64) -> u64 {
    sqlx::query_scalar::<_, u64>(
        "SELECT id FROM genres WHERE deleted_at IS NULL AND id != ? ORDER BY id LIMIT 1",
    )
    .bind(first)
    .fetch_one(pool)
    .await
    .expect("bootstrap migration must seed at least two genres")
}

#[sqlx::test(migrations = "./migrations")]
async fn empty_query_with_genre_filter_returns_filtered_titles(pool: MySqlPool) {
    let genre_a = first_genre_id(&pool).await;
    let genre_b = other_genre_id(&pool, genre_a).await;

    let _ = seed_title(&pool, "Matching One", genre_a).await;
    let _ = seed_title(&pool, "Matching Two", genre_a).await;
    let _ = seed_title(&pool, "Other genre", genre_b).await;

    let outcome = SearchService::search(&pool, "", Some(genre_a), None, &None, &None, 1)
        .await
        .expect("search must succeed");

    match outcome {
        SearchOutcome::Results(paginated) => {
            assert_eq!(
                paginated.items.len(),
                2,
                "expected 2 titles in genre_a, got {}",
                paginated.items.len()
            );
            assert_eq!(paginated.total_items, 2);
            // All 3 seeded titles used `media_type='book'`; assert the 2 we
            // got back are NOT the "Other genre" outlier by title.
            let titles: Vec<&str> = paginated.items.iter().map(|i| i.title.as_str()).collect();
            assert!(titles.contains(&"Matching One"), "got {:?}", titles);
            assert!(titles.contains(&"Matching Two"), "got {:?}", titles);
            assert!(!titles.contains(&"Other genre"), "got {:?}", titles);
        }
        SearchOutcome::Redirect(_) => panic!("unexpected redirect for filter-only browse"),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn empty_query_without_filter_returns_empty(pool: MySqlPool) {
    let genre_a = first_genre_id(&pool).await;
    let _ = seed_title(&pool, "Something", genre_a).await;

    let outcome = SearchService::search(&pool, "", None, None, &None, &None, 1)
        .await
        .expect("search must succeed");

    match outcome {
        SearchOutcome::Results(paginated) => {
            assert_eq!(
                paginated.items.len(),
                0,
                "empty query + no filter must return empty results (don't flood the home page)"
            );
            assert_eq!(paginated.total_items, 0);
        }
        SearchOutcome::Redirect(_) => panic!("unexpected redirect"),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn whitespace_query_with_filter_is_treated_as_filter_only_browse(pool: MySqlPool) {
    let genre_a = first_genre_id(&pool).await;
    let _ = seed_title(&pool, "Matching", genre_a).await;

    let outcome = SearchService::search(&pool, "   ", Some(genre_a), None, &None, &None, 1)
        .await
        .expect("search must succeed");

    match outcome {
        SearchOutcome::Results(paginated) => {
            assert_eq!(
                paginated.items.len(),
                1,
                "whitespace-only query + filter must still browse by filter"
            );
        }
        SearchOutcome::Redirect(_) => panic!("unexpected redirect"),
    }
}
