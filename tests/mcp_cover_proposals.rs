//! Story 11-14 (#493, #522) — the MCP tool `propose_cover`, the cover
//! staging area and its purge.
//!
//! Drives the full router against a fresh `#[sqlx::test]` database and a
//! private covers directory: a valid image (item, staged re-encoded
//! file), refused images (nothing staged, nothing filed), a replacement,
//! an unknown title, an unavailable staging area, key revocation through
//! Admin → API keys (stale + staged file gone), the scheduled purge,
//! promote / discard, and the `/covers` route never serving the staging
//! area.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test mcp_cover_proposals

mod common;

use std::path::Path;
use std::time::{Duration, SystemTime};

use axum::http::StatusCode;
use base64::Engine;
use common::mcp::*;
use common::web::{Browser, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use mybibli::models::proposal::{ProposalKind, ProposalStatus, ProposalTargetTable};
use mybibli::services::cover::MAX_PROPOSED_COVER_BYTES;
use mybibli::services::cover_proposals::MAX_PENDING_COVERS_PER_KEY;
use mybibli::services::cover_staging::{self, PURGE_GRACE, StagingError};
use mybibli::services::proposals::{Decision, ProposalService};
use serde_json::{Value, json};
use tower::ServiceExt;

const SRC: &str = "https://example.org/couverture";
const TOOL: &str = "propose_cover";

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::DynamicImage::new_rgb8(w, h)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn args(title_id: u64, image: &[u8], media_type: &str) -> Value {
    json!({
        "title_id": title_id,
        "image_base64": b64(image),
        "media_type": media_type,
        "sources": [SRC]
    })
}

struct Fixture {
    _dir: tempfile::TempDir,
    covers: std::path::PathBuf,
    staging: std::path::PathBuf,
    agent: Agent,
}

async fn fixture(pool: &DbPool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let covers = dir.path().join("covers");
    let app = router_with_covers(pool, &covers);
    Fixture {
        staging: cover_staging::staging_dir_for(&covers),
        covers,
        _dir: dir,
        agent: Agent {
            app,
            key: seed_key(pool, ApiKeyScope::Propose).await,
        },
    }
}

fn staged_files(staging: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(staging)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    v.sort();
    v
}

async fn proposal_count(pool: &DbPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM proposals")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn staged_file_of(pool: &DbPool, proposal_id: u64) -> String {
    let detail = ProposalService::detail(pool, proposal_id)
        .await
        .unwrap()
        .unwrap();
    detail.items[0].proposed_values["staged_file"]
        .as_str()
        .unwrap()
        .to_string()
}

#[sqlx::test(migrations = "./migrations")]
async fn a_valid_image_is_staged_reencoded_and_filed(pool: DbPool) {
    let f = fixture(&pool).await;
    let title = seed_title(&pool, "Sans couverture").await;
    let original = png(800, 1200);

    let res = f.agent.ok(TOOL, args(title, &original, "image/png")).await;
    assert_eq!(res["status"], "pending");
    assert_eq!(res["kind"], "cover");
    assert_eq!(res["item_count"], 1);
    assert_eq!(res["replaces_existing"], false);
    assert!(
        res.get("staged_file").is_none(),
        "staging layout stays internal"
    );

    let id = res["proposal_id"].as_u64().unwrap();
    let detail = ProposalService::detail(&pool, id).await.unwrap().unwrap();
    assert_eq!(detail.proposal.kind, ProposalKind::Cover);
    assert_eq!(detail.proposal.status, ProposalStatus::Pending);
    assert_eq!(detail.proposal.sources, vec![SRC.to_string()]);
    assert_eq!(detail.items.len(), 1);
    let item = &detail.items[0];
    let target = item.target.unwrap();
    assert_eq!(
        (target.table, target.id),
        (ProposalTargetTable::Titles, title)
    );
    let v = &item.proposed_values;
    assert_eq!(v["line"], "cover");
    assert_eq!(v["replaces_existing"], false);
    assert_eq!(v["current"], Value::Null);
    assert_eq!(v["media_type"], "image/png");
    assert_eq!(v["original_bytes"], original.len());
    assert_eq!(
        (v["width"].as_u64(), v["height"].as_u64()),
        (Some(400), Some(600))
    );

    let name = v["staged_file"].as_str().unwrap();
    assert!(cover_staging::is_staged_name(name));
    assert_eq!(staged_files(&f.staging), vec![name.to_string()]);
    let staged = std::fs::read(f.staging.join(name)).unwrap();
    assert_eq!(staged.len() as u64, v["bytes"].as_u64().unwrap());
    assert_eq!(
        mybibli::services::cover::detected_cover_media_type(&staged),
        Some("image/jpeg"),
        "the staged file is the re-encoded JPEG"
    );
    // The catalog is untouched.
    let cover: Option<String> =
        sqlx::query_scalar("SELECT cover_image_url FROM titles WHERE id = ?")
            .bind(title)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(cover, None);
    assert!(!f.covers.join(format!("{title}.jpg")).exists());
}

#[sqlx::test(migrations = "./migrations")]
async fn refused_images_leave_nothing_behind(pool: DbPool) {
    let f = fixture(&pool).await;
    let title = seed_title(&pool, "Refus").await;
    let png_bytes = png(4, 4);
    let mut oversized = vec![0u8; MAX_PROPOSED_COVER_BYTES + 1];
    oversized[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut broken_png = png_bytes.clone();
    broken_png.truncate(20);

    let cases: Vec<(Value, &str)> = vec![
        (
            args(title, b"plain text, not an image", "image/png"),
            "not a JPEG, PNG, WebP or GIF",
        ),
        (args(title, &oversized, "image/png"), "larger than"),
        (
            args(title, &png_bytes, "image/jpeg"),
            "but the bytes are image/png",
        ),
        (args(title, &png_bytes, "image/bmp"), "is not accepted"),
        (args(title, &broken_png, "image/png"), "cannot be read"),
        (
            json!({"title_id": title, "image_base64": format!("data:image/png;base64,{}", b64(&png_bytes)),
                   "media_type": "image/png", "sources": [SRC]}),
            "without a `data:",
        ),
        (
            json!({"title_id": title, "image_base64": "%%%not-base64%%%",
                   "media_type": "image/png", "sources": [SRC]}),
            "not valid base64",
        ),
        (
            json!({"title_id": title, "image_base64": b64(&png_bytes),
                   "media_type": "image/png", "sources": ["ftp://example.org/x"]}),
            "Invalid arguments",
        ),
    ];
    for (a, expected) in cases {
        let msg = f.agent.err(TOOL, a).await;
        assert!(msg.contains(expected), "expected {expected:?} in {msg}");
        assert!(msg.starts_with("Invalid arguments"), "{msg}");
    }
    assert_eq!(proposal_count(&pool).await, 0);
    assert!(staged_files(&f.staging).is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_title_with_a_cover_gets_a_replacement_proposal(pool: DbPool) {
    let f = fixture(&pool).await;
    let title = seed_title(&pool, "Avec couverture").await;
    sqlx::query("UPDATE titles SET cover_image_url = ?, version = version + 1 WHERE id = ?")
        .bind(format!("/covers/{title}.jpg?v=12"))
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    let version: i32 = sqlx::query_scalar("SELECT version FROM titles WHERE id = ?")
        .bind(title)
        .fetch_one(&pool)
        .await
        .unwrap();

    let mut a = args(title, &png(30, 40), "image/png");
    a["version"] = json!(version);
    let res = f.agent.ok(TOOL, a).await;
    assert_eq!(res["replaces_existing"], true);
    let id = res["proposal_id"].as_u64().unwrap();
    let detail = ProposalService::detail(&pool, id).await.unwrap().unwrap();
    let v = &detail.items[0].proposed_values;
    assert_eq!(v["replaces_existing"], true);
    assert_eq!(v["current"], format!("/covers/{title}.jpg?v=12"));
    assert_eq!(detail.items[0].target_version, Some(version));

    // A stale pin is refused, nothing staged for it.
    let mut stale = args(title, &png(30, 40), "image/png");
    stale["version"] = json!(version - 1);
    let msg = f.agent.err(TOOL, stale).await;
    assert!(msg.contains("changed since you read it"), "{msg}");
    assert_eq!(staged_files(&f.staging).len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn unknown_title_and_missing_staging_are_refused(pool: DbPool) {
    let f = fixture(&pool).await;
    let msg = f
        .agent
        .err(TOOL, args(999_999, &png(2, 2), "image/png"))
        .await;
    assert!(msg.contains("No title with id 999999"), "{msg}");

    let title = seed_title(&pool, "Sans staging").await;
    std::fs::remove_dir(&f.staging).unwrap();
    let msg = f
        .agent
        .err(TOOL, args(title, &png(2, 2), "image/png"))
        .await;
    assert!(msg.contains("cannot receive cover proposals"), "{msg}");
    assert_eq!(proposal_count(&pool).await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn revoking_the_key_stales_its_proposals_and_discards_their_covers(pool: DbPool) {
    let f = fixture(&pool).await;
    let title = seed_title(&pool, "Révocation").await;
    let res = f
        .agent
        .ok(TOOL, args(title, &png(10, 10), "image/png"))
        .await;
    let id = res["proposal_id"].as_u64().unwrap();
    let staged = staged_file_of(&pool, id).await;
    assert!(f.staging.join(&staged).exists());

    // A proposal filed by another key is untouched.
    let other = Agent {
        app: f.agent.app.clone(),
        key: seed_key(&pool, ApiKeyScope::Propose).await,
    };
    let other_res = other.ok(TOOL, args(title, &png(12, 12), "image/png")).await;
    let other_id = other_res["proposal_id"].as_u64().unwrap();
    let other_staged = staged_file_of(&pool, other_id).await;

    let key_id: u64 = sqlx::query_scalar("SELECT api_key_id FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let version: i32 = sqlx::query_scalar("SELECT version FROM api_keys WHERE id = ?")
        .bind(key_id)
        .fetch_one(&pool)
        .await
        .unwrap();

    seed_admin(&pool, "revoker").await;
    let browser = Browser::login(&f.agent.app, "revoker").await;
    let (status, body) = browser
        .post_form(
            &f.agent.app,
            &format!("/admin/api-keys/{key_id}/revoke"),
            &format!("version={version}&_csrf_token={}", browser.csrf),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let detail = ProposalService::detail(&pool, id).await.unwrap().unwrap();
    assert_eq!(detail.proposal.status, ProposalStatus::Stale);
    assert!(detail.proposal.decided_at.is_some());
    assert_eq!(detail.proposal.decided_by, None);
    assert!(!f.staging.join(&staged).exists(), "staged cover discarded");

    let other_detail = ProposalService::detail(&pool, other_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(other_detail.proposal.status, ProposalStatus::Pending);
    assert!(f.staging.join(&other_staged).exists());

    // The revoked key can no longer reach the tool.
    let (status, _) = send(
        &f.agent.app,
        Call::new(
            &f.agent.key,
            legacy_call(TOOL, args(title, &png(2, 2), "image/png")),
        )
        .version(V_LEGACY),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_stale_revocation_version_changes_nothing(pool: DbPool) {
    let f = fixture(&pool).await;
    let title = seed_title(&pool, "Conflit").await;
    let res = f
        .agent
        .ok(TOOL, args(title, &png(10, 10), "image/png"))
        .await;
    let id = res["proposal_id"].as_u64().unwrap();
    let key_id: u64 = sqlx::query_scalar("SELECT api_key_id FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let revoked =
        mybibli::services::cover_proposals::revoke_key(&pool, Some(&f.staging), key_id, 999)
            .await
            .unwrap();
    assert!(!revoked);
    let detail = ProposalService::detail(&pool, id).await.unwrap().unwrap();
    assert_eq!(detail.proposal.status, ProposalStatus::Pending);
    assert_eq!(staged_files(&f.staging).len(), 1);
}

fn age(path: &Path, by: Duration) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - by).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
async fn the_scheduled_purge_keeps_only_what_pending_proposals_need(pool: DbPool) {
    let f = fixture(&pool).await;
    let title = seed_title(&pool, "Purge").await;
    seed_admin(&pool, "decider").await;
    let decider: u64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'decider'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let old = PURGE_GRACE + Duration::from_secs(60);

    let file = |n: u32| {
        let a = args(title, &png(n, n), "image/png");
        let agent = &f.agent;
        async move { agent.ok(TOOL, a).await["proposal_id"].as_u64().unwrap() }
    };
    let pending = file(5).await;
    let rejected = file(6).await;
    let accepted_now = file(7).await;
    let accepted_long_ago = file(8).await;
    let soft_deleted = file(9).await;

    ProposalService::transition(
        &pool,
        rejected,
        &Decision::Reject {
            by: decider,
            reason: None,
        },
    )
    .await
    .unwrap();
    for id in [accepted_now, accepted_long_ago] {
        ProposalService::transition(&pool, id, &Decision::Accept { by: decider })
            .await
            .unwrap();
    }
    sqlx::query("UPDATE proposals SET decided_at = NOW() - INTERVAL 2 HOUR WHERE id = ?")
        .bind(accepted_long_ago)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE proposals SET deleted_at = NOW() WHERE id = ?")
        .bind(soft_deleted)
        .execute(&pool)
        .await
        .unwrap();

    let name = |id| staged_file_of(&pool, id);
    let (n_pending, n_rejected, n_acc_now, n_acc_old) = (
        name(pending).await,
        name(rejected).await,
        name(accepted_now).await,
        name(accepted_long_ago).await,
    );
    let n_deleted: String = sqlx::query_scalar(
        "SELECT CAST(JSON_UNQUOTE(JSON_EXTRACT(proposed_values, '$.staged_file')) AS CHAR) \
         FROM proposal_items WHERE proposal_id = ?",
    )
    .bind(soft_deleted)
    .fetch_one(&pool)
    .await
    .unwrap();
    // Orphans: one old (a filing that never committed), one fresh (a
    // filing in flight), an old temporary file, and a foreign file.
    let orphan_old = cover_staging::new_staged_name();
    let orphan_young = cover_staging::new_staged_name();
    let tmp_old = format!(".tmp-{}", cover_staging::new_staged_name());
    for n in [
        &orphan_old,
        &orphan_young,
        &tmp_old,
        &"notes.txt".to_string(),
    ] {
        std::fs::write(f.staging.join(n), b"x").unwrap();
    }
    for n in [
        &n_pending,
        &n_acc_now,
        &orphan_old,
        &tmp_old,
        &"notes.txt".to_string(),
    ] {
        age(&f.staging.join(n), old);
    }

    // A temporary file of an interrupted cross-filesystem promotion, in
    // the covers directory itself; and a fresh one, kept.
    let covers_tmp_old = f.covers.join(".tmp-promote-old.jpg");
    let covers_tmp_young = f.covers.join(".tmp-promote-young.jpg");
    std::fs::write(&covers_tmp_old, b"x").unwrap();
    std::fs::write(&covers_tmp_young, b"x").unwrap();
    age(&covers_tmp_old, old);

    let stats = cover_staging::purge(&pool, &f.staging).await.unwrap();
    assert!(!covers_tmp_old.exists());
    assert!(covers_tmp_young.exists());
    let mut expected = vec![
        n_pending.clone(),
        n_acc_now.clone(),
        orphan_young.clone(),
        "notes.txt".to_string(),
    ];
    expected.sort();
    assert_eq!(staged_files(&f.staging), expected);
    assert_eq!(stats.deleted, 6, "{stats:?}");
    assert_eq!(stats.errors, 0);
    for gone in [&n_rejected, &n_acc_old, &n_deleted, &orphan_old, &tmp_old] {
        assert!(!f.staging.join(gone).exists(), "{gone}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn promote_and_discard_for_the_deciding_stories(pool: DbPool) {
    let f = fixture(&pool).await;
    let title = seed_title(&pool, "Promotion").await;
    let a = f
        .agent
        .ok(TOOL, args(title, &png(600, 900), "image/png"))
        .await;
    let b = f
        .agent
        .ok(TOOL, args(title, &png(20, 20), "image/png"))
        .await;
    let (sa, sb) = (
        staged_file_of(&pool, a["proposal_id"].as_u64().unwrap()).await,
        staged_file_of(&pool, b["proposal_id"].as_u64().unwrap()).await,
    );

    let url = cover_staging::promote(&f.staging, &f.covers, &sa, title)
        .await
        .unwrap();
    assert_eq!(url, format!("/covers/{title}.jpg"));
    assert!(f.covers.join(format!("{title}.jpg")).exists());
    assert!(matches!(
        cover_staging::promote(&f.staging, &f.covers, &sa, title).await,
        Err(StagingError::Missing)
    ));

    cover_staging::discard(&f.staging, &sb).await.unwrap();
    assert!(staged_files(&f.staging).is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn the_covers_route_never_serves_the_staging_area(pool: DbPool) {
    let f = fixture(&pool).await;
    let title = seed_title(&pool, "Privé").await;
    let res = f.agent.ok(TOOL, args(title, &png(8, 8), "image/png")).await;
    let staged = staged_file_of(&pool, res["proposal_id"].as_u64().unwrap()).await;
    std::fs::write(f.covers.join("42.jpg"), b"public").unwrap();
    // A hidden directory below an ordinary one: without the decoding
    // guard, `ServeDir` would serve `x%2F.staging%2F<name>`.
    std::fs::create_dir_all(f.covers.join("x/.staging")).unwrap();
    std::fs::write(f.covers.join("x/.staging").join(&staged), b"hidden").unwrap();

    let get = |path: String| {
        let app = f.agent.app.clone();
        async move {
            app.oneshot(
                axum::http::Request::get(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
        }
    };
    assert_eq!(get("/covers/42.jpg".into()).await, StatusCode::OK);
    for path in [
        format!("/covers/.staging/{staged}"),
        format!("/covers/%2estaging/{staged}"),
        format!("/covers/%2Estaging/{staged}"),
        "/covers/.staging/".to_string(),
        format!("/covers/x%2F.staging%2F{staged}"),
        format!("/covers/x%2f%2estaging%2f{staged}"),
        format!("/covers/x/.staging/{staged}"),
    ] {
        assert_eq!(get(path.clone()).await, StatusCode::NOT_FOUND, "{path}");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn a_key_with_too_many_pending_covers_is_refused(pool: DbPool) {
    let f = fixture(&pool).await;
    let title = seed_title(&pool, "Plafond").await;
    let first = f.agent.ok(TOOL, args(title, &png(4, 4), "image/png")).await;
    let key_id: u64 = sqlx::query_scalar("SELECT api_key_id FROM proposals WHERE id = ?")
        .bind(first["proposal_id"].as_u64().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    // 99 more pending cover proposals for the same key (rows only).
    for _ in 1..MAX_PENDING_COVERS_PER_KEY {
        sqlx::query("INSERT INTO proposals (kind, api_key_id, sources) VALUES ('cover', ?, '[]')")
            .bind(key_id)
            .execute(&pool)
            .await
            .unwrap();
    }
    let msg = f
        .agent
        .err(TOOL, args(title, &png(4, 4), "image/png"))
        .await;
    assert!(msg.contains("already has 100 cover proposals"), "{msg}");
    assert_eq!(staged_files(&f.staging).len(), 1, "nothing staged for it");

    // Deciding one makes room again.
    sqlx::query("UPDATE proposals SET status = 'rejected', decided_at = NOW() WHERE id = ?")
        .bind(first["proposal_id"].as_u64().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    f.agent.ok(TOOL, args(title, &png(4, 4), "image/png")).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_calls_cannot_pass_the_pending_cap_together(pool: DbPool) {
    let f = fixture(&pool).await;
    let title = seed_title(&pool, "Concurrence").await;
    let first = f.agent.ok(TOOL, args(title, &png(4, 4), "image/png")).await;
    let key_id: u64 = sqlx::query_scalar("SELECT api_key_id FROM proposals WHERE id = ?")
        .bind(first["proposal_id"].as_u64().unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    // One slot left.
    for _ in 2..MAX_PENDING_COVERS_PER_KEY {
        sqlx::query("INSERT INTO proposals (kind, api_key_id, sources) VALUES ('cover', ?, '[]')")
            .bind(key_id)
            .execute(&pool)
            .await
            .unwrap();
    }
    let calls = (0..4).map(|i| {
        let agent = &f.agent;
        let a = args(title, &png(4 + i, 4), "image/png");
        async move { agent.call(TOOL, a).await }
    });
    let results = futures::future::join_all(calls).await;
    let filed = results.iter().filter(|r| r["isError"] == false).count();
    assert_eq!(filed, 1, "{results:?}");
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM proposals WHERE api_key_id = ? AND status = 'pending'",
    )
    .bind(key_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pending as u64, MAX_PENDING_COVERS_PER_KEY);
    // The refused calls left no staged file behind.
    assert_eq!(staged_files(&f.staging).len(), 2);
}
