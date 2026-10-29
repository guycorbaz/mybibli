//! Story 11-21 (#493) — access to the proposals page: the "Proposals"
//! nav entry with its pending-count badge and the "Pending proposals"
//! dashboard indicator (FR143, UX-DR32, UX-DR33).
//!
//! Drives the full router (session middleware included) against a fresh
//! `#[sqlx::test]` database. The test instance renders in French.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test proposals_access

mod common;

use axum::http::StatusCode;
use common::mcp::*;
use common::web::{Browser, PW_HASH, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::proposal::ProposalModel;
use serde_json::json;

const SRC: &str = "https://example.org/notice";

async fn librarian(pool: &DbPool, agent: &Agent) -> Browser {
    seed_admin(pool, "pa-admin").await;
    sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES ('pa-lib', ?, 'librarian')",
    )
    .bind(PW_HASH)
    .execute(pool)
    .await
    .unwrap();
    Browser::login(&agent.app, "pa-lib").await
}

async fn file_metadata(agent: &Agent, title_id: u64, subtitle: &str) -> u64 {
    agent
        .ok(
            "propose_title_metadata",
            json!({"title_id": title_id, "fields": {"subtitle": subtitle}, "sources": [SRC]}),
        )
        .await["proposal_id"]
        .as_u64()
        .expect("proposal_id")
}

/// The strip entry's opening tag (the first `/proposals` link: the
/// desktop strip comes before the mobile panel).
fn entry_tag(html: &str) -> &str {
    let start = html
        .find("<a href=\"/proposals\"")
        .expect("Proposals entry");
    let end = html[start..].find('>').unwrap() + start + 1;
    &html[start..end]
}

#[sqlx::test(migrations = "./migrations")]
async fn no_badge_and_no_indicator_without_pending_proposals(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let lib = librarian(&pool, &agent).await;
    let (status, html) = lib.get(&agent.app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        html.matches("<a href=\"/proposals\"").count(),
        2,
        "strip + panel"
    );
    assert!(html.contains(">Propositions</a>"));
    assert!(!entry_tag(&html).contains("aria-label="));
    assert!(!html.contains("data-proposals-badge"));
    assert!(!html.contains("filter-tag-pending-proposals"));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_pending_proposal_shows_the_badge_and_the_indicator(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let lib = librarian(&pool, &agent).await;
    let a = seed_title(&pool, "Badge A").await;
    let b = seed_title(&pool, "Badge B").await;
    file_metadata(&agent, a, "Un").await;
    file_metadata(&agent, b, "Deux").await;

    let (_, html) = lib.get(&agent.app, "/").await;
    assert!(
        entry_tag(&html).contains("aria-label=\"Propositions, 2 en attente\""),
        "{}",
        entry_tag(&html)
    );
    assert_eq!(
        html.matches("data-proposals-badge").count(),
        2,
        "strip + panel badge"
    );
    assert!(html.contains(">2</span></a>"));
    assert!(html.contains("<a href=\"/proposals\" id=\"filter-tag-pending-proposals\""));
    assert!(html.contains("aria-label=\"Propositions en attente: 2\""));

    // Any page carries the badge, not only the dashboard.
    let (_, html) = lib.get(&agent.app, "/catalog").await;
    assert!(entry_tag(&html).contains("Propositions, 2 en attente"));
    assert!(!html.contains("filter-tag-pending-proposals"));
}

#[sqlx::test(migrations = "./migrations")]
async fn the_badge_is_up_to_date_after_a_decision(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let lib = librarian(&pool, &agent).await;
    let title = seed_title(&pool, "Décision").await;
    let id = file_metadata(&agent, title, "Refusé").await;
    let (_, html) = lib.get(&agent.app, "/").await;
    assert!(html.contains("data-proposals-badge"));

    let version: i32 = sqlx::query_scalar("SELECT version FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let res = lib
        .send(
            &agent.app,
            "POST",
            &format!("/proposals/{id}/reject"),
            &format!("version={version}&reason="),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK, "{}", body_string(res).await);

    let (_, html) = lib.get(&agent.app, "/").await;
    assert!(
        !html.contains("data-proposals-badge"),
        "next navigation: no badge"
    );
    assert!(!html.contains("filter-tag-pending-proposals"));
}

/// D-11-21-2 — a pending proposal whose lines were all removed (11-19)
/// still awaits a decision and counts; decided, stale and deleted
/// proposals do not.
#[sqlx::test(migrations = "./migrations")]
async fn only_live_pending_proposals_count_even_without_lines(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let titles = [
        seed_title(&pool, "C1").await,
        seed_title(&pool, "C2").await,
        seed_title(&pool, "C3").await,
        seed_title(&pool, "C4").await,
        seed_title(&pool, "C5").await,
    ];
    let mut ids = Vec::new();
    for (i, t) in titles.iter().enumerate() {
        ids.push(file_metadata(&agent, *t, &format!("S{i}")).await);
    }
    assert_eq!(ProposalModel::count_pending(&pool).await.unwrap(), 5);

    // ids[0]: every line removed — still pending, still counted.
    sqlx::query("UPDATE proposal_items SET deleted_at = NOW() WHERE proposal_id = ?")
        .bind(ids[0])
        .execute(&pool)
        .await
        .unwrap();
    // ids[1..4]: decided, stale, soft-deleted.
    for (id, status) in [
        (ids[1], "accepted"),
        (ids[2], "rejected"),
        (ids[3], "stale"),
    ] {
        sqlx::query("UPDATE proposals SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE proposals SET deleted_at = NOW() WHERE id = ?")
        .bind(ids[4])
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(ProposalModel::count_pending(&pool).await.unwrap(), 1);

    let lib = librarian(&pool, &agent).await;
    let (_, html) = lib.get(&agent.app, "/").await;
    assert!(entry_tag(&html).contains("Propositions, 1 en attente"));
}

#[sqlx::test(migrations = "./migrations")]
async fn the_badge_caps_at_99_plus_and_the_name_keeps_the_count(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    let title = seed_title(&pool, "Beaucoup").await;
    let first = file_metadata(&agent, title, "Premier").await;
    let key_id: u64 = sqlx::query_scalar("SELECT api_key_id FROM proposals WHERE id = ?")
        .bind(first)
        .fetch_one(&pool)
        .await
        .unwrap();
    for _ in 0..104 {
        sqlx::query("INSERT INTO proposals (kind, api_key_id, sources) VALUES ('cover', ?, '[]')")
            .bind(key_id)
            .execute(&pool)
            .await
            .unwrap();
    }
    let lib = librarian(&pool, &agent).await;
    let (_, html) = lib.get(&agent.app, "/").await;
    assert!(entry_tag(&html).contains("Propositions, 105 en attente"));
    assert!(html.contains(">99+</span></a>"));
    assert!(!html.contains(">105</span></a>"));
    // The dashboard pill shows the exact number.
    assert!(html.contains("aria-label=\"Propositions en attente: 105\""));
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_visitors_see_neither_entry_nor_indicator(pool: DbPool) {
    let agent = Agent::new(&pool).await;
    seed_admin(&pool, "pa-admin").await;
    let title = seed_title(&pool, "Anonyme").await;
    file_metadata(&agent, title, "Caché").await;
    let anon = Browser::anonymous(&agent.app).await;
    let (_, html) = anon.get(&agent.app, "/").await;
    assert!(!html.contains("/proposals"));
    assert!(!html.contains("data-proposals-badge"));
    assert!(!html.contains("filter-tag-pending-proposals"));
}
