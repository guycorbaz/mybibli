//! Story 11-1 (#497) — a home-page search by ISBN-10 finds the title
//! stored under its ISBN-13.
//!
//! Before 11-1, `search::detect_code` accepted the ISBN-10 shape but looked
//! it up verbatim, so `TitleModel::find_by_isbn` never matched (titles keep
//! the ISBN-13) and the query fell through to an empty fulltext search.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!         DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test search_isbn10

use mybibli::services::search::{SearchOutcome, SearchService};
use mybibli::services::title::TitleService;
use sqlx::MySqlPool;

async fn search_ids(pool: &MySqlPool, query: &str) -> Vec<u64> {
    match SearchService::search(pool, query, None, None, &None, &None, 1, false, false)
        .await
        .expect("search")
    {
        SearchOutcome::Results(list) => list.items.iter().map(|r| r.id).collect(),
        SearchOutcome::Redirect(url) => panic!("unexpected redirect to {url}"),
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn search_by_isbn10_finds_title_stored_under_isbn13(pool: MySqlPool) {
    let (title, is_new) = TitleService::create_from_isbn(&pool, "9782205001211", None)
        .await
        .expect("create title");
    assert!(is_new);

    assert_eq!(search_ids(&pool, "2205001213").await, vec![title.id]);
    assert_eq!(search_ids(&pool, "  2205001213 ").await, vec![title.id]);
}

#[sqlx::test(migrations = "./migrations")]
async fn search_by_isbn10_with_x_check_digit_finds_title(pool: MySqlPool) {
    let (title, _) = TitleService::create_from_isbn(&pool, "9780804429573", None)
        .await
        .expect("create title");

    assert_eq!(search_ids(&pool, "080442957X").await, vec![title.id]);
    assert_eq!(search_ids(&pool, "080442957x").await, vec![title.id]);
}

#[sqlx::test(migrations = "./migrations")]
async fn search_by_bad_checksum_isbn10_does_not_match(pool: MySqlPool) {
    TitleService::create_from_isbn(&pool, "9782205001211", None)
        .await
        .expect("create title");

    assert!(search_ids(&pool, "2205001214").await.is_empty());
}
