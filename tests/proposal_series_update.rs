//! Story 15-4 (#590, FR164) — `series_update` proposals: filing through
//! the MCP tool `propose_series_update`, the review screen, line edits and
//! removals, acceptance (applied in one transaction, stale on a version
//! change, a changed tome, a position owned above the new total) and the
//! lock scope of an acceptance (epic 12 retro A1).
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test proposal_series_update

mod common;

use axum::http::StatusCode;
use common::mcp::*;
use common::web::{Browser, PW_HASH, seed_admin};
use mybibli::db::DbPool;
use serde_json::{Value, json};

const SRC: &str = "https://example.org/series";

struct Fx {
    agent: Agent,
    lib: Browser,
    lib_id: u64,
}

async fn fx(pool: &DbPool) -> Fx {
    let agent = Agent::new(pool).await;
    seed_admin(pool, "su-admin").await;
    let lib_id =
        sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
            .bind("su-lib")
            .bind(PW_HASH)
            .execute(pool)
            .await
            .unwrap()
            .last_insert_id();
    let lib = Browser::login(&agent.app, "su-lib").await;
    Fx { agent, lib, lib_id }
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

async fn assign(pool: &DbPool, title_id: u64, series_id: u64, position: i32) {
    sqlx::query("INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, ?)")
        .bind(title_id)
        .bind(series_id)
        .bind(position)
        .execute(pool)
        .await
        .unwrap();
}

async fn add_tome(pool: &DbPool, series_id: u64, position: i32, title: &str) -> u64 {
    sqlx::query(
        "INSERT INTO series_expected_tomes (series_id, position, title, origin) \
         VALUES (?, ?, ?, 'manual')",
    )
    .bind(series_id)
    .bind(position)
    .bind(title)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn file(f: &Fx, args: Value) -> u64 {
    f.agent.ok("propose_series_update", args).await["proposal_id"]
        .as_u64()
        .expect("proposal_id")
}

async fn proposal_count(pool: &DbPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM proposals")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn version_of(pool: &DbPool, id: u64) -> i32 {
    sqlx::query_scalar("SELECT version FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn status_of(pool: &DbPool, id: u64) -> String {
    sqlx::query_scalar("SELECT status FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// `(name, type, total, version)`.
async fn series_row(pool: &DbPool, id: u64) -> (String, String, Option<i32>, i32) {
    sqlx::query_as(
        "SELECT name, series_type, total_volume_count, version FROM series WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn live_tome_positions(pool: &DbPool, series_id: u64) -> Vec<i32> {
    sqlx::query_scalar(
        "SELECT position FROM series_expected_tomes \
         WHERE series_id = ? AND deleted_at IS NULL ORDER BY position",
    )
    .bind(series_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// `(item id, proposed_values)` of the live items, in order.
async fn items(pool: &DbPool, id: u64) -> Vec<(u64, Value)> {
    let rows: Vec<(u64, String)> = sqlx::query_as(
        "SELECT id, CAST(proposed_values AS CHAR) FROM proposal_items \
         WHERE proposal_id = ? AND deleted_at IS NULL ORDER BY id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .unwrap();
    rows.into_iter()
        .map(|(i, v)| (i, serde_json::from_str(&v).unwrap()))
        .collect()
}

fn text(body: String) -> String {
    body.replace("&#x27;", "'").replace("&quot;", "\"")
}

async fn accept(f: &Fx, pool: &DbPool, id: u64) -> (StatusCode, String) {
    let v = version_of(pool, id).await;
    let (status, body) = f
        .lib
        .post_form(
            &f.agent.app,
            &format!("/proposals/{id}/accept"),
            &format!("version={v}"),
        )
        .await;
    (status, text(body))
}

// ─── Filing ─────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn closing_a_series_records_the_change_and_the_removals(pool: DbPool) {
    let f = fx(&pool).await;
    let series = add_series(&pool, "Jeremiah", "open", None).await;
    let t1 = seed_title(&pool, "Jeremiah 1").await;
    assign(&pool, t1, series, 1).await;
    add_tome(&pool, series, 42, "Jeremiah 42").await;
    let ghost = add_tome(&pool, series, 43, "Jeremiah 43").await;
    let id = file(
        &f,
        json!({"series_id": series, "series_type": "closed", "total_volume_count": 42,
               "remove_expected_tomes": [ghost], "sources": [SRC]}),
    )
    .await;
    let lines = items(&pool, id).await;
    assert_eq!(lines.len(), 2);
    let update = &lines[0].1;
    assert_eq!(update["line"], "series_update");
    assert_eq!(update["series_type"], "closed");
    assert_eq!(update["total_volume_count"], 42);
    assert!(update.get("name").is_none(), "{update}");
    assert_eq!(
        update["current"],
        json!({"name": "Jeremiah", "series_type": "open", "total_volume_count": null})
    );
    assert_eq!(lines[1].1["line"], "tome_removal");
    assert_eq!(lines[1].1["tome_id"], ghost);
    assert_eq!(lines[1].1["position"], 43);
    let (kind, key): (String, String) =
        sqlx::query_as("SELECT kind, CAST(equivalence_key AS CHAR) FROM proposals WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(kind, "series_update");
    assert!(
        key.starts_with(&format!("series_update:series/{series}:")),
        "{key}"
    );
    // The catalog is untouched.
    assert_eq!(series_row(&pool, series).await.1, "open");
    assert_eq!(live_tome_positions(&pool, series).await, vec![42, 43]);
}

#[sqlx::test(migrations = "./migrations")]
async fn refusals_write_nothing_and_name_the_positions(pool: DbPool) {
    let f = fx(&pool).await;
    let series = add_series(&pool, "Neptune", "closed", Some(3)).await;
    let other = add_series(&pool, "Gaston", "open", None).await;
    for p in [1, 2] {
        let t = seed_title(&pool, &format!("Neptune {p}")).await;
        assign(&pool, t, series, p).await;
    }
    let t3 = add_tome(&pool, series, 3, "Neptune 3").await;
    let foreign = add_tome(&pool, other, 1, "Gaston 1").await;
    let cases: Vec<(Value, &str)> = vec![
        (json!({"series_id": 999_999}), "999999"),
        (json!({"series_id": series}), "Nothing to change"),
        (
            json!({"series_id": series, "series_type": "closed", "total_volume_count": 3}),
            "Nothing to change",
        ),
        (json!({"series_id": other, "total_volume_count": 4}), "is open"),
        (json!({"series_id": series, "name": "Gaston"}), "Another series"),
        (
            json!({"series_id": series, "total_volume_count": 1}),
            "owned position(s) 2",
        ),
        (
            json!({"series_id": series, "total_volume_count": 2}),
            "expected tome(s) at position(s) 3",
        ),
        (
            json!({"series_id": series, "remove_expected_tomes": [foreign]}),
            "not a live expected tome of series",
        ),
        (
            json!({"series_id": series, "remove_expected_tomes": [t3, t3]}),
            "listed twice",
        ),
        (json!({"series_id": series, "series_version": 99, "total_volume_count": 5}), "version"),
    ];
    for (mut args, expected) in cases {
        args["sources"] = json!([SRC]);
        let msg = f.agent.err("propose_series_update", args.clone()).await;
        assert!(msg.contains(expected), "{args}: {msg}");
    }
    assert_eq!(proposal_count(&pool).await, 0);
    // The rule is met by removing the tome in the same proposal.
    file(
        &f,
        json!({"series_id": series, "total_volume_count": 2,
               "remove_expected_tomes": [t3], "sources": [SRC]}),
    )
    .await;
    assert_eq!(proposal_count(&pool).await, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn one_pending_proposal_per_series_any_kind(pool: DbPool) {
    let f = fx(&pool).await;
    let series = add_series(&pool, "Tintin", "open", None).await;
    let attach = f
        .agent
        .ok(
            "propose_series_attach",
            json!({"series_id": series, "expected_tomes": [{"position": 1, "title": "T1"}],
                   "sources": [SRC]}),
        )
        .await["proposal_id"]
        .as_u64()
        .unwrap();
    let close = json!({"series_id": series, "series_type": "closed", "total_volume_count": 24,
                       "sources": [SRC]});
    let msg = f.agent.err("propose_series_update", close.clone()).await;
    assert!(msg.contains(&format!("Proposal {attach}")), "{msg}");
    // Once decided, the update is filed; it then blocks an attach.
    let (status, _) = f
        .lib
        .post_form(
            &f.agent.app,
            &format!("/proposals/{attach}/reject"),
            &format!("version={}", version_of(&pool, attach).await),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let update = file(&f, close.clone()).await;
    let msg = f
        .agent
        .err(
            "propose_series_attach",
            json!({"series_id": series, "expected_tomes": [{"position": 2, "title": "T2"}],
                   "sources": [SRC]}),
        )
        .await;
    assert!(msg.contains(&format!("Proposal {update}")), "{msg}");
    // A rejected change blocks the same change only.
    let (status, _) = f
        .lib
        .post_form(
            &f.agent.app,
            &format!("/proposals/{update}/reject"),
            &format!("version={}", version_of(&pool, update).await),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let msg = f.agent.err("propose_series_update", close).await;
    assert!(msg.contains("was rejected on"), "{msg}");
    file(
        &f,
        json!({"series_id": series, "series_type": "closed", "total_volume_count": 23,
               "sources": [SRC]}),
    )
    .await;
}

// ─── Review and acceptance ──────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn an_accepted_update_changes_the_series_and_removes_the_tomes(pool: DbPool) {
    let f = fx(&pool).await;
    let series = add_series(&pool, "Cubitus", "open", None).await;
    let t1 = seed_title(&pool, "Cubitus 1").await;
    assign(&pool, t1, series, 1).await;
    add_tome(&pool, series, 39, "Cubitus 39").await;
    let ghost = add_tome(&pool, series, 40, "Cubitus 40").await;
    let id = file(
        &f,
        json!({"series_id": series, "name": "Cubitus (Dupa)", "series_type": "closed",
               "total_volume_count": 39, "remove_expected_tomes": [ghost], "sources": [SRC]}),
    )
    .await;
    let before = series_row(&pool, series).await.3;

    // The review screen: current vs proposed, the removed tome.
    let (status, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    let page = text(page);
    assert!(page.contains("data-series-update"), "{page}");
    for s in ["Cubitus (Dupa)", "Cubitus 40", "39"] {
        assert!(page.contains(s), "{s} missing");
    }

    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("Cubitus (Dupa)"), "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted");
    let (name, series_type, total, version) = series_row(&pool, series).await;
    assert_eq!(
        (name.as_str(), series_type.as_str(), total),
        ("Cubitus (Dupa)", "closed", Some(39))
    );
    assert_eq!(version, before + 1);
    assert_eq!(live_tome_positions(&pool, series).await, vec![39]);
}

#[sqlx::test(migrations = "./migrations")]
async fn acceptance_goes_stale_when_the_catalog_changed(pool: DbPool) {
    let f = fx(&pool).await;
    // 1. The series was edited by hand (version).
    let a = add_series(&pool, "Survivants", "open", None).await;
    let id = file(
        &f,
        json!({"series_id": a, "series_type": "closed", "total_volume_count": 5, "sources": [SRC]}),
    )
    .await;
    sqlx::query("UPDATE series SET description = 'x', version = version + 1 WHERE id = ?")
        .bind(a)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("the series was changed or deleted")
            || body.contains("la série a été modifiée ou supprimée"),
        "{body}"
    );
    assert_eq!(status_of(&pool, id).await, "stale");
    assert_eq!(series_row(&pool, a).await.1, "open");

    // 2. A tome to remove was edited since filing.
    let b = add_series(&pool, "Wayne Shelton", "open", None).await;
    let ghost = add_tome(&pool, b, 15, "WS 15").await;
    let id = file(
        &f,
        json!({"series_id": b, "series_type": "closed", "total_volume_count": 14,
               "remove_expected_tomes": [ghost], "sources": [SRC]}),
    )
    .await;
    sqlx::query("UPDATE series_expected_tomes SET title = 'WS 15 bis', version = version + 1 WHERE id = ?")
        .bind(ghost)
        .execute(&pool)
        .await
        .unwrap();
    let (_, body) = accept(&f, &pool, id).await;
    assert!(body.contains("WS 15"), "{body}");
    assert_eq!(status_of(&pool, id).await, "stale");
    assert_eq!(live_tome_positions(&pool, b).await, vec![15]);

    // 3. A title was placed above the proposed total (no version bump).
    let c = add_series(&pool, "Bernard Prince", "open", None).await;
    let id = file(
        &f,
        json!({"series_id": c, "series_type": "closed", "total_volume_count": 18, "sources": [SRC]}),
    )
    .await;
    let t = seed_title(&pool, "Bernard Prince 19").await;
    assign(&pool, t, c, 19).await;
    let (_, body) = accept(&f, &pool, id).await;
    assert!(body.contains("19"), "{body}");
    assert_eq!(status_of(&pool, id).await, "stale");
    assert_eq!(series_row(&pool, c).await.1, "open");
}

// ─── Edits and removals ─────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn lines_are_edited_and_removed_under_the_filing_rules(pool: DbPool) {
    let f = fx(&pool).await;
    let series = add_series(&pool, "Gaston Lagaffe", "open", None).await;
    let t = seed_title(&pool, "Gaston 17").await;
    assign(&pool, t, series, 17).await;
    let ghost = add_tome(&pool, series, 20, "Gaston 20").await;
    let id = file(
        &f,
        json!({"series_id": series, "series_type": "closed", "total_volume_count": 19,
               "remove_expected_tomes": [ghost], "sources": [SRC]}),
    )
    .await;
    let lines = items(&pool, id).await;
    let (update_item, removal_item) = (lines[0].0, lines[1].0);
    let post = |path: String, body: String| {
        let f = &f;
        async move { f.lib.post_form(&f.agent.app, &path, &body).await }
    };
    // A total below the owned position 17 is refused.
    let v = version_of(&pool, id).await;
    let (status, body) = post(
        format!("/proposals/{id}/items/{update_item}"),
        format!("version={v}&name=Gaston+Lagaffe&series_type=closed&total=16"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(text(body).contains("17"));
    // A valid edit is saved.
    let (status, _) = post(
        format!("/proposals/{id}/items/{update_item}"),
        format!("version={v}&name=Gaston+Lagaffe&series_type=closed&total=18"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items(&pool, id).await[0].1["total_volume_count"], 18);
    // Removing the removal line would leave tome 20 above the total.
    let v = version_of(&pool, id).await;
    let (status, body) = post(
        format!("/proposals/{id}/items/{removal_item}/remove"),
        format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(text(body).contains("20"));
    // The update line itself is never removable.
    let (status, _) = post(
        format!("/proposals/{id}/items/{update_item}/remove"),
        format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(items(&pool, id).await.len(), 2);
    // Accepted as edited.
    let (status, _) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(series_row(&pool, series).await.2, Some(18));
    assert!(live_tome_positions(&pool, series).await.is_empty());
}

// ─── Lock scope (epic 12 retro A1) ──────────────────────────────────

// An acceptance on one series holds row locks on its proposal and its
// series row: a decision on a proposal about another series — acceptance
// or rejection — must not wait for them. (Its share-locked read of the
// series' tomes also next-key locks the first tome entry of the next
// series in `uq_expected_tomes_position`, as the series_attach acceptance
// does: a tome *write* on that neighbour waits for the commit — it
// serialises, it cannot deadlock — so B here changes its total only.)
#[sqlx::test(migrations = "./migrations")]
async fn an_update_acceptance_on_one_series_does_not_lock_proposals_of_another(pool: DbPool) {
    use mybibli::services::acceptance::AcceptOutcome;
    use mybibli::services::proposals::{Decision, ProposalService};
    use mybibli::services::series_update_acceptance;
    let f = fx(&pool).await;
    let a = add_series(&pool, "Série A", "open", None).await;
    let b = add_series(&pool, "Série B", "open", None).await;
    let c = add_series(&pool, "Série C", "open", None).await;
    let ga = add_tome(&pool, a, 9, "A9").await;
    add_tome(&pool, b, 5, "B5").await;
    let on_a = file(
        &f,
        json!({"series_id": a, "series_type": "closed", "total_volume_count": 8,
               "remove_expected_tomes": [ga], "sources": [SRC]}),
    )
    .await;
    let on_b = file(
        &f,
        json!({"series_id": b, "series_type": "closed", "total_volume_count": 8,
               "sources": [SRC]}),
    )
    .await;
    let on_c = file(&f, json!({"series_id": c, "name": "Série C bis", "sources": [SRC]})).await;

    let mut tx1 = pool.begin().await.unwrap();
    let v = version_of(&pool, on_a).await;
    let outcome = series_update_acceptance::accept_in(&mut tx1, on_a, f.lib_id, v)
        .await
        .unwrap();
    assert!(matches!(outcome, AcceptOutcome::Accepted(_)), "{outcome:?}");

    let mut tx2 = pool.begin().await.unwrap();
    sqlx::query("SET SESSION innodb_lock_wait_timeout = 2")
        .execute(&mut *tx2)
        .await
        .unwrap();
    let v = version_of(&pool, on_b).await;
    let outcome = series_update_acceptance::accept_in(&mut tx2, on_b, f.lib_id, v)
        .await
        .expect("accepting an update of another series must not wait for A's locks");
    assert!(matches!(outcome, AcceptOutcome::Accepted(_)), "{outcome:?}");
    let _ = ProposalService::transition_in(
        &mut tx2,
        on_c,
        &Decision::Reject {
            by: f.lib_id,
            reason: None,
        },
    )
    .await
    .expect("rejecting a proposal on another series must not wait for A's locks");
    tx2.commit().await.unwrap();
    tx1.commit().await.unwrap();
    assert_eq!(status_of(&pool, on_a).await, "accepted");
    assert_eq!(status_of(&pool, on_b).await, "accepted");
    assert_eq!(status_of(&pool, on_c).await, "rejected");
    assert!(live_tome_positions(&pool, a).await.is_empty());
    assert_eq!(live_tome_positions(&pool, b).await, vec![5]);
    assert_eq!(series_row(&pool, b).await.2, Some(8));
}
