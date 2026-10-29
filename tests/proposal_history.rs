//! Story 11-15 (#493) — an agent's proposal history (`list_my_proposals`,
//! FR135), the resubmission delay at filing (FR136) and its Admin →
//! System setting (AR30).
//!
//! Drives the full router against a fresh `#[sqlx::test]` database. The
//! clock is simulated by back-dating `proposals.decided_at` (the delay is
//! measured against the database's `NOW()`).
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test proposal_history

mod common;

use axum::http::StatusCode;
use base64::Engine;
use chrono::NaiveDateTime;
use common::mcp::*;
use common::web::{Browser, PW_HASH, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use mybibli::services::cover_staging;
use mybibli::services::proposals::{Decision, ProposalService};
use serde_json::{Value, json};

const SRC: &str = "https://example.org/notice";
const LIST: &str = "list_my_proposals";

async fn decider(pool: &DbPool) -> u64 {
    sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES ('ph-lib', 'x', 'librarian')",
    )
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn reject(pool: &DbPool, by: u64, id: u64, reason: Option<&str>) {
    ProposalService::transition(
        pool,
        id,
        &Decision::Reject {
            by,
            reason: reason.map(str::to_string),
        },
    )
    .await
    .unwrap();
}

/// Move a decision `days` (and one minute) into the past: the simulated clock.
async fn back_date(pool: &DbPool, id: u64, days: i64) {
    sqlx::query(
        "UPDATE proposals SET decided_at = NOW() - INTERVAL ? DAY - INTERVAL 1 MINUTE WHERE id = ?",
    )
    .bind(days)
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

async fn set_delay(pool: &DbPool, days: &str) {
    sqlx::query(
        "UPDATE settings SET setting_value = ? WHERE setting_key = 'proposal_resubmission_delay_days'",
    )
    .bind(days)
    .execute(pool)
    .await
    .unwrap();
}

async fn proposal_count(pool: &DbPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM proposals")
        .fetch_one(pool)
        .await
        .unwrap()
}

fn id_of(doc: &Value) -> u64 {
    doc["proposal_id"].as_u64().expect("proposal_id")
}

fn metadata(title_id: u64, fields: Value) -> Value {
    json!({"title_id": title_id, "fields": fields, "sources": [SRC]})
}

fn create(name: &str) -> Value {
    json!({"name": name, "series_type": "open", "sources": [SRC]})
}

fn parse_utc(s: &Value) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s.as_str().expect("date"), "%Y-%m-%dT%H:%M:%SZ").unwrap()
}

// ─── History (FR135) ──────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn history_lists_the_keys_own_proposals_with_decisions(pool: DbPool) {
    let a = Agent::new(&pool).await;
    let b = Agent {
        app: a.app.clone(),
        key: seed_key(&pool, ApiKeyScope::Propose).await,
    };
    let title = seed_title(&pool, "Histoire").await;
    let by = decider(&pool).await;

    let meta = id_of(
        &a.ok(
            "propose_title_metadata",
            metadata(title, json!({"subtitle": "Tome 1", "publisher": "Dargaud"})),
        )
        .await,
    );
    let series = id_of(&a.ok("propose_series_create", create("Yakari")).await);
    let other = id_of(&b.ok("propose_series_create", create("Gaston")).await);
    reject(&pool, by, meta, Some("  Wrong edition  ")).await;

    // Everything of A, newest first; never B's.
    let page = a.ok(LIST, json!({})).await;
    assert_eq!(page["total_items"], 2, "{page}");
    assert_eq!(page["page_size"], 25);
    let items = page["items"].as_array().unwrap();
    assert_eq!(id_of(&items[0]), series);
    assert_eq!(id_of(&items[1]), meta);
    assert!(!items.iter().any(|i| id_of(i) == other));

    let pending = &items[0];
    assert_eq!(pending["status"], "pending");
    assert_eq!(pending["kind"], "series_create");
    assert!(pending["decided_at"].is_null());
    assert!(pending["rejection_reason"].is_null());
    assert!(pending["resubmittable_from"].is_null());
    assert_eq!(pending["subject"], json!({"series_name": "Yakari"}));
    assert_eq!(pending["item_count"], 1);

    let rejected = &items[1];
    assert_eq!(rejected["status"], "rejected");
    assert_eq!(rejected["kind"], "metadata");
    assert_eq!(rejected["rejection_reason"], "Wrong edition");
    assert_eq!(
        rejected["subject"],
        json!({"title_id": title, "fields": ["publisher", "subtitle"]})
    );
    assert_eq!(rejected["item_count"], 2);
    let decided = parse_utc(&rejected["decided_at"]);
    assert!(parse_utc(&rejected["filed_at"]) <= decided);
    assert_eq!(
        parse_utc(&rejected["resubmittable_from"]),
        decided + chrono::Duration::days(90)
    );

    // Filters.
    let only = a.ok(LIST, json!({"status": "rejected"})).await;
    assert_eq!(only["total_items"], 1);
    assert_eq!(id_of(&only["items"][0]), meta);
    let only = a
        .ok(LIST, json!({"kind": "series_create", "status": "pending"}))
        .await;
    assert_eq!(only["total_items"], 1);
    assert_eq!(id_of(&only["items"][0]), series);
    let none = a.ok(LIST, json!({"status": "accepted"})).await;
    assert_eq!(none["total_items"], 0);
    let msg = a.err(LIST, json!({"status": "refused"})).await;
    assert!(
        msg.contains("Invalid arguments") && msg.contains("pending, accepted"),
        "{msg}"
    );

    // B sees its own only; a stale proposal carries its decision date.
    ProposalService::transition(&pool, other, &Decision::MarkStale)
        .await
        .unwrap();
    let page = b.ok(LIST, json!({})).await;
    assert_eq!(page["total_items"], 1);
    assert_eq!(page["items"][0]["status"], "stale");
    assert!(page["items"][0]["decided_at"].is_string());
    assert!(page["items"][0]["resubmittable_from"].is_null());
}

#[sqlx::test(migrations = "./migrations")]
async fn history_tool_is_listed_read_only(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let list = send_json(
        &agent.app,
        Call::new(
            &agent.key,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        )
        .version(V_LEGACY),
    )
    .await;
    let tool = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == LIST)
        .expect("list_my_proposals listed")
        .clone();
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
}

// ─── Resubmission delay (FR136) ───────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn metadata_same_title_and_field_set_is_refused_inside_the_delay(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let title = seed_title(&pool, "Délai").await;
    let by = decider(&pool).await;

    let first = id_of(
        &agent
            .ok(
                "propose_title_metadata",
                metadata(title, json!({"subtitle": "A"})),
            )
            .await,
    );
    reject(&pool, by, first, Some("Not on the cover")).await;

    // Same title, same set of fields (another value): refused, nothing filed.
    let msg = agent
        .err(
            "propose_title_metadata",
            metadata(title, json!({"subtitle": "B"})),
        )
        .await;
    assert!(msg.contains("was rejected on"), "{msg}");
    assert!(msg.contains(&format!("your proposal {first}")), "{msg}");
    assert!(msg.contains("Not on the cover"), "{msg}");
    assert!(msg.contains("again from"), "{msg}");
    assert!(msg.contains("90 days"), "{msg}");
    let decided: NaiveDateTime =
        sqlx::query_scalar("SELECT CAST(decided_at AS DATETIME) FROM proposals WHERE id = ?")
            .bind(first)
            .fetch_one(&pool)
            .await
            .unwrap();
    let reopen = (decided + chrono::Duration::days(90))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    assert!(msg.contains(&reopen), "{msg} should name {reopen}");
    assert_eq!(proposal_count(&pool).await, 1);

    // Another set of fields or another title: filed.
    agent
        .ok(
            "propose_title_metadata",
            metadata(title, json!({"subtitle": "B", "publisher": "P"})),
        )
        .await;
    let other_title = seed_title(&pool, "Autre").await;
    agent
        .ok(
            "propose_title_metadata",
            metadata(other_title, json!({"subtitle": "B"})),
        )
        .await;

    // Simulated clock: 89 days later still refused, 90 days later filed.
    back_date(&pool, first, 89).await;
    agent
        .err(
            "propose_title_metadata",
            metadata(title, json!({"subtitle": "B"})),
        )
        .await;
    back_date(&pool, first, 90).await;
    agent
        .ok(
            "propose_title_metadata",
            metadata(title, json!({"subtitle": "B"})),
        )
        .await;
    assert_eq!(proposal_count(&pool).await, 4);
}

#[sqlx::test(migrations = "./migrations")]
async fn series_equivalence_by_normalised_name_and_by_series_id(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let by = decider(&pool).await;

    let created = id_of(
        &agent
            .ok("propose_series_create", create("Les Schtroumpfs"))
            .await,
    );
    reject(&pool, by, created, None).await;
    let msg = agent
        .err("propose_series_create", create("  LES   SCHTRÖUMPFS "))
        .await;
    assert!(msg.contains("creation of the series"), "{msg}");
    assert!(msg.contains("no reason given"), "{msg}");
    agent
        .ok("propose_series_create", create("Johan et Pirlouit"))
        .await;

    let series: u64 =
        sqlx::query("INSERT INTO series (name, series_type) VALUES ('Spirou', 'open')")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_id();
    let attach = |pos: i64| json!({"series_id": series, "expected_tomes": [{"position": pos, "title": format!("S{pos}")}], "sources": [SRC]});
    let first = id_of(&agent.ok("propose_series_attach", attach(1)).await);
    reject(&pool, by, first, Some("Wrong series")).await;
    // Other lines, same series: equivalent.
    let msg = agent.err("propose_series_attach", attach(2)).await;
    assert!(
        msg.contains(&format!("additions to series {series}")),
        "{msg}"
    );
    back_date(&pool, first, 90).await;
    agent.ok("propose_series_attach", attach(2)).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn only_live_rejections_block(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let title = seed_title(&pool, "Statuts").await;
    let by = decider(&pool).await;
    let file = || {
        agent.ok(
            "propose_title_metadata",
            metadata(title, json!({"publisher": "X"})),
        )
    };

    // Pending, stale and accepted proposals never block.
    let p1 = id_of(&file().await);
    let p2 = id_of(&file().await);
    ProposalService::transition(&pool, p2, &Decision::MarkStale)
        .await
        .unwrap();
    let p3 = id_of(&file().await);
    ProposalService::transition(&pool, p3, &Decision::Accept { by })
        .await
        .unwrap();
    // A soft-deleted rejection never blocks either.
    reject(&pool, by, p1, None).await;
    sqlx::query("UPDATE proposals SET deleted_at = NOW() WHERE id = ?")
        .bind(p1)
        .execute(&pool)
        .await
        .unwrap();
    file().await;
}

#[sqlx::test(migrations = "./migrations")]
async fn another_keys_rejection_blocks_without_being_shown(pool: DbPool) {
    let a = Agent::new(&pool).await;
    let b = Agent {
        app: a.app.clone(),
        key: seed_key(&pool, ApiKeyScope::Propose).await,
    };
    let title = seed_title(&pool, "Clés").await;
    let by = decider(&pool).await;
    let first = id_of(
        &a.ok(
            "propose_title_metadata",
            metadata(title, json!({"dewey_code": "843"})),
        )
        .await,
    );
    reject(&pool, by, first, Some("private note")).await;
    let msg = b
        .err(
            "propose_title_metadata",
            metadata(title, json!({"dewey_code": "843.9"})),
        )
        .await;
    assert!(msg.contains("was rejected on"), "{msg}");
    assert!(!msg.contains("your proposal"), "{msg}");
    assert!(!msg.contains("private note"), "{msg}");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_setting_applies_to_the_next_filing(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let title = seed_title(&pool, "Réglage").await;
    let by = decider(&pool).await;
    let first = id_of(
        &agent
            .ok(
                "propose_title_metadata",
                metadata(title, json!({"subtitle": "A"})),
            )
            .await,
    );
    reject(&pool, by, first, None).await;
    back_date(&pool, first, 10).await;

    // 30 days: still inside.
    set_delay(&pool, "30").await;
    let msg = agent
        .err(
            "propose_title_metadata",
            metadata(title, json!({"subtitle": "B"})),
        )
        .await;
    assert!(msg.contains("30 days"), "{msg}");
    // 10 days: just elapsed.
    set_delay(&pool, "10").await;
    agent
        .ok(
            "propose_title_metadata",
            metadata(title, json!({"subtitle": "B"})),
        )
        .await;
    // 0 disables the rule, even for a fresh rejection.
    let second = id_of(
        &agent
            .ok(
                "propose_title_metadata",
                metadata(title, json!({"description": "D"})),
            )
            .await,
    );
    reject(&pool, by, second, None).await;
    set_delay(&pool, "0").await;
    agent
        .ok(
            "propose_title_metadata",
            metadata(title, json!({"description": "E"})),
        )
        .await;
    // An unreadable stored value reads as the default (90 days).
    set_delay(&pool, "soon").await;
    agent
        .err(
            "propose_title_metadata",
            metadata(title, json!({"description": "F"})),
        )
        .await;
}

#[sqlx::test(migrations = "./migrations")]
async fn a_cover_for_the_same_title_is_refused_and_nothing_stays_staged(pool: DbPool) {
    let dir = tempfile::tempdir().unwrap();
    let covers = dir.path().join("covers");
    let agent = Agent {
        app: router_with_covers(&pool, &covers),
        key: seed_key(&pool, ApiKeyScope::Propose).await,
    };
    let staging = cover_staging::staging_dir_for(&covers);
    let title = seed_title(&pool, "Couverture").await;
    let by = decider(&pool).await;
    let png = |w: u32| {
        let mut out = Vec::new();
        image::DynamicImage::new_rgb8(w, w)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        json!({
            "title_id": title,
            "image_base64": base64::engine::general_purpose::STANDARD.encode(&out),
            "media_type": "image/png",
            "sources": [SRC]
        })
    };
    let first = id_of(&agent.ok("propose_cover", png(4)).await);
    reject(&pool, by, first, None).await;
    let staged_before = std::fs::read_dir(&staging).unwrap().count();
    let msg = agent.err("propose_cover", png(5)).await;
    assert!(msg.contains(&format!("cover of title {title}")), "{msg}");
    assert_eq!(std::fs::read_dir(&staging).unwrap().count(), staged_before);
    assert_eq!(proposal_count(&pool).await, 1);
}

// ─── Admin → System setting (AR30) ────────────────────────────────

async fn delay_row(pool: &DbPool) -> (String, i32) {
    sqlx::query_as(
        "SELECT setting_value, version FROM settings WHERE setting_key = 'proposal_resubmission_delay_days'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_sets_the_delay_in_admin_system(pool: DbPool) {
    assert_eq!(
        delay_row(&pool).await,
        ("90".to_string(), 1),
        "seeded default"
    );
    seed_admin(&pool, "ph_admin").await;
    let app = router(&pool);
    let admin = Browser::login(&app, "ph_admin").await;

    let (status, html) = admin.get(&app, "/admin?tab=system").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        html.contains("id=\"admin-system-proposals-form\""),
        "{html}"
    );
    assert!(html.contains("name=\"resubmission_delay_days\""));

    let (status, body) = admin
        .post_form(
            &app,
            "/admin/system/proposals",
            "resubmission_delay_days=30&resubmission_delay_version=1&_csrf_token=x",
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("value=\"30\""), "{body}");
    assert!(
        body.contains("name=\"resubmission_delay_version\" value=\"2\""),
        "{body}"
    );
    assert_eq!(delay_row(&pool).await, ("30".to_string(), 2));
    // The panel reads the saved row back (value and version).
    let (_, html) = admin.get(&app, "/admin?tab=system").await;
    assert!(
        html.contains("name=\"resubmission_delay_version\" value=\"2\""),
        "{html}"
    );

    // Out of range: 400, the typed value re-rendered, the row unchanged.
    let (status, body) = admin
        .post_form(
            &app,
            "/admin/system/proposals",
            "resubmission_delay_days=4000&resubmission_delay_version=2&_csrf_token=x",
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("value=\"4000\""), "{body}");
    assert_eq!(delay_row(&pool).await, ("30".to_string(), 2));

    // A stale version is a conflict.
    let (status, _) = admin
        .post_form(
            &app,
            "/admin/system/proposals",
            "resubmission_delay_days=0&resubmission_delay_version=1&_csrf_token=x",
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(delay_row(&pool).await, ("30".to_string(), 2));

    // 0 is allowed (no delay).
    let (status, _) = admin
        .post_form(
            &app,
            "/admin/system/proposals",
            "resubmission_delay_days=0&resubmission_delay_version=2&_csrf_token=x",
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(delay_row(&pool).await, ("0".to_string(), 3));
}

#[sqlx::test(migrations = "./migrations")]
async fn only_an_admin_may_set_the_delay(pool: DbPool) {
    seed_admin(&pool, "ph_admin").await;
    sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES ('ph_librarian', ?, 'librarian')",
    )
    .bind(PW_HASH)
    .execute(&pool)
    .await
    .unwrap();
    let app = router(&pool);
    let body = "resubmission_delay_days=5&resubmission_delay_version=1&_csrf_token=x";

    let librarian = Browser::login(&app, "ph_librarian").await;
    let (status, _) = librarian
        .post_form(&app, "/admin/system/proposals", body)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let anonymous = Browser::anonymous(&app).await;
    let res = anonymous
        .send(&app, "POST", "/admin/system/proposals", body, false)
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(delay_row(&pool).await, ("90".to_string(), 1));
}
