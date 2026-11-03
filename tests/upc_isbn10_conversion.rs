//! Story 13-3 (#584) — titles catalogued before v1.21.0 with an ISBN-10
//! stored as UPC are converted to their ISBN-13 at startup.
//!
//! DB integration tests of the data step
//! (`services::title::convert_isbn10_upc_titles`) on its own; the
//! background re-fetch is spawned by `main.rs` from the returned list.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test upc_isbn10_conversion

mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mybibli::metadata::provider::{MetadataError, MetadataProvider, MetadataResult};
use mybibli::metadata::registry::ProviderRegistry;
use mybibli::models::media_type::MediaType;
use mybibli::models::title::TitleModel;
use mybibli::services::title::{ConvertedTitle, convert_isbn10_upc_titles};
use sqlx::MySqlPool;

/// Records the ISBNs it is asked for; answers with metadata, or 503.
struct StubProvider {
    throttled: bool,
    asked: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl MetadataProvider for StubProvider {
    fn name(&self) -> &str {
        "bnf"
    }
    fn supports_media_type(&self, _media_type: &MediaType) -> bool {
        true
    }
    async fn lookup_by_isbn(&self, isbn: &str) -> Result<Option<MetadataResult>, MetadataError> {
        self.asked.lock().unwrap().push(isbn.to_string());
        if self.throttled {
            return Err(MetadataError::Unavailable);
        }
        Ok(Some(MetadataResult {
            title: Some("Found under its ISBN".to_string()),
            ..MetadataResult::default()
        }))
    }
}

fn state_with(pool: &MySqlPool, throttled: bool) -> (mybibli::AppState, Arc<Mutex<Vec<String>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(StubProvider {
        throttled,
        asked: asked.clone(),
    }));
    let mut state = common::mcp::state_with_pool(pool.clone());
    state.registry = Arc::new(registry);
    if let Ok(mut s) = state.settings.write() {
        s.bulk_refetch_delay_ms = 0;
        s.metadata_fetch_timeout_secs = 5;
    }
    (state, asked)
}

/// A live title carrying the given identifiers, as a pre-v1.21.0 scan
/// left it.
async fn title_with(pool: &MySqlPool, name: &str, isbn: Option<&str>, upc: Option<&str>) -> u64 {
    let genre_id: u64 =
        sqlx::query_scalar("SELECT id FROM genres WHERE deleted_at IS NULL LIMIT 1")
            .fetch_one(pool)
            .await
            .expect("a seeded genre");
    sqlx::query(
        "INSERT INTO titles (title, media_type, genre_id, language, isbn, upc) \
         VALUES (?, 'bd', ?, 'fr', ?, ?)",
    )
    .bind(name)
    .bind(genre_id)
    .bind(isbn)
    .bind(upc)
    .execute(pool)
    .await
    .expect("insert title")
    .last_insert_id()
}

/// `(isbn, upc, version, manually_edited_fields)` of a title, deleted or not.
async fn identifiers(
    pool: &MySqlPool,
    id: u64,
) -> (Option<String>, Option<String>, i32, Option<String>) {
    sqlx::query_as(
        "SELECT isbn, upc, version, CAST(manually_edited_fields AS CHAR) FROM titles WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("read title")
}

fn converted_ids(converted: &[ConvertedTitle]) -> Vec<u64> {
    converted.iter().map(|c| c.title_id).collect()
}

#[sqlx::test(migrations = "./migrations")]
async fn a_valid_isbn10_upc_becomes_the_isbn13(pool: MySqlPool) {
    let id = title_with(&pool, "Title 271", None, Some("2205001213")).await;
    let x = title_with(&pool, "X check digit", None, Some("080442957X")).await;
    let (_, _, version_before, edited_before) = identifiers(&pool, id).await;

    let report = convert_isbn10_upc_titles(&pool).await.expect("convert");

    assert!(report.converted.contains(&ConvertedTitle {
        title_id: id,
        isbn13: "9782205001211".to_string(),
        media_type: "bd".to_string(),
    }));
    assert!(converted_ids(&report.converted).contains(&x));
    let (isbn, upc, version, edited) = identifiers(&pool, id).await;
    assert_eq!(isbn.as_deref(), Some("9782205001211"));
    assert_eq!(upc, None);
    assert_eq!(
        version,
        version_before + 1,
        "optimistic locking bumps the version"
    );
    // AC5 — the conversion is not a hand edit.
    assert_eq!(edited, edited_before);
    assert_eq!(
        identifiers(&pool, x).await.0.as_deref(),
        Some("9780804429573")
    );
    // The scan path now finds it under its ISBN.
    let found = TitleModel::find_by_isbn(&pool, "9782205001211")
        .await
        .expect("find")
        .expect("title found by ISBN");
    assert_eq!(found.id, id);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_collision_leaves_the_title_unchanged(pool: MySqlPool) {
    let holder = title_with(&pool, "Already scanned", Some("9782205011500"), None).await;
    let id = title_with(&pool, "Title 282", None, Some("2205011502")).await;
    let before = identifiers(&pool, id).await;

    let report = convert_isbn10_upc_titles(&pool).await.expect("convert");

    assert!(!converted_ids(&report.converted).contains(&id));
    assert!(report.collisions >= 1);
    assert_eq!(
        identifiers(&pool, id).await,
        before,
        "left unchanged (no merge, #591)"
    );
    assert_eq!(
        identifiers(&pool, holder).await.0.as_deref(),
        Some("9782205011500")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_holder_is_no_collision(pool: MySqlPool) {
    let holder = title_with(&pool, "Deleted holder", Some("9782862270166"), None).await;
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(holder)
        .execute(&pool)
        .await
        .expect("soft-delete");
    let id = title_with(&pool, "Title 99", None, Some("2862270164")).await;

    let report = convert_isbn10_upc_titles(&pool).await.expect("convert");

    assert!(converted_ids(&report.converted).contains(&id));
    assert_eq!(
        identifiers(&pool, id).await.0.as_deref(),
        Some("9782862270166")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn two_titles_with_the_same_isbn10_convert_only_the_first(pool: MySqlPool) {
    let first = title_with(&pool, "First copy", None, Some("2604000288")).await;
    let second = title_with(&pool, "Second copy", None, Some("2604000288")).await;

    let report = convert_isbn10_upc_titles(&pool).await.expect("convert");

    let ids = converted_ids(&report.converted);
    assert!(ids.contains(&first));
    assert!(!ids.contains(&second));
    assert_eq!(
        identifiers(&pool, second).await.1.as_deref(),
        Some("2604000288")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn other_titles_are_left_alone(pool: MySqlPool) {
    // Title 478: ten digits failing the ISBN-10 checksum.
    let bad_checksum = title_with(&pool, "Title 478", None, Some("2205025062")).await;
    // A real UPC-A, and a short code.
    let upc_a = title_with(&pool, "A DVD", None, Some("036000291452")).await;
    let short = title_with(&pool, "Short", None, Some("12345678")).await;
    // Already has an ISBN: its UPC stays as it is.
    let has_isbn = title_with(&pool, "Has ISBN", Some("9782070360246"), Some("2205001213")).await;
    // Deleted.
    let deleted = title_with(&pool, "Deleted", None, Some("2205011502")).await;
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(deleted)
        .execute(&pool)
        .await
        .expect("soft-delete");
    let ids = [bad_checksum, upc_a, short, has_isbn, deleted];
    let mut before = Vec::new();
    for id in ids {
        before.push(identifiers(&pool, id).await);
    }

    let report = convert_isbn10_upc_titles(&pool).await.expect("convert");

    let converted = converted_ids(&report.converted);
    for (id, before) in ids.iter().zip(before) {
        assert!(!converted.contains(id), "title {id} must not be converted");
        assert_eq!(
            identifiers(&pool, *id).await,
            before,
            "title {id} untouched"
        );
    }
    assert!(
        report.not_isbn10 >= 1,
        "the bad checksum is counted as skipped"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_second_run_converts_nothing(pool: MySqlPool) {
    let id = title_with(&pool, "Title 174", None, Some("2604000288")).await;
    title_with(&pool, "Title 478", None, Some("2205025062")).await;

    let first = convert_isbn10_upc_titles(&pool).await.expect("first run");
    assert!(converted_ids(&first.converted).contains(&id));
    let after_first = identifiers(&pool, id).await;

    let second = convert_isbn10_upc_titles(&pool).await.expect("second run");
    assert!(second.converted.is_empty(), "nothing matches any more");
    assert_eq!(second.collisions, 0);
    assert_eq!(identifiers(&pool, id).await, after_first);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_stale_version_is_not_overwritten(pool: MySqlPool) {
    let id = title_with(&pool, "Edited meanwhile", None, Some("2205001213")).await;
    let (_, _, version, _) = identifiers(&pool, id).await;

    assert!(
        !TitleModel::convert_upc_to_isbn(&pool, id, version - 1, "2205001213", "9782205001211")
            .await
            .expect("update")
    );
    assert!(
        !TitleModel::convert_upc_to_isbn(&pool, id, version, "2205011502", "9782205011500")
            .await
            .expect("update"),
        "the UPC changed meanwhile"
    );
    assert_eq!(
        identifiers(&pool, id).await.1.as_deref(),
        Some("2205001213")
    );
    assert!(
        TitleModel::convert_upc_to_isbn(&pool, id, version, "2205001213", "9782205001211")
            .await
            .expect("update")
    );
}

/// The re-fetch the caller spawns looks each converted title up by its
/// ISBN-13, in order, through the scan wrapper.
#[sqlx::test(migrations = "./migrations")]
async fn converted_titles_are_looked_up_again_by_isbn(pool: MySqlPool) {
    let first = title_with(&pool, "2205001213", None, Some("2205001213")).await;
    let second = title_with(&pool, "2604000288", None, Some("2604000288")).await;
    let report = convert_isbn10_upc_titles(&pool).await.expect("convert");
    let (state, asked) = state_with(&pool, false);

    mybibli::tasks::isbn10_upc_refetch::run(&state, report.converted).await;

    assert_eq!(
        *asked.lock().unwrap(),
        vec!["9782205001211".to_string(), "9782604000280".to_string()]
    );
    for id in [first, second] {
        let title = TitleModel::find_by_id(&pool, id)
            .await
            .expect("find")
            .expect("live title");
        assert_eq!(title.title, "Found under its ISBN");
    }
}

/// A throttled re-fetch lands in the story 13-2 retry queue.
#[sqlx::test(migrations = "./migrations")]
async fn a_throttled_refetch_is_queued_for_retry(pool: MySqlPool) {
    let id = title_with(&pool, "2205011502", None, Some("2205011502")).await;
    let report = convert_isbn10_upc_titles(&pool).await.expect("convert");
    let (state, _) = state_with(&pool, true);

    mybibli::tasks::isbn10_upc_refetch::run(&state, report.converted).await;

    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM metadata_fetch_retries WHERE title_id = ? AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("count queue rows");
    assert_eq!(live, 1);
}

/// AC5 — a field edited by hand survives the re-fetch.
#[sqlx::test(migrations = "./migrations")]
async fn the_refetch_respects_hand_edited_fields(pool: MySqlPool) {
    let id = title_with(&pool, "My own title", None, Some("2862270164")).await;
    sqlx::query("UPDATE titles SET manually_edited_fields = '[\"title\"]' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .expect("mark title hand-edited");
    let report = convert_isbn10_upc_titles(&pool).await.expect("convert");
    let (state, asked) = state_with(&pool, false);

    mybibli::tasks::isbn10_upc_refetch::run(&state, report.converted).await;

    assert_eq!(*asked.lock().unwrap(), vec!["9782862270166".to_string()]);
    let title = TitleModel::find_by_id(&pool, id)
        .await
        .expect("find")
        .expect("live title");
    assert_eq!(title.title, "My own title");
}
