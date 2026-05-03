//! DB-backed tests for `series::count_with_gaps` and
//! `series::list_with_gaps` (story 9-6).
//!
//! Locks in:
//! - **AC7 / AC12a:** open series + closed-with-NULL/zero-total are
//!   NEVER counted as having gaps; only closed series with positive
//!   total AND `total > distinct_filled_positions` qualify.
//! - **AC8 / AC12a:** `count_with_gaps` uses `COUNT(DISTINCT
//!   position_number)` to (a) collapse same-position-different-titles
//!   data-error rows AND (b) correctly count BD omnibus titles whose
//!   multiple `title_series` rows for the same `title_id` populate
//!   distinct positions.
//! - **AC8 / AC12b:** `list_with_gaps` returns rows ordered by gap_count
//!   DESC then name ASC, honors LIMIT, projects the four fields the row
//!   template needs (id, name, total_volume_count, owned_count).
//! - Soft-delete invariants for both series and assignments.
//!
//! To run locally:
//!
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test dashboard_gaps

use mybibli::models::series::SeriesModel;
use sqlx::MySqlPool;

// ─── Fixture helpers ────────────────────────────────────────────────

async fn first_genre_id(pool: &MySqlPool) -> u64 {
    sqlx::query_scalar::<_, u64>(
        "SELECT id FROM genres WHERE deleted_at IS NULL ORDER BY id LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .expect("bootstrap migration must seed at least one genre")
}

async fn insert_title(pool: &MySqlPool, title: &str, genre_id: u64) -> u64 {
    let r = sqlx::query(
        "INSERT INTO titles (title, language, media_type, genre_id) VALUES (?, 'fr', 'book', ?)",
    )
    .bind(title)
    .bind(genre_id)
    .execute(pool)
    .await
    .expect("insert title");
    r.last_insert_id()
}

/// `series_type` is the literal `'open'` or `'closed'` matching the
/// ENUM column. `total` is `Option<i32>`: `None` writes SQL NULL.
async fn insert_series(
    pool: &MySqlPool,
    name: &str,
    series_type: &str,
    total: Option<i32>,
) -> u64 {
    let r = sqlx::query("INSERT INTO series (name, series_type, total_volume_count) VALUES (?, ?, ?)")
        .bind(name)
        .bind(series_type)
        .bind(total)
        .execute(pool)
        .await
        .expect("insert series");
    r.last_insert_id()
}

async fn insert_title_series_assignment(
    pool: &MySqlPool,
    title_id: u64,
    series_id: u64,
    position: i32,
    is_omnibus: bool,
) -> u64 {
    let r = sqlx::query(
        "INSERT INTO title_series (title_id, series_id, position_number, is_omnibus) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(title_id)
    .bind(series_id)
    .bind(position)
    .bind(is_omnibus)
    .execute(pool)
    .await
    .expect("insert title_series");
    r.last_insert_id()
}

async fn soft_delete_series(pool: &MySqlPool, series_id: u64) {
    sqlx::query("UPDATE series SET deleted_at = NOW() WHERE id = ?")
        .bind(series_id)
        .execute(pool)
        .await
        .expect("soft delete series");
}

async fn soft_delete_title_series_assignment(pool: &MySqlPool, assignment_id: u64) {
    sqlx::query("UPDATE title_series SET deleted_at = NOW() WHERE id = ?")
        .bind(assignment_id)
        .execute(pool)
        .await
        .expect("soft delete title_series");
}

async fn soft_delete_title(pool: &MySqlPool, title_id: u64) {
    sqlx::query("UPDATE titles SET deleted_at = NOW() WHERE id = ?")
        .bind(title_id)
        .execute(pool)
        .await
        .expect("soft delete title");
}

async fn insert_title_series_assignment_for_title(
    pool: &MySqlPool,
    title_id: u64,
    series_id: u64,
    position: i32,
) -> u64 {
    insert_title_series_assignment(pool, title_id, series_id, position, false).await
}

/// Insert a fresh title and assign it to `series_id` at `position`.
/// Returns the assignment id (so callers can soft-delete it).
async fn insert_filled_position(
    pool: &MySqlPool,
    series_id: u64,
    position: i32,
    seq: u32,
) -> u64 {
    let g = first_genre_id(pool).await;
    let t = insert_title(pool, &format!("Z-9-6-Title-{seq:04}-pos{position}"), g).await;
    insert_title_series_assignment(pool, t, series_id, position, false).await
}

// ─── AC12a — count_with_gaps ────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_on_empty_db_returns_zero(pool: MySqlPool) {
    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(n, 0, "fresh DB must have zero gappy series");
}

/// AC7: open series have no defined "completeness" — they are NEVER
/// counted as having gaps regardless of how many positions are filled.
#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_open_series_never_counted(pool: MySqlPool) {
    insert_series(&pool, "Open A", "open", None).await;
    insert_series(&pool, "Open B", "open", None).await;
    insert_series(&pool, "Open C", "open", None).await;

    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(n, 0, "open series are excluded by series_type filter");
}

/// AC7: a closed series without `total_volume_count` is a data-integrity
/// edge case (likely user-in-progress) — exclude.
#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_closed_with_null_total_excluded(pool: MySqlPool) {
    insert_series(&pool, "Closed-NoTotal", "closed", None).await;

    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(n, 0, "closed series with NULL total is excluded");
}

/// AC7: a closed series with `total = 0` has no defined slots — exclude.
#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_closed_with_zero_total_excluded(pool: MySqlPool) {
    insert_series(&pool, "Closed-ZeroTotal", "closed", Some(0)).await;

    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(n, 0, "closed series with total=0 is excluded");
}

/// AC7: a fully-filled closed series has no gaps.
#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_closed_full_not_counted(pool: MySqlPool) {
    let s = insert_series(&pool, "Closed-Full", "closed", Some(5)).await;
    for pos in 1..=5 {
        insert_filled_position(&pool, s, pos, pos as u32).await;
    }

    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(n, 0, "5/5 filled positions = no gap");
}

/// AC7: a partially-filled closed series IS counted.
#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_closed_partial_counted(pool: MySqlPool) {
    let s = insert_series(&pool, "Closed-Partial", "closed", Some(5)).await;
    insert_filled_position(&pool, s, 1, 1).await;
    insert_filled_position(&pool, s, 2, 2).await;
    insert_filled_position(&pool, s, 4, 4).await;

    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(n, 1, "3/5 filled positions = gappy");
}

#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_excludes_soft_deleted_series(pool: MySqlPool) {
    let s_alive = insert_series(&pool, "Alive", "closed", Some(5)).await;
    insert_filled_position(&pool, s_alive, 1, 1).await;

    let s_dead = insert_series(&pool, "Dead", "closed", Some(5)).await;
    insert_filled_position(&pool, s_dead, 1, 2).await;
    soft_delete_series(&pool, s_dead).await;

    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(n, 1, "soft-deleted series is excluded");
}

/// AC8: a soft-deleted assignment unfills the position.
#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_soft_deleted_assignments_dont_fill_gaps(pool: MySqlPool) {
    let s = insert_series(&pool, "Closed-WithSoftDeletes", "closed", Some(5)).await;
    let _a1 = insert_filled_position(&pool, s, 1, 1).await;
    let _a2 = insert_filled_position(&pool, s, 2, 2).await;
    let a3 = insert_filled_position(&pool, s, 3, 3).await;
    let a4 = insert_filled_position(&pool, s, 4, 4).await;
    let _a5 = insert_filled_position(&pool, s, 5, 5).await;

    // Distinct filled positions = 5 → no gap. Then soft-delete two:
    soft_delete_title_series_assignment(&pool, a3).await;
    soft_delete_title_series_assignment(&pool, a4).await;
    // Distinct filled positions (active only) = 3 → gappy.

    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(
        n, 1,
        "after soft-deleting positions 3+4, distinct filled = 3 < 5 = gappy"
    );
}

/// AC8: `COUNT(DISTINCT position_number)` collapses
/// same-position-different-title rows. The UNIQUE constraint at
/// `(title_id, series_id, position_number)` allows two distinct
/// `title_id` values to share `(series_id, position_number)` — a
/// data-error edge case the SQL must handle without double-counting.
#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_distinct_positions(pool: MySqlPool) {
    let s = insert_series(&pool, "Closed-WithDuplicate", "closed", Some(5)).await;
    let g = first_genre_id(&pool).await;

    // Two different titles both at position 1 (allowed by UNIQUE
    // constraint since title_id differs). DISTINCT must collapse them.
    let t_a = insert_title(&pool, "Title A pos 1", g).await;
    let t_b = insert_title(&pool, "Title B pos 1 (dup)", g).await;
    insert_title_series_assignment(&pool, t_a, s, 1, false).await;
    insert_title_series_assignment(&pool, t_b, s, 1, false).await;

    insert_filled_position(&pool, s, 2, 2).await;
    insert_filled_position(&pool, s, 3, 3).await;
    insert_filled_position(&pool, s, 4, 4).await;
    insert_filled_position(&pool, s, 5, 5).await;

    // 6 raw rows but COUNT(DISTINCT position_number) = 5 = total → not gappy.
    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(
        n, 0,
        "DISTINCT collapses pos 1 duplicate; 5 distinct positions = full"
    );
}

/// AC7: BD omnibus = one title (single `title_id`) covers multiple
/// distinct positions via separate `title_series` rows. Each row
/// contributes a distinct `position_number` to the COUNT, so an omnibus
/// covering positions 1-3 fills 3 slots.
#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_omnibus_fills_each_position(pool: MySqlPool) {
    let s = insert_series(&pool, "Closed-WithOmnibus", "closed", Some(5)).await;
    let g = first_genre_id(&pool).await;

    let omnibus_title = insert_title(&pool, "Omnibus Vol 1-3", g).await;
    insert_title_series_assignment(&pool, omnibus_title, s, 1, true).await;
    insert_title_series_assignment(&pool, omnibus_title, s, 2, true).await;
    insert_title_series_assignment(&pool, omnibus_title, s, 3, true).await;

    insert_filled_position(&pool, s, 5, 5).await;

    // Distinct positions: 1, 2, 3, 5 = 4. Total = 5. 5 > 4 → gappy.
    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(
        n, 1,
        "omnibus fills 3 distinct positions, 1 single = 4/5 total → gap at pos 4"
    );
}

// ─── AC12b — list_with_gaps ─────────────────────────────────────────

/// AC8 ordering: gap_count DESC, then name ASC for ties.
/// Verifies projection fields (id, name, total, owned).
#[sqlx::test(migrations = "./migrations")]
async fn list_with_gaps_returns_in_gap_count_desc_then_name_asc_order_with_limit(
    pool: MySqlPool,
) {
    // Three series:
    //   - "Tintin" total=24, 18 distinct positions filled → gap=6
    //   - "Blacksad" total=10, 5 distinct positions filled → gap=5
    //   - "Mortelle Adèle" total=20, 14 distinct positions filled → gap=6
    let tintin = insert_series(&pool, "Tintin", "closed", Some(24)).await;
    for pos in 1..=18 {
        insert_filled_position(&pool, tintin, pos, pos as u32).await;
    }

    let blacksad = insert_series(&pool, "Blacksad", "closed", Some(10)).await;
    for pos in 1..=5 {
        insert_filled_position(&pool, blacksad, pos, 100 + pos as u32).await;
    }

    let mortelle = insert_series(&pool, "Mortelle Adèle", "closed", Some(20)).await;
    for pos in 1..=14 {
        insert_filled_position(&pool, mortelle, pos, 200 + pos as u32).await;
    }

    let rows = SeriesModel::list_with_gaps(&pool, 100)
        .await
        .expect("query ok");
    assert_eq!(rows.len(), 3, "3 gappy series in result");

    // Order: gap=6/Mortelle (M < T alphabetically) → gap=6/Tintin → gap=5/Blacksad
    assert_eq!(rows[0].name, "Mortelle Adèle");
    assert_eq!(rows[0].total_volume_count, 20);
    assert_eq!(rows[0].owned_count, 14);
    assert_eq!(rows[0].gap_count(), 6);

    assert_eq!(rows[1].name, "Tintin");
    assert_eq!(rows[1].total_volume_count, 24);
    assert_eq!(rows[1].owned_count, 18);
    assert_eq!(rows[1].gap_count(), 6);

    assert_eq!(rows[2].name, "Blacksad");
    assert_eq!(rows[2].total_volume_count, 10);
    assert_eq!(rows[2].owned_count, 5);
    assert_eq!(rows[2].gap_count(), 5);
}

#[sqlx::test(migrations = "./migrations")]
async fn list_with_gaps_honors_limit(pool: MySqlPool) {
    let tintin = insert_series(&pool, "Tintin", "closed", Some(24)).await;
    for pos in 1..=18 {
        insert_filled_position(&pool, tintin, pos, pos as u32).await;
    }
    let blacksad = insert_series(&pool, "Blacksad", "closed", Some(10)).await;
    for pos in 1..=5 {
        insert_filled_position(&pool, blacksad, pos, 100 + pos as u32).await;
    }
    let mortelle = insert_series(&pool, "Mortelle Adèle", "closed", Some(20)).await;
    for pos in 1..=14 {
        insert_filled_position(&pool, mortelle, pos, 200 + pos as u32).await;
    }

    let rows = SeriesModel::list_with_gaps(&pool, 1)
        .await
        .expect("query ok");
    assert_eq!(rows.len(), 1, "LIMIT 1 truncates to the top row");
    assert_eq!(rows[0].name, "Mortelle Adèle", "top row by gap+name order");
}

#[sqlx::test(migrations = "./migrations")]
async fn list_with_gaps_excludes_soft_deleted_series(pool: MySqlPool) {
    let alive = insert_series(&pool, "Alive", "closed", Some(5)).await;
    insert_filled_position(&pool, alive, 1, 1).await;

    let dead = insert_series(&pool, "Dead", "closed", Some(5)).await;
    insert_filled_position(&pool, dead, 1, 2).await;
    soft_delete_series(&pool, dead).await;

    let rows = SeriesModel::list_with_gaps(&pool, 100)
        .await
        .expect("query ok");
    assert_eq!(rows.len(), 1, "soft-deleted series excluded from list too");
    assert_eq!(rows[0].name, "Alive");
}

// ─── Code-review patch P2 (2026-05-03) — titles.deleted_at filter ───

/// Code-review patch P2: a `title_series` row whose parent `title` is
/// soft-deleted MUST NOT count as a filled position. Locks symmetry
/// with `series::active_count_titles` (which already filters
/// `t.deleted_at IS NULL`).
///
/// Fixture: closed series total=5 with 5 distinct positions filled,
/// then soft-delete 2 of the 5 parent titles. The dashboard must
/// register the series as gappy (3 effective filled positions < 5
/// total), even though the `title_series` rows themselves are still
/// active.
#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_excludes_titles_with_soft_deleted_parent(pool: MySqlPool) {
    let s = insert_series(&pool, "Closed-WithDeletedTitles", "closed", Some(5)).await;
    let g = first_genre_id(&pool).await;

    // 5 distinct positions, 5 distinct titles — series is full (no gap)
    // before soft-delete.
    let titles: Vec<u64> = {
        let mut ids = Vec::with_capacity(5);
        for pos in 1..=5 {
            let t = insert_title(&pool, &format!("Z-P2-Title-{pos}"), g).await;
            insert_title_series_assignment_for_title(&pool, t, s, pos).await;
            ids.push(t);
        }
        ids
    };

    let n_before = SeriesModel::count_with_gaps(&pool)
        .await
        .expect("query ok");
    assert_eq!(n_before, 0, "5/5 distinct positions before soft-delete = no gap");

    // Soft-delete 2 titles (positions 3 and 4 effectively become empty).
    soft_delete_title(&pool, titles[2]).await;
    soft_delete_title(&pool, titles[3]).await;

    let n_after = SeriesModel::count_with_gaps(&pool)
        .await
        .expect("query ok");
    assert_eq!(
        n_after, 1,
        "after soft-deleting parent titles, distinct effective filled = 3 < 5 = gappy"
    );

    // list_with_gaps surfaces it with the correct effective owned_count.
    let rows = SeriesModel::list_with_gaps(&pool, 100)
        .await
        .expect("query ok");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Closed-WithDeletedTitles");
    assert_eq!(rows[0].total_volume_count, 5);
    assert_eq!(rows[0].owned_count, 3, "owned_count reflects active titles only");
    assert_eq!(rows[0].gap_count(), 2);
}

// ─── Code-review patch P4 (2026-05-03) — position_number > 0 guard ──

/// Code-review patch P4: `title_series.position_number INT NOT NULL`
/// has no schema CHECK > 0, so a data-error row at position 0 (or
/// negative) could silently fill a slot in `COUNT(DISTINCT
/// position_number)`. Slot-numbering convention is 1..total. Fix:
/// `AND ts.position_number > 0` in both queries.
///
/// Fixture: closed series total=5 with positions 1, 2, 3, 4, AND a
/// rogue row at position 0. Without the guard, COUNT(DISTINCT) = 5 →
/// not gappy (false negative). With the guard, only positions 1-4
/// count → 4 < 5 → gappy.
#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_position_zero_does_not_fill_slot(pool: MySqlPool) {
    let s = insert_series(&pool, "Closed-WithPosZero", "closed", Some(5)).await;
    let g = first_genre_id(&pool).await;

    // Positions 1..4 filled — 4 distinct valid positions.
    for pos in 1..=4 {
        let t = insert_title(&pool, &format!("Z-P4-Title-{pos}"), g).await;
        insert_title_series_assignment_for_title(&pool, t, s, pos).await;
    }

    // Rogue row at position 0 — must NOT count toward filled slots.
    let rogue = insert_title(&pool, "Z-P4-Title-rogue-pos0", g).await;
    insert_title_series_assignment_for_title(&pool, rogue, s, 0).await;

    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(
        n, 1,
        "position 0 row excluded; 4 distinct valid positions < 5 total = gappy"
    );

    let rows = SeriesModel::list_with_gaps(&pool, 100)
        .await
        .expect("query ok");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].owned_count, 4,
        "owned_count reflects positions 1-4 only, NOT the rogue position 0 row"
    );
    assert_eq!(rows[0].gap_count(), 1);
}

/// Counterpart: a negative position_number is also excluded.
#[sqlx::test(migrations = "./migrations")]
async fn count_with_gaps_negative_position_does_not_fill_slot(pool: MySqlPool) {
    let s = insert_series(&pool, "Closed-WithNegPos", "closed", Some(3)).await;
    let g = first_genre_id(&pool).await;

    // Position 1 + 2 filled.
    for pos in 1..=2 {
        let t = insert_title(&pool, &format!("Z-P4N-Title-{pos}"), g).await;
        insert_title_series_assignment_for_title(&pool, t, s, pos).await;
    }
    // Rogue row at position -1 — excluded.
    let rogue = insert_title(&pool, "Z-P4N-Title-rogue-neg", g).await;
    insert_title_series_assignment_for_title(&pool, rogue, s, -1).await;

    let n = SeriesModel::count_with_gaps(&pool).await.expect("query ok");
    assert_eq!(n, 1, "negative position excluded; 2 < 3 = gappy");
}
