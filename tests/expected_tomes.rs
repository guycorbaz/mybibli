//! Story 11-10 (FR141, FR145, AR29) — expected tomes: service CRUD,
//! auto-settle at catalogue time, the scan / manual-creation feedback,
//! `get_series` over MCP, and the purge / FK behaviour.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test expected_tomes

mod common;

use common::mcp::{Agent, add_title, router, soft_delete};
use common::web::{Browser, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::expected_tome::ExpectedTomeOrigin;
use mybibli::services::expected_tomes::{
    ExpectedTomeError, ExpectedTomeInput, ExpectedTomeService, SettleResult,
};
use serde_json::json;

// ─── Fixtures ────────────────────────────────────────────────────

/// A valid ISBN-13 `978` + `n` (9 digits) + check digit.
fn isbn(n: u32) -> String {
    let body = format!("978{n:09}");
    let check = mybibli::utils::ean13_check_digit(&format!("{body}0")).unwrap();
    format!("{body}{check}")
}

async fn add_series(pool: &DbPool, name: &str, series_type: &str, total: Option<i32>) -> u64 {
    sqlx::query("INSERT INTO series (name, series_type, total_volume_count) VALUES (?, ?, ?)")
        .bind(name)
        .bind(series_type)
        .bind(total)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn assign(pool: &DbPool, title_id: u64, series_id: u64, position: i32, omnibus: bool) {
    sqlx::query(
        "INSERT INTO title_series (title_id, series_id, position_number, is_omnibus) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(title_id)
    .bind(series_id)
    .bind(position)
    .bind(omnibus)
    .execute(pool)
    .await
    .unwrap();
}

async fn book(pool: &DbPool, title: &str, isbn: Option<&str>) -> u64 {
    add_title(pool, title, "book", "Roman", isbn).await
}

fn input<'a>(position: i32, title: &'a str, isbn: Option<&'a str>) -> ExpectedTomeInput<'a> {
    ExpectedTomeInput {
        position,
        title,
        isbn,
    }
}

async fn create(pool: &DbPool, series: u64, position: i32, title: &str, isbn: Option<&str>) -> u64 {
    ExpectedTomeService::create(
        pool,
        series,
        &input(position, title, isbn),
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap_or_else(|e| panic!("create {title}: {e}"))
    .id
}

async fn live_positions(pool: &DbPool, title_id: u64, series_id: u64) -> Vec<i32> {
    sqlx::query_scalar(
        "SELECT position_number FROM title_series \
         WHERE title_id = ? AND series_id = ? AND deleted_at IS NULL ORDER BY position_number",
    )
    .bind(title_id)
    .bind(series_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn settled_title(pool: &DbPool, tome: u64) -> Option<u64> {
    ExpectedTomeService::find(pool, tome)
        .await
        .unwrap()
        .expect("tome")
        .settled_title_id
}

// ─── CRUD ────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn crud_round_trip(pool: DbPool) {
    let s = add_series(&pool, "Tintin", "closed", Some(24)).await;
    let i3 = isbn(3);
    let t3 = create(&pool, s, 3, "  Le Lotus bleu ", Some(&i3)).await;
    let t1 = create(&pool, s, 1, "Tintin au pays des Soviets", None).await;

    let list = ExpectedTomeService::list_by_series(&pool, s).await.unwrap();
    assert_eq!(list.iter().map(|r| r.id).collect::<Vec<_>>(), vec![t1, t3]);
    let row = &list[1];
    assert_eq!(row.title, "Le Lotus bleu");
    assert_eq!(row.isbn.as_deref(), Some(i3.as_str()));
    assert_eq!(row.origin, ExpectedTomeOrigin::Manual);
    assert_eq!(row.settled_title_id, None);
    assert_eq!(row.version, 1);

    // Update: keeping its own position and ISBN is not a collision.
    let updated = ExpectedTomeService::update(&pool, t3, 1, &input(3, "Le Lotus Bleu", Some(&i3)))
        .await
        .unwrap();
    assert_eq!(updated.title, "Le Lotus Bleu");
    assert_eq!(updated.version, 2);
    // Moving it to a free position.
    let moved = ExpectedTomeService::update(&pool, t3, 2, &input(5, "Le Lotus Bleu", None))
        .await
        .unwrap();
    assert_eq!((moved.position, moved.isbn.as_deref()), (5, None));

    // Stale version.
    let e = ExpectedTomeService::update(&pool, t3, 2, &input(6, "X", None))
        .await
        .unwrap_err();
    assert!(
        matches!(e, ExpectedTomeError::VersionConflict { .. }),
        "{e}"
    );
    let e = ExpectedTomeService::delete(&pool, t1, 7).await.unwrap_err();
    assert!(
        matches!(e, ExpectedTomeError::VersionConflict { .. }),
        "{e}"
    );

    // Delete, then the freed position (and ISBN) can be reused.
    ExpectedTomeService::delete(&pool, t1, 1).await.unwrap();
    assert!(
        ExpectedTomeService::find(&pool, t1)
            .await
            .unwrap()
            .is_none()
    );
    let again = create(&pool, s, 1, "Soviets", None).await;
    assert_ne!(again, t1);
    let e = ExpectedTomeService::delete(&pool, t1, 2).await.unwrap_err();
    assert!(matches!(e, ExpectedTomeError::NotFound { .. }), "{e}");

    // A proposal origin is kept.
    let p =
        ExpectedTomeService::create(&pool, s, &input(9, "P", None), ExpectedTomeOrigin::Proposal)
            .await
            .unwrap();
    assert_eq!(p.origin, ExpectedTomeOrigin::Proposal);
}

#[sqlx::test(migrations = "./migrations")]
async fn isbn10_is_stored_as_isbn13(pool: DbPool) {
    let s = add_series(&pool, "Astérix", "open", None).await;
    let id = create(&pool, s, 1, "Astérix le Gaulois", Some("2-205-00121-3")).await;
    let stored: Option<String> =
        sqlx::query_scalar("SELECT isbn FROM series_expected_tomes WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored.as_deref(), Some("9782205001211"));
    // The ISBN-13 form of the same book is now a duplicate in this series.
    let e = ExpectedTomeService::create(
        &pool,
        s,
        &input(2, "Dup", Some("9782205001211")),
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap_err();
    assert!(matches!(e, ExpectedTomeError::DuplicateIsbn { .. }), "{e}");
}

#[sqlx::test(migrations = "./migrations")]
async fn create_refusals(pool: DbPool) {
    let s = add_series(&pool, "Closed", "closed", Some(3)).await;
    let owned = book(&pool, "Owned", None).await;
    assign(&pool, owned, s, 1, false).await;
    let i = isbn(10);
    create(&pool, s, 2, "Two", Some(&i)).await;
    let deleted = add_series(&pool, "Gone", "open", None).await;
    soft_delete(&pool, "series", deleted).await;

    let cases: Vec<(u64, ExpectedTomeInput<'_>, &str)> = vec![
        (999_999, input(1, "x", None), "SeriesNotFound"),
        (deleted, input(1, "x", None), "SeriesNotFound"),
        (s, input(0, "x", None), "PositionOutOfRange"),
        (s, input(4, "x", None), "PositionOutOfRange"),
        (s, input(1, "x", None), "PositionHeld"),
        (s, input(2, "x", None), "PositionTaken"),
        (s, input(3, "x", Some(&i)), "DuplicateIsbn"),
        (s, input(3, "x", Some("9782205001212")), "InvalidIsbn"),
        (s, input(3, "   ", None), "TitleRequired"),
    ];
    for (series, inp, want) in cases {
        let e = ExpectedTomeService::create(&pool, series, &inp, ExpectedTomeOrigin::Manual)
            .await
            .unwrap_err();
        assert!(format!("{e:?}").starts_with(want), "{inp:?}: got {e:?}");
    }
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM series_expected_tomes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "no refused tome was written");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_held_position_by_a_deleted_title_is_free(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    let t = book(&pool, "Deleted copy", None).await;
    assign(&pool, t, s, 1, false).await;
    soft_delete(&pool, "titles", t).await;
    create(&pool, s, 1, "Tome 1", None).await;
}

// ─── Auto-settle ─────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn settle_attaches_the_title_and_settles_the_tome(pool: DbPool) {
    let s = add_series(&pool, "Lucky Luke", "closed", Some(5)).await;
    let i = isbn(20);
    let tome = create(&pool, s, 3, "Tome 3", Some(&i)).await;
    let title = book(&pool, "Lucky Luke 3", Some(&i)).await;

    let out = ExpectedTomeService::settle_for_title(&pool, title, &i)
        .await
        .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].result, SettleResult::Settled);
    assert_eq!((out[0].series_id, out[0].position), (s, 3));
    assert_eq!(out[0].series_name, "Lucky Luke");
    assert_eq!(live_positions(&pool, title, s).await, vec![3]);
    assert_eq!(settled_title(&pool, tome).await, Some(title));

    // A settled tome is read-only and matches nothing any more.
    let e = ExpectedTomeService::update(&pool, tome, 2, &input(3, "x", None))
        .await
        .unwrap_err();
    assert!(matches!(e, ExpectedTomeError::Settled { .. }), "{e}");
    assert!(
        ExpectedTomeService::settle_for_title(&pool, title, &i)
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn settle_when_the_title_already_holds_the_position(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    let i = isbn(21);
    let tome = create(&pool, s, 2, "Tome 2", Some(&i)).await;
    let title = book(&pool, "T2", Some(&i)).await;
    assign(&pool, title, s, 2, false).await;

    let out = ExpectedTomeService::settle_for_title(&pool, title, &i)
        .await
        .unwrap();
    assert_eq!(out[0].result, SettleResult::Settled);
    assert_eq!(
        live_positions(&pool, title, s).await,
        vec![2],
        "no second row"
    );
    assert_eq!(settled_title(&pool, tome).await, Some(title));
}

#[sqlx::test(migrations = "./migrations")]
async fn conflicts_attach_nothing(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(6)).await;

    // Title already elsewhere in the series.
    let i1 = isbn(31);
    let tome1 = create(&pool, s, 1, "One", Some(&i1)).await;
    let t1 = book(&pool, "T1", Some(&i1)).await;
    assign(&pool, t1, s, 6, false).await;
    // Position held by another title (single row).
    let i2 = isbn(32);
    let tome2 = create(&pool, s, 2, "Two", Some(&i2)).await;
    let other = book(&pool, "Other <b>2</b>", None).await;
    assign(&pool, other, s, 2, false).await;
    // Position held by an omnibus covering 3..=4.
    let i3 = isbn(33);
    let tome3 = create(&pool, s, 4, "Four", Some(&i3)).await;
    let omnibus = book(&pool, "Omnibus 3-4", None).await;
    assign(&pool, omnibus, s, 3, true).await;
    assign(&pool, omnibus, s, 4, true).await;
    // Total lowered below the position.
    let other_series = add_series(&pool, "Shrunk", "closed", Some(5)).await;
    let i4 = isbn(34);
    let tome4 = create(&pool, other_series, 5, "Five", Some(&i4)).await;
    sqlx::query("UPDATE series SET total_volume_count = 3 WHERE id = ?")
        .bind(other_series)
        .execute(&pool)
        .await
        .unwrap();

    let t2 = book(&pool, "T2", Some(&i2)).await;
    let t3 = book(&pool, "T3", Some(&i3)).await;
    let t4 = book(&pool, "T4", Some(&i4)).await;
    let r = |out: Vec<mybibli::services::expected_tomes::SettleOutcome>| out[0].result.clone();
    assert_eq!(
        r(ExpectedTomeService::settle_for_title(&pool, t1, &i1)
            .await
            .unwrap()),
        SettleResult::TitleElsewhere { position: 6 }
    );
    assert_eq!(
        r(ExpectedTomeService::settle_for_title(&pool, t2, &i2)
            .await
            .unwrap()),
        SettleResult::PositionHeld {
            holder: "Other <b>2</b>".into()
        }
    );
    assert_eq!(
        r(ExpectedTomeService::settle_for_title(&pool, t3, &i3)
            .await
            .unwrap()),
        SettleResult::PositionHeld {
            holder: "Omnibus 3-4".into()
        }
    );
    assert_eq!(
        r(ExpectedTomeService::settle_for_title(&pool, t4, &i4)
            .await
            .unwrap()),
        SettleResult::OutOfRange { total: 3 }
    );
    for tome in [tome1, tome2, tome3, tome4] {
        assert_eq!(
            settled_title(&pool, tome).await,
            None,
            "tome {tome} unsettled"
        );
    }
    assert_eq!(live_positions(&pool, t1, s).await, vec![6]);
    for t in [t2, t3] {
        assert!(live_positions(&pool, t, s).await.is_empty());
    }
    assert!(live_positions(&pool, t4, other_series).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn settle_handles_every_series_and_ignores_deleted_ones(pool: DbPool) {
    let i = isbn(40);
    let a = add_series(&pool, "A", "open", None).await;
    let b = add_series(&pool, "B", "open", None).await;
    let gone = add_series(&pool, "Gone", "open", None).await;
    create(&pool, a, 1, "In A", Some(&i)).await;
    create(&pool, b, 7, "In B", Some(&i)).await;
    create(&pool, gone, 2, "In Gone", Some(&i)).await;
    soft_delete(&pool, "series", gone).await;
    let deleted_tome = create(&pool, a, 9, "Deleted", Some(&isbn(41))).await;
    ExpectedTomeService::delete(&pool, deleted_tome, 1)
        .await
        .unwrap();

    let title = book(&pool, "Shared", Some(&i)).await;
    let out = ExpectedTomeService::settle_for_title(&pool, title, &i)
        .await
        .unwrap();
    let got: Vec<(u64, i32)> = out.iter().map(|o| (o.series_id, o.position)).collect();
    assert_eq!(got, vec![(a, 1), (b, 7)]);
    assert!(live_positions(&pool, title, gone).await.is_empty());
    // A deleted tome never settles; a non-ISBN-13 never matches.
    let t = book(&pool, "Other", Some(&isbn(41))).await;
    assert!(
        ExpectedTomeService::settle_for_title(&pool, t, &isbn(41))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        ExpectedTomeService::settle_for_title(&pool, t, "abc")
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_settled_title_unsettles_the_tome(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    let i = isbn(50);
    let tome = create(&pool, s, 1, "One", Some(&i)).await;
    let first = book(&pool, "First copy", Some(&i)).await;
    ExpectedTomeService::settle_for_title(&pool, first, &i)
        .await
        .unwrap();
    soft_delete(&pool, "titles", first).await;
    assert_eq!(settled_title(&pool, tome).await, None);

    let second = book(&pool, "Second copy", Some(&i)).await;
    let out = ExpectedTomeService::settle_for_title(&pool, second, &i)
        .await
        .unwrap();
    assert_eq!(out[0].result, SettleResult::Settled);
    assert_eq!(settled_title(&pool, tome).await, Some(second));
}

// ─── FKs and purge ───────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn hard_deletes_cascade_or_unset(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    let i = isbn(60);
    let tome = create(&pool, s, 1, "One", Some(&i)).await;
    let title = book(&pool, "T", Some(&i)).await;
    ExpectedTomeService::settle_for_title(&pool, title, &i)
        .await
        .unwrap();

    // Purging the settled title (after its junction rows) unsets the link.
    sqlx::query("DELETE FROM title_series WHERE title_id = ?")
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM titles WHERE id = ?")
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    let raw: Option<u64> =
        sqlx::query_scalar("SELECT settled_title_id FROM series_expected_tomes WHERE id = ?")
            .bind(tome)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(raw, None);

    // Purging the series takes its tomes.
    sqlx::query("DELETE FROM series WHERE id = ?")
        .bind(s)
        .execute(&pool)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM series_expected_tomes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn auto_purge_drains_old_tomes_and_series(pool: DbPool) {
    let s = add_series(&pool, "Old", "open", None).await;
    let live_tome = create(&pool, s, 1, "Live tome of a deleted series", None).await;
    let keep = add_series(&pool, "Kept", "open", None).await;
    let old_tome = create(&pool, keep, 2, "Old deleted tome", None).await;
    let recent_tome = create(&pool, keep, 3, "Recently deleted tome", None).await;
    for (table, id, days) in [
        ("series", s, 31),
        ("series_expected_tomes", old_tome, 31),
        ("series_expected_tomes", recent_tome, 2),
    ] {
        sqlx::query(&format!(
            "UPDATE {table} SET deleted_at = NOW() - INTERVAL {days} DAY WHERE id = ?"
        ))
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    }
    let stats = mybibli::services::auto_purge::AutoPurgeService::run_purge(&pool)
        .await
        .unwrap();
    assert!(stats.errors.is_empty(), "{:?}", stats.errors);
    assert_eq!(stats.per_table.get("series_expected_tomes"), Some(&1));
    let remaining: Vec<u64> =
        sqlx::query_scalar("SELECT id FROM series_expected_tomes ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        remaining,
        vec![recent_tome],
        "{live_tome} cascaded with its series"
    );
}

// ─── Web feedback (FR145) ────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn scanning_the_isbn_settles_and_says_so(pool: DbPool) {
    seed_admin(&pool, "tome_admin").await;
    let app = router(&pool);
    let browser = Browser::login(&app, "tome_admin").await;
    let s = add_series(&pool, "Blake <et> Mortimer", "closed", Some(4)).await;
    // The tome is entered with the ISBN-10; the scanner sends it too.
    let tome = create(&pool, s, 2, "Le Mystère", Some("0201633612")).await;

    let (status, body) = browser
        .post_form(&app, "/catalog/scan", "code=0201633612")
        .await;
    assert!(status.is_success(), "{status}: {body}");
    assert!(
        body.contains("Expected tome found") || body.contains("Tome attendu trouvé"),
        "{body}"
    );
    assert!(
        body.contains("Blake &lt;et&gt; Mortimer"),
        "escaped: {body}"
    );
    let title: u64 = sqlx::query_scalar("SELECT id FROM titles WHERE isbn = '9780201633610'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(live_positions(&pool, title, s).await, vec![2]);
    assert_eq!(settled_title(&pool, tome).await, Some(title));
}

#[sqlx::test(migrations = "./migrations")]
async fn scanning_reports_a_conflict(pool: DbPool) {
    seed_admin(&pool, "tome_admin").await;
    let app = router(&pool);
    let browser = Browser::login(&app, "tome_admin").await;
    let s = add_series(&pool, "S", "open", None).await;
    let i = isbn(70);
    let tome = create(&pool, s, 1, "One", Some(&i)).await;
    let holder = book(&pool, "Holder", None).await;
    assign(&pool, holder, s, 1, false).await;

    let (status, body) = browser
        .post_form(&app, "/catalog/scan", &format!("code={i}"))
        .await;
    assert!(status.is_success(), "{status}: {body}");
    assert!(
        body.contains("nothing was attached")
            || body.contains("rien n&#x27;a été rattaché")
            || body.contains("rien n'a été rattaché"),
        "{body}"
    );
    assert!(body.contains("Holder"), "{body}");
    assert_eq!(settled_title(&pool, tome).await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn manual_creation_with_the_isbn_settles(pool: DbPool) {
    seed_admin(&pool, "tome_admin").await;
    let app = router(&pool);
    let browser = Browser::login(&app, "tome_admin").await;
    let s = add_series(&pool, "Manual", "open", None).await;
    let i = isbn(80);
    let tome = create(&pool, s, 4, "Four", Some(&i)).await;
    let genre = common::mcp::genre_id(&pool, "Roman").await;

    let (status, body) = browser
        .post_form(
            &app,
            "/catalog/title",
            &format!("title=Four&media_type=book&genre_id={genre}&isbn={i}"),
        )
        .await;
    assert!(status.is_success(), "{status}: {body}");
    assert!(
        body.contains("Expected tome found") || body.contains("Tome attendu trouvé"),
        "{body}"
    );
    let title: u64 = sqlx::query_scalar("SELECT id FROM titles WHERE isbn = ?")
        .bind(&i)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(settled_title(&pool, tome).await, Some(title));
}

// ─── MCP get_series ──────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn get_series_returns_the_expected_tomes(pool: DbPool) {
    let s = add_series(&pool, "MCP", "closed", Some(5)).await;
    let i = isbn(90);
    let t5 = create(&pool, s, 5, "Five", None).await;
    let t2 = create(&pool, s, 2, "Two", Some(&i)).await;
    let title = book(&pool, "Two owned", Some(&i)).await;
    ExpectedTomeService::settle_for_title(&pool, title, &i)
        .await
        .unwrap();
    let deleted = create(&pool, s, 3, "Deleted", None).await;
    ExpectedTomeService::delete(&pool, deleted, 1)
        .await
        .unwrap();

    let agent = Agent::new(&pool).await;
    let doc = agent.ok("get_series", json!({"id": s})).await;
    assert_eq!(
        doc["expected_tomes"],
        json!([
            {"id": t2, "position": 2, "title": "Two", "isbn": i, "origin": "manual",
             "settled_title_id": title},
            {"id": t5, "position": 5, "title": "Five", "isbn": null, "origin": "manual",
             "settled_title_id": null}
        ])
    );
    // The settled tome's title now fills position 2.
    assert_eq!(doc["missing_positions"], json!([1, 3, 4, 5]));
}

// ─── Review round 1 regressions ──────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn update_refusals_and_unchanged_position(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(4)).await;
    let i2 = isbn(101);
    let t1 = create(&pool, s, 1, "One", None).await;
    create(&pool, s, 2, "Two", Some(&i2)).await;
    let owned = book(&pool, "Owned 3", None).await;
    assign(&pool, owned, s, 3, false).await;

    for (inp, want) in [
        (input(3, "One", None), "PositionHeld"),
        (input(2, "One", None), "PositionTaken"),
        (input(5, "One", None), "PositionOutOfRange"),
        (input(1, "One", Some(&i2)), "DuplicateIsbn"),
    ] {
        let e = ExpectedTomeService::update(&pool, t1, 1, &inp)
            .await
            .unwrap_err();
        assert!(format!("{e:?}").starts_with(want), "{inp:?}: got {e:?}");
    }

    // Position 1 gets filled by hand by an unrelated title: fixing the
    // tome's title in place still works.
    let other = book(&pool, "Unrelated", None).await;
    assign(&pool, other, s, 1, false).await;
    let row = ExpectedTomeService::update(&pool, t1, 1, &input(1, "One (fixed)", None))
        .await
        .unwrap();
    assert_eq!(row.title, "One (fixed)");
    // The total is lowered below tome 2's position: it can still be fixed
    // in place, but not moved outside the new range.
    sqlx::query("UPDATE series SET total_volume_count = 1 WHERE id = ?")
        .bind(s)
        .execute(&pool)
        .await
        .unwrap();
    let t2 = ExpectedTomeService::list_by_series(&pool, s).await.unwrap()[1].clone();
    assert_eq!(t2.position, 2);
    ExpectedTomeService::update(
        &pool,
        t2.id,
        t2.version,
        &input(2, "Two (fixed)", Some(&i2)),
    )
    .await
    .unwrap();
    let e = ExpectedTomeService::update(&pool, t2.id, t2.version + 1, &input(4, "Two", Some(&i2)))
        .await
        .unwrap_err();
    assert!(
        matches!(e, ExpectedTomeError::PositionOutOfRange { .. }),
        "{e}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn unassigning_the_settled_title_unsettles_the_tome(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    let i = isbn(102);
    let tome = create(&pool, s, 2, "Two", Some(&i)).await;
    let title = book(&pool, "T", Some(&i)).await;
    ExpectedTomeService::settle_for_title(&pool, title, &i)
        .await
        .unwrap();
    assert_eq!(settled_title(&pool, tome).await, Some(title));

    sqlx::query("UPDATE title_series SET deleted_at = NOW() WHERE title_id = ?")
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        settled_title(&pool, tome).await,
        None,
        "position is a gap again"
    );

    // Scanning again re-attaches it.
    let out = ExpectedTomeService::settle_for_title(&pool, title, &i)
        .await
        .unwrap();
    assert_eq!(out[0].result, SettleResult::Settled);
    assert_eq!(live_positions(&pool, title, s).await, vec![2]);
}

#[sqlx::test(migrations = "./migrations")]
async fn editing_after_the_settled_title_is_deleted_clears_the_link(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    let i = isbn(103);
    let tome = create(&pool, s, 1, "One", Some(&i)).await;
    let title = book(&pool, "T", Some(&i)).await;
    ExpectedTomeService::settle_for_title(&pool, title, &i)
        .await
        .unwrap();
    soft_delete(&pool, "titles", title).await;
    let version = ExpectedTomeService::find(&pool, tome)
        .await
        .unwrap()
        .unwrap()
        .version;
    ExpectedTomeService::update(&pool, tome, version, &input(4, "Four", None))
        .await
        .unwrap();
    // The title comes back from the Trash: the edited tome stays unsettled.
    sqlx::query("UPDATE titles SET deleted_at = NULL WHERE id = ?")
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(settled_title(&pool, tome).await, None);
    let raw: Option<u64> =
        sqlx::query_scalar("SELECT settled_title_id FROM series_expected_tomes WHERE id = ?")
            .bind(tome)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(raw, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_tome_for_an_owned_isbn_settles_on_entry(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    let i = isbn(104);
    let title = book(&pool, "Already owned", Some(&i)).await;
    let row = ExpectedTomeService::create(
        &pool,
        s,
        &input(3, "Three", Some(&i)),
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap();
    assert_eq!(row.settled_title_id, Some(title));
    assert_eq!(live_positions(&pool, title, s).await, vec![3]);
}

#[sqlx::test(migrations = "./migrations")]
async fn hand_assignment_and_isbn_edit_only_record_what_the_series_shows(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(5)).await;
    let i1 = isbn(105);
    let i2 = isbn(106);
    let tome1 = create(&pool, s, 1, "One", Some(&i1)).await;
    let tome2 = create(&pool, s, 2, "Two", Some(&i2)).await;

    // A title carrying the ISBN is assigned by hand at the tome's position.
    let t1 = book(&pool, "T1", Some(&i1)).await;
    mybibli::services::series::SeriesService::assign_title(&pool, t1, s, 1)
        .await
        .unwrap();
    assert_eq!(settled_title(&pool, tome1).await, Some(t1));

    // A title gains the tome's ISBN through an edit while it sits
    // elsewhere: nothing is attached behind the librarian's back.
    let t2 = book(&pool, "T2", None).await;
    let elsewhere = add_series(&pool, "Elsewhere", "open", None).await;
    assign(&pool, t2, elsewhere, 1, false).await;
    sqlx::query("UPDATE titles SET isbn = ? WHERE id = ?")
        .bind(&i2)
        .bind(t2)
        .execute(&pool)
        .await
        .unwrap();
    ExpectedTomeService::settle_for_title_id(&pool, t2).await;
    assert_eq!(settled_title(&pool, tome2).await, None);
    assert!(live_positions(&pool, t2, s).await.is_empty());

    // Once it is placed at the tome's position by hand, the tome settles.
    mybibli::services::series::SeriesService::assign_title(&pool, t2, s, 2)
        .await
        .unwrap();
    assert_eq!(settled_title(&pool, tome2).await, Some(t2));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_title_removed_on_purpose_is_not_reattached_by_other_actions(pool: DbPool) {
    let a = add_series(&pool, "A", "open", None).await;
    let b = add_series(&pool, "B", "open", None).await;
    let i = isbn(108);
    let tome = create(&pool, a, 2, "Two", Some(&i)).await;
    let t = book(&pool, "T", Some(&i)).await;
    ExpectedTomeService::settle_for_title(&pool, t, &i)
        .await
        .unwrap();
    // The librarian removes T from A on purpose.
    sqlx::query("UPDATE title_series SET deleted_at = NOW() WHERE title_id = ? AND series_id = ?")
        .bind(t)
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();
    // Assigning T to another series, or fixing the tome's label, does not
    // put it back into A.
    mybibli::services::series::SeriesService::assign_title(&pool, t, b, 1)
        .await
        .unwrap();
    let v = ExpectedTomeService::find(&pool, tome)
        .await
        .unwrap()
        .unwrap()
        .version;
    ExpectedTomeService::update(&pool, tome, v, &input(2, "Two (fixed)", Some(&i)))
        .await
        .unwrap();
    assert!(live_positions(&pool, t, a).await.is_empty());
    assert_eq!(settled_title(&pool, tome).await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn omnibus_assignment_settles_a_tome_in_its_range(pool: DbPool) {
    let s = add_series(&pool, "S", "closed", Some(6)).await;
    let i = isbn(109);
    let tome = create(&pool, s, 4, "Four", Some(&i)).await;
    let omnibus = book(&pool, "Omnibus", Some(&i)).await;
    mybibli::services::series::SeriesService::assign_omnibus(&pool, omnibus, s, 3, 5)
        .await
        .unwrap();
    assert_eq!(settled_title(&pool, tome).await, Some(omnibus));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_tome_entered_for_a_title_already_in_place_or_stored_as_isbn10(pool: DbPool) {
    let s = add_series(&pool, "S", "open", None).await;
    // Title already at position 1, same ISBN: the tome is accepted, settled.
    let i1 = isbn(110);
    let held = book(&pool, "Held", Some(&i1)).await;
    assign(&pool, held, s, 1, false).await;
    let row = ExpectedTomeService::create(
        &pool,
        s,
        &input(1, "One", Some(&i1)),
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap();
    assert_eq!(row.settled_title_id, Some(held));
    // A title whose ISBN was typed as an ISBN-10 is found when the tome is
    // entered with the ISBN-13.
    let ten = book(&pool, "Typed ten", Some("2-07-036024-5")).await;
    let row = ExpectedTomeService::create(
        &pool,
        s,
        &input(2, "Two", Some("9782070360246")),
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap();
    assert_eq!(row.settled_title_id, Some(ten));
    assert_eq!(live_positions(&pool, ten, s).await, vec![2]);
}

#[sqlx::test(migrations = "./migrations")]
async fn rescanning_an_existing_title_settles(pool: DbPool) {
    seed_admin(&pool, "tome_admin").await;
    let app = router(&pool);
    let browser = Browser::login(&app, "tome_admin").await;
    let s = add_series(&pool, "Rescan", "open", None).await;
    let i = isbn(107);
    let tome = create(&pool, s, 6, "Six", Some(&i)).await;
    // Catalogued before the tome existed in the series' knowledge.
    let title = book(&pool, "Owned copy", Some(&i)).await;

    let (status, body) = browser
        .post_form(&app, "/catalog/scan", &format!("code={i}"))
        .await;
    assert!(status.is_success(), "{status}: {body}");
    assert!(
        body.contains("Expected tome found") || body.contains("Tome attendu trouvé"),
        "{body}"
    );
    assert_eq!(settled_title(&pool, tome).await, Some(title));
}

#[sqlx::test(migrations = "./migrations")]
async fn manual_creation_with_an_isbn10_settles(pool: DbPool) {
    seed_admin(&pool, "tome_admin").await;
    let app = router(&pool);
    let browser = Browser::login(&app, "tome_admin").await;
    let s = add_series(&pool, "Hyphen", "open", None).await;
    let tome = create(&pool, s, 1, "One", Some("9782070360246")).await;
    let genre = common::mcp::genre_id(&pool, "Roman").await;

    let (status, body) = browser
        .post_form(
            &app,
            "/catalog/title",
            &format!("title=One&media_type=book&genre_id={genre}&isbn=2070360245"),
        )
        .await;
    assert!(status.is_success(), "{status}: {body}");
    assert!(settled_title(&pool, tome).await.is_some(), "{body}");
}
