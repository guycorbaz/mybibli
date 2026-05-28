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

    let outcome = SearchService::search(&pool, "", Some(genre_a), None, &None, &None, 1, false, false)
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

    let outcome = SearchService::search(&pool, "", None, None, &None, &None, 1, false, false)
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

    let outcome = SearchService::search(&pool, "   ", Some(genre_a), None, &None, &None, 1, false, false)
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

/// CR #279 — the `no_volumes_only` flag restricts results to titles
/// that have no active volume row. Locks the `NOT EXISTS` branch in
/// `TitleModel::active_search`. Seeds 3 titles: one bare, one with a
/// live volume, one with a soft-deleted volume only. The bare title
/// AND the title with only soft-deleted volumes must surface; the
/// title with a live volume must NOT.
#[sqlx::test(migrations = "./migrations")]
async fn no_volumes_only_filters_titles_with_active_volumes(pool: MySqlPool) {
    let genre = first_genre_id(&pool).await;

    let _bare = seed_title(&pool, "Bare Title", genre).await;
    let with_volume = seed_title(&pool, "Has Volume", genre).await;
    let soft_only = seed_title(&pool, "Soft Only", genre).await;

    sqlx::query("INSERT INTO volumes (title_id, label) VALUES (?, 'V9001')")
        .bind(with_volume)
        .execute(&pool)
        .await
        .expect("insert live volume");

    // Soft-deleted volume on `soft_only` — NOT EXISTS guard must still
    // surface this title (the `WHERE deleted_at IS NULL` inside the
    // subquery is the load-bearing predicate).
    sqlx::query("INSERT INTO volumes (title_id, label, deleted_at) VALUES (?, 'V9002', NOW())")
        .bind(soft_only)
        .execute(&pool)
        .await
        .expect("insert soft-deleted volume");

    let outcome = SearchService::search(&pool, "", None, None, &None, &None, 1, true, false)
        .await
        .expect("search must succeed");

    match outcome {
        SearchOutcome::Results(paginated) => {
            let titles: Vec<&str> = paginated.items.iter().map(|i| i.title.as_str()).collect();
            assert!(
                titles.contains(&"Bare Title"),
                "bare title must surface, got {:?}",
                titles
            );
            assert!(
                titles.contains(&"Soft Only"),
                "title with only soft-deleted volumes must surface (deleted_at-aware), got {:?}",
                titles
            );
            assert!(
                !titles.contains(&"Has Volume"),
                "title with a live volume must NOT surface, got {:?}",
                titles
            );
        }
        SearchOutcome::Redirect(_) => panic!("unexpected redirect for no_volumes browse"),
    }
}

/// CR #279 — when `no_volumes_only` is `true`, the empty-query
/// short-circuit must NOT fire. The handler-shortcut depends on this:
/// `?filter=no_volumes` lands with no query and no genre filter set.
#[sqlx::test(migrations = "./migrations")]
async fn no_volumes_only_disables_empty_query_short_circuit(pool: MySqlPool) {
    let genre = first_genre_id(&pool).await;
    let _ = seed_title(&pool, "Bare", genre).await;

    let outcome = SearchService::search(&pool, "", None, None, &None, &None, 1, true, false)
        .await
        .expect("search must succeed");

    match outcome {
        SearchOutcome::Results(paginated) => {
            assert!(
                paginated.total_items >= 1,
                "no_volumes filter must override the empty-query short-circuit"
            );
        }
        SearchOutcome::Redirect(_) => panic!("unexpected redirect"),
    }
}

/// CR #355 — the `no_cover_only` flag restricts results to titles whose
/// `cover_image_url` is NULL or empty string. Seeds 3 titles: one with a
/// real cover URL, one left NULL, one set to empty string. The NULL and
/// empty-string titles must surface; the title with a cover must NOT.
#[sqlx::test(migrations = "./migrations")]
async fn no_cover_only_filters_titles_with_cover(pool: MySqlPool) {
    let genre = first_genre_id(&pool).await;

    let with_cover = seed_title(&pool, "Has Cover", genre).await;
    let _null_cover = seed_title(&pool, "Null Cover", genre).await; // cover defaults to NULL
    let empty_cover = seed_title(&pool, "Empty Cover", genre).await;

    sqlx::query("UPDATE titles SET cover_image_url = '/covers/has-cover.jpg' WHERE id = ?")
        .bind(with_cover)
        .execute(&pool)
        .await
        .expect("set cover url");
    sqlx::query("UPDATE titles SET cover_image_url = '' WHERE id = ?")
        .bind(empty_cover)
        .execute(&pool)
        .await
        .expect("set empty cover");

    let outcome = SearchService::search(&pool, "", None, None, &None, &None, 1, false, true)
        .await
        .expect("search must succeed");

    match outcome {
        SearchOutcome::Results(paginated) => {
            let titles: Vec<&str> = paginated.items.iter().map(|i| i.title.as_str()).collect();
            assert!(
                titles.contains(&"Null Cover"),
                "NULL-cover title must surface, got {:?}",
                titles
            );
            assert!(
                titles.contains(&"Empty Cover"),
                "empty-string-cover title must surface, got {:?}",
                titles
            );
            assert!(
                !titles.contains(&"Has Cover"),
                "title with a cover must NOT surface, got {:?}",
                titles
            );
        }
        SearchOutcome::Redirect(_) => panic!("unexpected redirect for no_cover browse"),
    }
}

/// CR #355 — when `no_cover_only` is `true`, the empty-query short-circuit
/// must NOT fire (mirror of the no_volumes case). `?filter=no_cover` lands
/// with no query and no genre filter set.
#[sqlx::test(migrations = "./migrations")]
async fn no_cover_only_disables_empty_query_short_circuit(pool: MySqlPool) {
    let genre = first_genre_id(&pool).await;
    let _ = seed_title(&pool, "Coverless", genre).await; // cover defaults to NULL

    let outcome = SearchService::search(&pool, "", None, None, &None, &None, 1, false, true)
        .await
        .expect("search must succeed");

    match outcome {
        SearchOutcome::Results(paginated) => {
            assert!(
                paginated.total_items >= 1,
                "no_cover filter must override the empty-query short-circuit"
            );
        }
        SearchOutcome::Redirect(_) => panic!("unexpected redirect"),
    }
}
