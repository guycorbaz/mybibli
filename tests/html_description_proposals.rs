//! Story 13-4 (#578), review round 2 (M1) — the run-once repair of HTML
//! descriptions does not outdate pending agent proposals.
//!
//! Production held ~370 pending proposals (series attach/create, metadata)
//! on titles whose description the repair converts: the repair must not
//! bump the title's `version` (series proposals compare it), and it
//! rewrites the `current` / `proposed` description recorded in pending
//! metadata items so `FieldChanged` does not fire and no HTML lands at
//! acceptance. Proposals are filed through the real MCP tools and accepted
//! through the real router.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test html_description_proposals

mod common;

use axum::http::StatusCode;
use base64::Engine;
use common::mcp::*;
use common::web::{Browser, PW_HASH, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use mybibli::services::description_repair::repair_html_descriptions_once;
use serde_json::{Value, json};

const SRC: &str = "https://example.org/notice";
const HTML: &str = "<p><b>Grandis un peu, Titeuf !</b></p><p>La vie &eacute;volue.</p>";
const PLAIN: &str = "Grandis un peu, Titeuf !\n\nLa vie évolue.";

struct Fx {
    _dir: tempfile::TempDir,
    agent: Agent,
    lib: Browser,
}

async fn fx(pool: &DbPool) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let agent = Agent {
        app: router_with_covers(pool, &dir.path().join("covers")),
        key: seed_key(pool, ApiKeyScope::Propose).await,
    };
    seed_admin(pool, "hd-admin").await;
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
        .bind("hd-lib")
        .bind(PW_HASH)
        .execute(pool)
        .await
        .unwrap();
    let lib = Browser::login(&agent.app, "hd-lib").await;
    Fx {
        _dir: dir,
        agent,
        lib,
    }
}

/// A title carrying a provider's HTML description, as before v1.23.0.
async fn html_title(pool: &DbPool, name: &str) -> u64 {
    let id = seed_title(pool, name).await;
    sqlx::query("UPDATE titles SET description = ? WHERE id = ?")
        .bind(HTML)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    id
}

async fn file(f: &Fx, tool: &str, args: Value) -> u64 {
    f.agent.ok(tool, args).await["proposal_id"]
        .as_u64()
        .expect("proposal_id")
}

async fn accept(f: &Fx, pool: &DbPool, id: u64) -> (StatusCode, String) {
    let v: i32 = sqlx::query_scalar("SELECT version FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    f.lib
        .post_form(
            &f.agent.app,
            &format!("/proposals/{id}/accept"),
            &format!("version={v}"),
        )
        .await
}

async fn status_of(pool: &DbPool, id: u64) -> String {
    sqlx::query_scalar("SELECT status FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn description_of(pool: &DbPool, id: u64) -> Option<String> {
    sqlx::query_scalar("SELECT description FROM titles WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_series_attach_stays_acceptable(pool: DbPool) {
    let f = fx(&pool).await;
    let sid = sqlx::query(
        "INSERT INTO series (name, series_type, total_volume_count) VALUES ('Titeuf', 'closed', 5)",
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id();
    let t = html_title(&pool, "Titeuf 1").await;
    let id = file(
        &f,
        "propose_series_attach",
        json!({"series_id": sid, "titles": [{"title_id": t, "position": 1}], "sources": [SRC]}),
    )
    .await;

    let report = repair_html_descriptions_once(&pool)
        .await
        .unwrap()
        .expect("first run");
    assert_eq!(report.converted, 1);
    assert_eq!(description_of(&pool, t).await.as_deref(), Some(PLAIN));

    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted", "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_description_proposal_is_repaired_and_stays_acceptable(pool: DbPool) {
    let f = fx(&pool).await;
    let t = html_title(&pool, "Titeuf 2").await;
    let id = file(
        &f,
        "propose_title_metadata",
        json!({"title_id": t, "fields": {"description": "Placeholder"}, "sources": [SRC]}),
    )
    .await;
    // As filed before the deploy: the agent's description still in HTML.
    sqlx::query(
        "UPDATE proposal_items SET proposed_values = \
         JSON_SET(proposed_values, '$.proposed', ?) WHERE proposal_id = ?",
    )
    .bind("<p>Nouvelle <i>description</i> &amp; suite</p>")
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    let report = repair_html_descriptions_once(&pool)
        .await
        .unwrap()
        .expect("first run");
    assert_eq!((report.items_current, report.items_proposed), (1, 1));

    let raw: String = sqlx::query_scalar(
        "SELECT CAST(proposed_values AS CHAR) FROM proposal_items WHERE proposal_id = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let item: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(item["current"], PLAIN);
    assert_eq!(item["proposed"], "Nouvelle description & suite");

    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted", "not stale: {body}");
    assert_eq!(
        description_of(&pool, t).await.as_deref(),
        Some("Nouvelle description & suite")
    );
}

/// Runs the repair and checks it converted the one HTML title.
async fn repair(pool: &DbPool) -> mybibli::services::description_repair::DescriptionCleanup {
    let report = repair_html_descriptions_once(pool)
        .await
        .unwrap()
        .expect("first run");
    assert_eq!(report.converted, 1);
    report
}

/// The `description` item of a proposal.
async fn description_item(pool: &DbPool, id: u64) -> Value {
    let raw: String = sqlx::query_scalar(
        "SELECT CAST(proposed_values AS CHAR) FROM proposal_items \
         WHERE proposal_id = ? AND JSON_UNQUOTE(JSON_EXTRACT(proposed_values, '$.field')) = 'description'",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap();
    serde_json::from_str(&raw).unwrap()
}

/// Review round 3 (L4) — a proposal with a description line and another
/// field: both lines are still accepted after the repair.
#[sqlx::test(migrations = "./migrations")]
async fn a_description_and_publisher_proposal_stays_acceptable(pool: DbPool) {
    let f = fx(&pool).await;
    let t = html_title(&pool, "Titeuf 3").await;
    let id = file(
        &f,
        "propose_title_metadata",
        json!({"title_id": t,
               "fields": {"description": "Nouveau texte", "publisher": "Glénat"},
               "sources": [SRC]}),
    )
    .await;
    let report = repair(&pool).await;
    assert_eq!((report.items_current, report.items_proposed), (1, 0));

    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted", "{body}");
    let (desc, publisher): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT description, publisher FROM titles WHERE id = ?")
            .bind(t)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(desc.as_deref(), Some("Nouveau texte"));
    assert_eq!(publisher.as_deref(), Some("Glénat"));
}

/// Review round 3 (L4) — a line a librarian edited keeps the value typed
/// (hand-typed text is never converted); its `current` still follows the
/// converted title, so the proposal stays acceptable.
#[sqlx::test(migrations = "./migrations")]
async fn a_librarian_edited_line_keeps_its_proposed_value(pool: DbPool) {
    let f = fx(&pool).await;
    let t = html_title(&pool, "Titeuf 4").await;
    let id = file(
        &f,
        "propose_title_metadata",
        json!({"title_id": t, "fields": {"description": "Agent"}, "sources": [SRC]}),
    )
    .await;
    // As the 11-19 edit leaves it: values replaced, originals kept, marked.
    sqlx::query(
        "UPDATE proposal_items SET original_values = proposed_values, \
         proposed_values = JSON_SET(proposed_values, '$.proposed', ?), \
         edited_by_name = 'hd-lib', edited_at = NOW() WHERE proposal_id = ?",
    )
    .bind("Tapé <b>à la main</b>")
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    let report = repair(&pool).await;
    assert_eq!((report.items_current, report.items_proposed), (1, 0));
    let item = description_item(&pool, id).await;
    assert_eq!(item["proposed"], "Tapé <b>à la main</b>");
    assert_eq!(item["current"], PLAIN);

    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted", "{body}");
    assert_eq!(
        description_of(&pool, t).await.as_deref(),
        Some("Tapé <b>à la main</b>")
    );
}

fn png() -> Vec<u8> {
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        40,
        60,
        image::Rgb([90, 90, 90]),
    ))
    .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
    .unwrap();
    out
}

/// Review round 3 (L4) — a pending cover proposal on a converted title
/// stays acceptable.
#[sqlx::test(migrations = "./migrations")]
async fn a_pending_cover_proposal_stays_acceptable(pool: DbPool) {
    let f = fx(&pool).await;
    let t = html_title(&pool, "Titeuf 5").await;
    let id = file(
        &f,
        "propose_cover",
        json!({
            "title_id": t,
            "image_base64": base64::engine::general_purpose::STANDARD.encode(png()),
            "media_type": "image/png",
            "sources": [SRC]
        }),
    )
    .await;
    repair(&pool).await;

    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted", "{body}");
}
