//! Story 13-2 (#583) — a throttled metadata fetch is retried.
//!
//! DB integration tests of the `metadata_fetch_retries` queue and of one
//! tick of the retry task (`tasks::metadata_retry::tick`), with stub
//! providers standing in for a saturated / answering / empty chain.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test metadata_retry

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use mybibli::metadata::provider::{MetadataError, MetadataProvider, MetadataResult};
use mybibli::metadata::registry::ProviderRegistry;
use mybibli::models::media_type::{CodeType, MediaType};
use mybibli::models::title::TitleModel;
use mybibli::tasks::metadata_retry::{TickStats, tick};
use sqlx::MySqlPool;

#[derive(Clone, Copy)]
enum Behaviour {
    /// HTTP 503 — the chain comes back throttled.
    Unavailable,
    /// The provider answers with nothing — a genuine miss.
    Empty,
    /// The provider answers with metadata.
    Found,
}

struct StubProvider {
    behaviour: Behaviour,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl MetadataProvider for StubProvider {
    fn name(&self) -> &str {
        "bnf"
    }
    fn supports_media_type(&self, _media_type: &MediaType) -> bool {
        true
    }
    async fn lookup_by_isbn(&self, _isbn: &str) -> Result<Option<MetadataResult>, MetadataError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.behaviour {
            Behaviour::Unavailable => Err(MetadataError::Unavailable),
            Behaviour::Empty => Ok(None),
            Behaviour::Found => Ok(Some(MetadataResult {
                title: Some("Resolved on retry".to_string()),
                publisher: Some("Retry Press".to_string()),
                // Unreachable on purpose: the cover download fails fast
                // instead of probing real cover services for the ISBN.
                cover_url: Some("http://127.0.0.1:1/cover.jpg".to_string()),
                ..MetadataResult::default()
            })),
        }
    }
}

fn state_with(pool: &MySqlPool, behaviour: Behaviour) -> (mybibli::AppState, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(StubProvider {
        behaviour,
        calls: calls.clone(),
    }));
    let mut state = common::mcp::state_with_pool(pool.clone());
    state.registry = Arc::new(registry);
    if let Ok(mut s) = state.settings.write() {
        s.bulk_refetch_delay_ms = 0;
        s.metadata_fetch_timeout_secs = 5;
    }
    (state, calls)
}

/// A scanned title: its placeholder title is its ISBN, as `/catalog/scan`
/// creates it.
async fn seed_scanned_title(pool: &MySqlPool, isbn: &str) -> u64 {
    sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id, isbn) \
         VALUES (?, 'fr', 'book', 1, ?)",
    )
    .bind(isbn)
    .bind(isbn)
    .execute(pool)
    .await
    .expect("insert title")
    .last_insert_id()
}

/// Put `title_id` on the queue, already due, with `attempts` retries run.
async fn seed_due_row(pool: &MySqlPool, title_id: u64, attempts: i32) {
    sqlx::query(
        "INSERT INTO metadata_fetch_retries (title_id, attempts, next_attempt_at) \
         VALUES (?, ?, NOW() - INTERVAL 1 MINUTE)",
    )
    .bind(title_id)
    .bind(attempts)
    .execute(pool)
    .await
    .expect("insert queue row");
}

/// (live rows, total rows) for the title.
async fn queue_rows(pool: &MySqlPool, title_id: u64) -> (i64, i64) {
    sqlx::query_as(
        "SELECT CAST(COALESCE(SUM(deleted_at IS NULL), 0) AS SIGNED), COUNT(*) \
         FROM metadata_fetch_retries WHERE title_id = ?",
    )
    .bind(title_id)
    .fetch_one(pool)
    .await
    .expect("count queue rows")
}

/// (attempts, next attempt is in the future) of the live row.
async fn live_row(pool: &MySqlPool, title_id: u64) -> (i32, bool) {
    let (attempts, in_future): (i32, i64) = sqlx::query_as(
        "SELECT attempts, CAST(next_attempt_at > NOW() AS SIGNED) \
         FROM metadata_fetch_retries WHERE title_id = ? AND deleted_at IS NULL",
    )
    .bind(title_id)
    .fetch_one(pool)
    .await
    .expect("live queue row");
    (attempts, in_future == 1)
}

async fn scan_fetch(state: &mybibli::AppState, title_id: u64, isbn: &str) {
    mybibli::tasks::metadata_fetch::fetch_metadata_chain(
        state.pool.clone(),
        title_id,
        isbn.to_string(),
        CodeType::Isbn,
        MediaType::Book,
        state.registry.clone(),
        5,
        state.metadata_chain_provider_timeouts(),
        state.http_client.clone(),
        std::env::temp_dir(),
        false,
    )
    .await;
}

// ─── AC1 / AC2 — enqueue ───────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_throttled_scan_fetch_enqueues_the_title_once(pool: MySqlPool) {
    let isbn = "9799999913201";
    let id = seed_scanned_title(&pool, isbn).await;
    let (state, _) = state_with(&pool, Behaviour::Unavailable);

    scan_fetch(&state, id, isbn).await;
    scan_fetch(&state, id, isbn).await;

    assert_eq!(
        queue_rows(&pool, id).await,
        (1, 1),
        "one live row, however often throttled"
    );
    let (attempts, in_future) = live_row(&pool, id).await;
    assert_eq!(attempts, 0);
    assert!(in_future, "the first retry waits for the first delay");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_genuine_miss_is_not_enqueued(pool: MySqlPool) {
    let isbn = "9799999913218";
    let id = seed_scanned_title(&pool, isbn).await;
    let (state, _) = state_with(&pool, Behaviour::Empty);

    scan_fetch(&state, id, isbn).await;

    assert_eq!(queue_rows(&pool, id).await, (0, 0));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_dequeued_title_throttled_again_is_queued_afresh(pool: MySqlPool) {
    let isbn = "9799999913225";
    let id = seed_scanned_title(&pool, isbn).await;
    seed_due_row(&pool, id, 3).await;
    sqlx::query("UPDATE metadata_fetch_retries SET deleted_at = NOW() WHERE title_id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let (state, _) = state_with(&pool, Behaviour::Unavailable);

    scan_fetch(&state, id, isbn).await;

    assert_eq!(
        queue_rows(&pool, id).await,
        (1, 1),
        "the old row is revived, not duplicated"
    );
    assert_eq!(live_row(&pool, id).await, (0, true), "fresh schedule");
}

// ─── AC3 / AC4 — the retry tick ────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_due_row_resolved_by_the_chain_is_dequeued_and_the_title_updated(pool: MySqlPool) {
    let isbn = "9799999913232";
    let id = seed_scanned_title(&pool, isbn).await;
    seed_due_row(&pool, id, 1).await;
    let (state, calls) = state_with(&pool, Behaviour::Found);

    let stats = tick(&state).await.expect("tick");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(stats.retried, 1);
    assert_eq!(stats.done, 1);
    assert_eq!(
        queue_rows(&pool, id).await,
        (0, 1),
        "dequeued (soft delete)"
    );
    let title = TitleModel::find_by_id(&pool, id).await.unwrap().unwrap();
    assert_eq!(title.title, "Resolved on retry");
    assert_eq!(title.publisher.as_deref(), Some("Retry Press"));
    assert_eq!(
        TitleModel::find_metadata_source(&pool, id)
            .await
            .unwrap()
            .as_deref(),
        Some("bnf"),
        "applied through the scan path, provenance included"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_retry_respects_manually_edited_fields(pool: MySqlPool) {
    let isbn = "9799999913249";
    let id = seed_scanned_title(&pool, isbn).await;
    sqlx::query(
        "UPDATE titles SET publisher = 'Typed by hand', \
         manually_edited_fields = '[\"publisher\"]' WHERE id = ?",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    seed_due_row(&pool, id, 0).await;
    let (state, _) = state_with(&pool, Behaviour::Found);

    tick(&state).await.expect("tick");

    let title = TitleModel::find_by_id(&pool, id).await.unwrap().unwrap();
    assert_eq!(title.title, "Resolved on retry");
    assert_eq!(title.publisher.as_deref(), Some("Typed by hand"));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_row_throttled_again_is_rescheduled(pool: MySqlPool) {
    let isbn = "9799999913256";
    let id = seed_scanned_title(&pool, isbn).await;
    seed_due_row(&pool, id, 1).await;
    let (state, _) = state_with(&pool, Behaviour::Unavailable);

    let stats = tick(&state).await.expect("tick");

    assert_eq!(stats.rescheduled, 1);
    assert_eq!(live_row(&pool, id).await, (2, true));
    // Not due any more: a second tick leaves it alone.
    assert_eq!(tick(&state).await.unwrap(), TickStats::default());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_row_past_its_last_attempt_is_dequeued(pool: MySqlPool) {
    let isbn = "9799999913263";
    let id = seed_scanned_title(&pool, isbn).await;
    seed_due_row(&pool, id, 4).await;
    let (state, _) = state_with(&pool, Behaviour::Unavailable);

    let stats = tick(&state).await.expect("tick");

    assert_eq!(stats.given_up, 1);
    assert_eq!(queue_rows(&pool, id).await, (0, 1));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_title_is_skipped_and_dequeued(pool: MySqlPool) {
    let isbn = "9799999913270";
    let id = seed_scanned_title(&pool, isbn).await;
    seed_due_row(&pool, id, 0).await;
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let (state, calls) = state_with(&pool, Behaviour::Found);

    let stats = tick(&state).await.expect("tick");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "no lookup for a deleted title"
    );
    assert_eq!(stats.skipped, 1);
    assert_eq!(queue_rows(&pool, id).await, (0, 1));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_title_already_resolved_once_is_still_retried(pool: MySqlPool) {
    // Review M2 — a throttled re-download / backfill of a title that has
    // metadata is retried too: the row means "latest lookup throttled".
    let isbn = "9799999913287";
    let id = seed_scanned_title(&pool, isbn).await;
    sqlx::query("UPDATE titles SET metadata_source = 'google_books' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    seed_due_row(&pool, id, 0).await;
    let (state, calls) = state_with(&pool, Behaviour::Found);

    let stats = tick(&state).await.expect("tick");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(stats.done, 1);
    assert_eq!(queue_rows(&pool, id).await, (0, 1));
}

#[sqlx::test(migrations = "./migrations")]
async fn rows_not_yet_due_are_left_alone(pool: MySqlPool) {
    let isbn = "9799999913294";
    let id = seed_scanned_title(&pool, isbn).await;
    mybibli::models::metadata_retry::MetadataRetryModel::enqueue(&pool, id, 900)
        .await
        .unwrap();
    let (state, calls) = state_with(&pool, Behaviour::Found);

    assert_eq!(tick(&state).await.unwrap(), TickStats::default());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(queue_rows(&pool, id).await, (1, 1));
}

// ─── Queue rule: every lookup that ends not throttled dequeues ─────

#[sqlx::test(migrations = "./migrations")]
async fn a_scan_fetch_that_finds_metadata_dequeues_the_title(pool: MySqlPool) {
    let isbn = "9799999913300";
    let id = seed_scanned_title(&pool, isbn).await;
    seed_due_row(&pool, id, 2).await;
    let (state, _) = state_with(&pool, Behaviour::Found);

    scan_fetch(&state, id, isbn).await;

    assert_eq!(queue_rows(&pool, id).await, (0, 1));
}

#[sqlx::test(migrations = "./migrations")]
async fn bulk_loop_outcome_enqueues_throttled_and_dequeues_the_rest(pool: MySqlPool) {
    // The bulk loop calls `note_fetch_outcome` once per title with its final
    // outcome (after its own in-run retries); its HTTP path waits minutes of
    // back-off, so the decision it calls is tested here.
    use mybibli::tasks::metadata_fetch::FetchOutcome;
    use mybibli::tasks::metadata_retry::note_fetch_outcome;
    let id = seed_scanned_title(&pool, "9799999913317").await;

    note_fetch_outcome(&pool, id, FetchOutcome::Throttled, "bulk").await;
    assert_eq!(queue_rows(&pool, id).await, (1, 1));
    note_fetch_outcome(&pool, id, FetchOutcome::NotFound, "bulk").await;
    assert_eq!(queue_rows(&pool, id).await, (0, 1));
}

// ─── Review M1 / L2 / L3 ───────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_failing_row_does_not_stop_the_batch(pool: MySqlPool) {
    // The oldest due row cannot be dequeued: its title is deleted (so the
    // task skips it) and a trigger makes the dequeue write fail.
    let bad = seed_scanned_title(&pool, "9799999913324").await;
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(bad)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(&format!(
        "CREATE TRIGGER fail_dequeue BEFORE UPDATE ON metadata_fetch_retries \
         FOR EACH ROW BEGIN \
           IF NEW.title_id = {bad} AND NEW.deleted_at IS NOT NULL THEN \
             SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'dequeue refused'; \
           END IF; \
         END"
    ))
    .execute(&pool)
    .await
    .expect("create trigger");
    sqlx::query(
        "INSERT INTO metadata_fetch_retries (title_id, attempts, next_attempt_at) \
         VALUES (?, 1, NOW() - INTERVAL 2 HOUR)",
    )
    .bind(bad)
    .execute(&pool)
    .await
    .unwrap();
    let good = seed_scanned_title(&pool, "9799999913331").await;
    seed_due_row(&pool, good, 0).await;
    let (state, calls) = state_with(&pool, Behaviour::Found);

    let stats = tick(&state).await.expect("tick survives a failing row");

    assert_eq!(stats.errors, 1);
    assert_eq!(stats.done, 1, "the next row is processed");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(queue_rows(&pool, good).await, (0, 1));
    assert_eq!(
        live_row(&pool, bad).await,
        (2, true),
        "the failing row is pushed back and burns an attempt"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn an_enqueue_landing_during_a_retry_is_not_erased(pool: MySqlPool) {
    use mybibli::models::metadata_retry::MetadataRetryModel;
    let id = seed_scanned_title(&pool, "9799999913348").await;
    seed_due_row(&pool, id, 1).await;
    let row = MetadataRetryModel::due_batch(&pool, 10).await.unwrap()[0];
    // A scan throttled meanwhile re-enqueues the (live) title.
    MetadataRetryModel::enqueue(&pool, id, 900).await.unwrap();

    assert!(!MetadataRetryModel::dequeue_read(&pool, &row).await.unwrap());
    assert!(
        !MetadataRetryModel::reschedule(&pool, &row, 2, 3_600)
            .await
            .unwrap()
    );
    assert_eq!(queue_rows(&pool, id).await, (1, 1), "still queued");
    assert_eq!(live_row(&pool, id).await.0, 1, "attempts untouched");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_tick_waits_while_a_bulk_run_is_in_progress(pool: MySqlPool) {
    let id = seed_scanned_title(&pool, "9799999913355").await;
    seed_due_row(&pool, id, 0).await;
    let (state, calls) = state_with(&pool, Behaviour::Found);
    mybibli::services::bulk_cover_fetch::try_start(&state.bulk_cover_fetch, 10).unwrap();

    let stats = tick(&state).await.expect("tick");

    assert!(stats.deferred_for_bulk_run);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(queue_rows(&pool, id).await, (1, 1));
}

// ─── Manual re-download (HTTP) ─────────────────────────────────────

async fn redownload(state: mybibli::AppState, pool: &MySqlPool, id: u64) -> axum::http::StatusCode {
    common::web::seed_admin(pool, "retry_admin").await;
    let router = mybibli::routes::build_router_with_mcp(state, mybibli::mcp::McpConfig::default());
    let browser = common::web::Browser::login(&router, "retry_admin").await;
    let (status, _) = browser
        .post_form(&router, &format!("/title/{id}/redownload"), "")
        .await;
    status
}

#[sqlx::test(migrations = "./migrations")]
async fn a_throttled_redownload_enqueues_the_title(pool: MySqlPool) {
    let id = seed_scanned_title(&pool, "9799999913362").await;
    let (state, _) = state_with(&pool, Behaviour::Unavailable);

    assert!(redownload(state, &pool, id).await.is_success());

    assert_eq!(queue_rows(&pool, id).await, (1, 1));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_redownload_that_applies_metadata_dequeues_the_title(pool: MySqlPool) {
    let id = seed_scanned_title(&pool, "9799999913379").await;
    seed_due_row(&pool, id, 1).await;
    let (state, _) = state_with(&pool, Behaviour::Found);

    assert!(redownload(state, &pool, id).await.is_success());

    let title = TitleModel::find_by_id(&pool, id).await.unwrap().unwrap();
    assert_eq!(title.title, "Resolved on retry", "applied first");
    assert_eq!(queue_rows(&pool, id).await, (0, 1));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_redownload_with_a_genuine_miss_dequeues_the_title(pool: MySqlPool) {
    let id = seed_scanned_title(&pool, "9799999913386").await;
    seed_due_row(&pool, id, 1).await;
    let (state, _) = state_with(&pool, Behaviour::Empty);

    assert!(redownload(state, &pool, id).await.is_success());

    assert_eq!(queue_rows(&pool, id).await, (0, 1));
}
