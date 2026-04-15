//! Integration tests for `TitleModel::find_similar` — story 5-7.
//!
//! These tests cover AC #13 (Task 5.1 + Task 5.2) of the Similar Titles story.
//! Each test gets a freshly provisioned database via `#[sqlx::test]`, with all
//! migrations applied (genres, contributor_roles, and media-type reference data
//! are seeded by the bootstrap migrations).
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test find_similar
//!
//! `#[sqlx::test]` uses the `DATABASE_URL` admin connection to create a unique
//! temporary database per test, runs migrations, and drops the DB on teardown.

use chrono::NaiveDate;
use mybibli::models::title::TitleModel;
use sqlx::MySqlPool;
use std::time::Instant;

// ─── Seeding helpers ───────────────────────────────────────────────────────

async fn create_title(
    pool: &MySqlPool,
    title: &str,
    genre_id: u64,
    media_type: &str,
    publication_date: Option<NaiveDate>,
) -> u64 {
    let result = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id, publication_date) \
         VALUES (?, 'fr', ?, ?, ?)",
    )
    .bind(title)
    .bind(media_type)
    .bind(genre_id)
    .bind(publication_date)
    .execute(pool)
    .await
    .expect("insert title");
    result.last_insert_id()
}

async fn soft_delete_title(pool: &MySqlPool, title_id: u64) {
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(title_id)
        .execute(pool)
        .await
        .expect("soft delete title");
}

async fn create_contributor(pool: &MySqlPool, name: &str) -> u64 {
    let result = sqlx::query("INSERT INTO contributors (name) VALUES (?)")
        .bind(name)
        .execute(pool)
        .await
        .expect("insert contributor");
    result.last_insert_id()
}

async fn link_contributor(pool: &MySqlPool, title_id: u64, contributor_id: u64, role_id: u64) {
    sqlx::query(
        "INSERT INTO title_contributors (title_id, contributor_id, role_id) VALUES (?, ?, ?)",
    )
    .bind(title_id)
    .bind(contributor_id)
    .bind(role_id)
    .execute(pool)
    .await
    .expect("link contributor");
}

async fn create_series(pool: &MySqlPool, name: &str) -> u64 {
    let result = sqlx::query(
        "INSERT INTO series (name, series_type, total_volume_count) VALUES (?, 'open', NULL)",
    )
    .bind(name)
    .execute(pool)
    .await
    .expect("insert series");
    result.last_insert_id()
}

async fn link_series(pool: &MySqlPool, title_id: u64, series_id: u64, position: i32) {
    sqlx::query("INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, ?)")
        .bind(title_id)
        .bind(series_id)
        .bind(position)
        .execute(pool)
        .await
        .expect("link series");
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

// ─── AC #13 Task 5.1 — algorithm coverage ─────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn test_series_only_match(pool: MySqlPool) {
    // Anchor shares a series with candidate A, and nothing else.
    let series_id = create_series(&pool, "Testing Series").await;
    let anchor = create_title(&pool, "Anchor", 1, "book", None).await;
    let cand_a = create_title(&pool, "Candidate A", 1, "book", None).await;
    let cand_unrelated = create_title(&pool, "Unrelated", 1, "book", None).await;

    link_series(&pool, anchor, series_id, 1).await;
    link_series(&pool, cand_a, series_id, 2).await;

    let results = TitleModel::find_similar(&pool, anchor).await.unwrap();
    let ids: Vec<u64> = results.iter().map(|s| s.id).collect();

    assert_eq!(ids, vec![cand_a], "expected only series match, got {ids:?}");
    assert_eq!(results[0].priority, 1, "priority must be 1 (series)");
    assert!(!ids.contains(&cand_unrelated));
}

#[sqlx::test(migrations = "./migrations")]
async fn test_contributor_only_match(pool: MySqlPool) {
    // Anchor and candidate B share a contributor, no series, no date.
    let contributor = create_contributor(&pool, "Jean Tester").await;
    let anchor = create_title(&pool, "Anchor", 1, "book", None).await;
    let cand_b = create_title(&pool, "Candidate B", 1, "book", None).await;
    let _unrelated = create_title(&pool, "Unrelated", 1, "book", None).await;

    // Role 1 = Auteur per seed
    link_contributor(&pool, anchor, contributor, 1).await;
    link_contributor(&pool, cand_b, contributor, 1).await;

    let results = TitleModel::find_similar(&pool, anchor).await.unwrap();
    let ids: Vec<u64> = results.iter().map(|s| s.id).collect();

    assert_eq!(ids, vec![cand_b]);
    assert_eq!(results[0].priority, 2, "priority must be 2 (contributor)");
    assert_eq!(
        results[0].primary_contributor.as_deref(),
        Some("Jean Tester"),
        "primary_contributor must be attached via correlated subquery"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn test_genre_decade_only_match(pool: MySqlPool) {
    // Anchor and candidate share genre_id=1 and decade 1950–1959.
    let anchor = create_title(&pool, "Anchor 1957", 1, "book", Some(date(1957, 6, 19))).await;
    let cand_c = create_title(&pool, "Candidate 1952", 1, "book", Some(date(1952, 3, 1))).await;
    // Different decade — must NOT match
    let out_of_decade =
        create_title(&pool, "Candidate 1969", 1, "book", Some(date(1969, 3, 1))).await;
    // Different genre — must NOT match
    let wrong_genre = create_title(
        &pool,
        "Candidate 1957 BD",
        2,
        "book",
        Some(date(1957, 3, 1)),
    )
    .await;

    let results = TitleModel::find_similar(&pool, anchor).await.unwrap();
    let ids: Vec<u64> = results.iter().map(|s| s.id).collect();

    assert_eq!(ids, vec![cand_c], "only same genre+decade matches");
    assert_eq!(results[0].priority, 3);
    assert!(!ids.contains(&out_of_decade));
    assert!(!ids.contains(&wrong_genre));
}

#[sqlx::test(migrations = "./migrations")]
async fn test_dedup_series_beats_contributor(pool: MySqlPool) {
    // Candidate matches via BOTH series (priority 1) and contributor (priority 2)
    // → must appear ONCE with priority 1.
    let series_id = create_series(&pool, "Series").await;
    let contributor = create_contributor(&pool, "Shared Author").await;

    let anchor = create_title(&pool, "Anchor", 1, "book", None).await;
    let dual_match = create_title(&pool, "Dual Match", 1, "book", None).await;

    link_series(&pool, anchor, series_id, 1).await;
    link_series(&pool, dual_match, series_id, 2).await;
    link_contributor(&pool, anchor, contributor, 1).await;
    link_contributor(&pool, dual_match, contributor, 1).await;

    let results = TitleModel::find_similar(&pool, anchor).await.unwrap();
    let ids: Vec<u64> = results.iter().map(|s| s.id).collect();

    assert_eq!(
        ids,
        vec![dual_match],
        "dedup: candidate appears exactly once"
    );
    assert_eq!(
        results[0].priority, 1,
        "multi-match must collapse to highest priority (series)"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn test_dedup_contributor_beats_decade(pool: MySqlPool) {
    // Candidate matches via contributor (priority 2) and genre+decade (priority 3)
    // → must appear ONCE with priority 2.
    let contributor = create_contributor(&pool, "Shared Author").await;

    let anchor = create_title(&pool, "Anchor", 1, "book", Some(date(2020, 1, 1))).await;
    let dual_match = create_title(&pool, "Dual Match", 1, "book", Some(date(2024, 1, 1))).await;

    link_contributor(&pool, anchor, contributor, 1).await;
    link_contributor(&pool, dual_match, contributor, 1).await;

    let results = TitleModel::find_similar(&pool, anchor).await.unwrap();
    let hit = results
        .iter()
        .find(|s| s.id == dual_match)
        .expect("dual_match must appear");

    let dual_count = results.iter().filter(|s| s.id == dual_match).count();
    assert_eq!(dual_count, 1, "dedup: appears once");
    assert_eq!(hit.priority, 2, "contributor (2) wins over decade (3)");
}

#[sqlx::test(migrations = "./migrations")]
async fn test_year_less_candidate_excluded_from_decade_only(pool: MySqlPool) {
    // A candidate with publication_date = NULL must NOT match via arm 3,
    // but CAN still match via arm 2 (contributor).
    let contributor = create_contributor(&pool, "Shared Author").await;
    let anchor = create_title(&pool, "Anchor", 1, "book", Some(date(2024, 1, 1))).await;
    // Year-less candidate with shared contributor — matches via arm 2, not arm 3
    let via_contrib = create_title(&pool, "Year-less shared", 1, "book", None).await;
    // Year-less candidate with no shared criterion — must not match
    let via_nothing = create_title(&pool, "Year-less stray", 1, "book", None).await;

    link_contributor(&pool, anchor, contributor, 1).await;
    link_contributor(&pool, via_contrib, contributor, 1).await;

    let results = TitleModel::find_similar(&pool, anchor).await.unwrap();
    let ids: Vec<u64> = results.iter().map(|s| s.id).collect();

    assert!(
        ids.contains(&via_contrib),
        "arm 2 still matches year-less titles"
    );
    assert!(
        !ids.contains(&via_nothing),
        "year-less stray must not appear (arm 3 filters IS NOT NULL)"
    );
    let hit = results.iter().find(|s| s.id == via_contrib).unwrap();
    assert_eq!(
        hit.priority, 2,
        "year-less match comes from contributor arm, not decade"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn test_current_title_not_self_match(pool: MySqlPool) {
    // Anchor has a series, a contributor, and a date that ALL match itself.
    // It must NEVER appear in its own results.
    let series_id = create_series(&pool, "Solo Series").await;
    let contributor = create_contributor(&pool, "Solo Author").await;

    let anchor = create_title(&pool, "Anchor", 1, "book", Some(date(2024, 1, 1))).await;
    link_series(&pool, anchor, series_id, 1).await;
    link_contributor(&pool, anchor, contributor, 1).await;

    let results = TitleModel::find_similar(&pool, anchor).await.unwrap();
    let ids: Vec<u64> = results.iter().map(|s| s.id).collect();

    assert!(!ids.contains(&anchor), "self-exclusion (AC #5)");
}

#[sqlx::test(migrations = "./migrations")]
async fn test_soft_deleted_candidates_excluded(pool: MySqlPool) {
    // A candidate matching via all 3 arms must disappear once soft-deleted.
    let series_id = create_series(&pool, "Series").await;
    let contributor = create_contributor(&pool, "Author").await;

    let anchor = create_title(&pool, "Anchor", 1, "book", Some(date(2024, 1, 1))).await;
    let cand_live = create_title(&pool, "Live Candidate", 1, "book", Some(date(2024, 6, 1))).await;
    let cand_dead = create_title(&pool, "Dead Candidate", 1, "book", Some(date(2024, 7, 1))).await;

    link_series(&pool, anchor, series_id, 1).await;
    link_series(&pool, cand_live, series_id, 2).await;
    link_series(&pool, cand_dead, series_id, 3).await;
    link_contributor(&pool, anchor, contributor, 1).await;
    link_contributor(&pool, cand_live, contributor, 1).await;
    link_contributor(&pool, cand_dead, contributor, 1).await;

    soft_delete_title(&pool, cand_dead).await;

    let results = TitleModel::find_similar(&pool, anchor).await.unwrap();
    let ids: Vec<u64> = results.iter().map(|s| s.id).collect();

    assert!(ids.contains(&cand_live));
    assert!(
        !ids.contains(&cand_dead),
        "soft-deleted candidates must not appear"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn test_limit_8_and_ordering(pool: MySqlPool) {
    // Seed >8 candidates across multiple priority buckets and verify:
    //   1. exactly 8 are returned
    //   2. order is priority ASC, id ASC (stable)
    //   3. series-priority titles appear before contributor-priority titles
    let series_id = create_series(&pool, "Big Series").await;
    let contributor = create_contributor(&pool, "Big Author").await;

    let anchor = create_title(&pool, "Anchor", 1, "book", Some(date(2024, 1, 1))).await;
    link_series(&pool, anchor, series_id, 1).await;
    link_contributor(&pool, anchor, contributor, 1).await;

    // 3 series-matching candidates (priority 1)
    let mut series_ids: Vec<u64> = Vec::new();
    for i in 2i32..=4 {
        let id = create_title(&pool, &format!("Series Cand {i}"), 1, "book", None).await;
        link_series(&pool, id, series_id, i).await;
        series_ids.push(id);
    }

    // 10 contributor-matching candidates (priority 2)
    let mut contrib_ids: Vec<u64> = Vec::new();
    for i in 0..10 {
        let id = create_title(&pool, &format!("Contrib Cand {i}"), 1, "book", None).await;
        link_contributor(&pool, id, contributor, 1).await;
        contrib_ids.push(id);
    }

    let results = TitleModel::find_similar(&pool, anchor).await.unwrap();
    assert_eq!(results.len(), 8, "LIMIT 8 must cap the result set");

    // First 3 are series (priority 1), remaining 5 are contributor (priority 2).
    // Within each bucket the inner arm LIMIT 16 plus outer ORDER BY id ASC
    // guarantees the lowest IDs first.
    assert_eq!(
        &results[0..3].iter().map(|s| s.id).collect::<Vec<_>>(),
        &series_ids
    );
    for s in &results[0..3] {
        assert_eq!(s.priority, 1);
    }
    assert_eq!(
        &results[3..8].iter().map(|s| s.id).collect::<Vec<_>>(),
        &contrib_ids[0..5]
    );
    for s in &results[3..8] {
        assert_eq!(s.priority, 2);
    }

    // Verify strict ordering within bucket
    for w in results.windows(2) {
        if w[0].priority == w[1].priority {
            assert!(w[0].id < w[1].id, "id ASC within same priority bucket");
        } else {
            assert!(w[0].priority < w[1].priority, "priority ASC across buckets");
        }
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn test_empty_early_return_no_criteria(pool: MySqlPool) {
    // Anchor with no series, no contributor, no publication_date → empty result.
    let anchor = create_title(&pool, "Lonely Anchor", 1, "book", None).await;
    // Seed some unrelated data to prove arms don't accidentally match.
    let _noise = create_title(&pool, "Noise 1", 1, "book", Some(date(2024, 1, 1))).await;

    let results = TitleModel::find_similar(&pool, anchor).await.unwrap();
    assert!(
        results.is_empty(),
        "no criteria = no results (early return)"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn test_nonexistent_anchor_returns_empty(pool: MySqlPool) {
    // Defensive: if the anchor title_id doesn't exist, return empty (no panic,
    // no 500). This guards the route's title_detail 404 path if called out-of-order.
    let results = TitleModel::find_similar(&pool, 99_999_999).await.unwrap();
    assert!(results.is_empty());
}

// ─── AC #13 Task 5.2 — perf smoke test ─────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn test_perf_smoke_50_titles(pool: MySqlPool) {
    // Seed 50 titles sharing one contributor across 3 genres and 2 decades,
    // then call find_similar on one of them. Soft gate: < 50 ms.
    // This catches obvious regressions; the FR114 < 200 ms target at 10k
    // is validated informally.
    let contributor = create_contributor(&pool, "Prolific Author").await;

    let mut anchor_id: Option<u64> = None;
    for i in 0..50 {
        let genre_id = ((i % 3) + 1) as u64; // 1..=3
        let year = if i % 2 == 0 { 2020 } else { 2015 };
        let id = create_title(
            &pool,
            &format!("Perf Title {i}"),
            genre_id,
            "book",
            Some(date(year, 1, (i % 28 + 1) as u32)),
        )
        .await;
        link_contributor(&pool, id, contributor, 1).await;
        if anchor_id.is_none() {
            anchor_id = Some(id);
        }
    }

    let anchor = anchor_id.unwrap();

    // Warm-up call (first query pays the query plan / cache cost).
    let _ = TitleModel::find_similar(&pool, anchor).await.unwrap();

    let start = Instant::now();
    let results = TitleModel::find_similar(&pool, anchor).await.unwrap();
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 8, "LIMIT 8 cap applies");
    assert!(
        elapsed.as_millis() < 50,
        "perf smoke gate: find_similar took {} ms (expected < 50 ms)",
        elapsed.as_millis()
    );
}
