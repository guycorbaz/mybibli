//! Story 13-9 (#581, FR156) — an agent proposes a label on a doubtful
//! record: `list_labels`, the labels in `get_title`, `propose_label`
//! (filing and refusals), acceptance through the real router (attach,
//! stale on a deleted target or label or a label attached meanwhile),
//! rejection and resubmission delay, key revocation, the review pages, and
//! the lock scope of an acceptance.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test mcp_label_proposals

mod common;

use axum::http::StatusCode;
use common::mcp::*;
use common::web::{Browser, PW_HASH};
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use serde_json::{Value, json};

const TOOL: &str = "propose_label";
const SRC: &str = "https://example.org/notice";

struct Fx {
    agent: Agent,
    lib: Browser,
    lib_id: u64,
}

async fn fx(pool: &DbPool) -> Fx {
    let agent = Agent {
        app: router(pool),
        key: seed_key(pool, ApiKeyScope::Propose).await,
    };
    let lib_id =
        sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
            .bind("lb-lib")
            .bind(PW_HASH)
            .execute(pool)
            .await
            .unwrap()
            .last_insert_id();
    let lib = Browser::login(&agent.app, "lb-lib").await;
    Fx { agent, lib, lib_id }
}

async fn seed_volume(pool: &DbPool, title_id: u64, code: &str) -> u64 {
    sqlx::query("INSERT INTO volumes (label, title_id) VALUES (?, ?)")
        .bind(code)
        .bind(title_id)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn seed_label(pool: &DbPool, name: &str) -> u64 {
    sqlx::query("INSERT INTO labels (name) VALUES (?)")
        .bind(name)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

async fn attach_by_hand(pool: &DbPool, table: &str, fk: &str, id: u64, label: u64) {
    sqlx::query(&format!(
        "INSERT INTO {table} ({fk}, label_id) VALUES (?, ?)"
    ))
    .bind(id)
    .bind(label)
    .execute(pool)
    .await
    .unwrap();
}

/// Live `(volume, label)` links of a volume.
async fn volume_labels(pool: &DbPool, volume: u64) -> Vec<u64> {
    sqlx::query_scalar(
        "SELECT label_id FROM volume_labels \
         WHERE volume_id = ? AND deleted_at IS NULL ORDER BY label_id",
    )
    .bind(volume)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn title_labels(pool: &DbPool, title: u64) -> Vec<u64> {
    sqlx::query_scalar(
        "SELECT label_id FROM title_labels \
         WHERE title_id = ? AND deleted_at IS NULL ORDER BY label_id",
    )
    .bind(title)
    .fetch_all(pool)
    .await
    .unwrap()
}

fn on_volume(volume: u64, label: Value) -> Value {
    let mut args = json!({"target": {"volume_id": volume}, "sources": [SRC]});
    args.as_object_mut()
        .unwrap()
        .extend(label.as_object().unwrap().clone());
    args
}

async fn file(f: &Fx, args: Value) -> u64 {
    f.agent.ok(TOOL, args).await["proposal_id"]
        .as_u64()
        .expect("proposal_id")
}

async fn status_of(pool: &DbPool, id: u64) -> String {
    sqlx::query_scalar("SELECT status FROM proposals WHERE id = ?")
        .bind(id)
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

/// Apostrophes come back escaped. Pages render in the default locale
/// (French).
fn text(body: String) -> String {
    body.replace("&#x27;", "'").replace("&#39;", "'")
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

async fn reject(f: &Fx, pool: &DbPool, id: u64) {
    let v = version_of(pool, id).await;
    let (status, body) = f
        .lib
        .post_form(
            &f.agent.app,
            &format!("/proposals/{id}/reject"),
            &format!("version={v}&reason=Not+doubtful"),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

// ─── Read side (AC1) ────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn list_labels_and_get_title_show_the_labels(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Tintin au Tibet").await;
    let vol = seed_volume(&pool, title, "V0951").await;
    let other = seed_volume(&pool, title, "V0952").await;
    let doubt = seed_label(&pool, "Code douteux").await;
    let check = seed_label(&pool, "À vérifier").await;
    attach_by_hand(&pool, "title_labels", "title_id", title, check).await;
    attach_by_hand(&pool, "volume_labels", "volume_id", vol, doubt).await;

    let list = f.agent.ok("list_labels", json!({})).await;
    assert_eq!(
        list["items"],
        json!([
            {"id": check, "name": "À vérifier", "title_count": 1, "volume_count": 0},
            {"id": doubt, "name": "Code douteux", "title_count": 0, "volume_count": 1}
        ])
    );
    assert!(
        f.agent
            .err("list_labels", json!({"page": 1}))
            .await
            .contains("expected")
    );

    let doc = f.agent.ok("get_title", json!({"id": title})).await;
    assert_eq!(doc["labels"], json!([{"id": check, "name": "À vérifier"}]));
    let volumes = doc["volumes"].as_array().unwrap();
    let by_id = |id: u64| volumes.iter().find(|v| v["id"] == id).unwrap().clone();
    assert_eq!(
        by_id(vol)["labels"],
        json!([{"id": doubt, "name": "Code douteux"}])
    );
    assert_eq!(by_id(other)["labels"], json!([]));
}

// ─── Filing (AC2) ───────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_label_proposal_is_filed_and_changes_nothing(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Le Lotus bleu").await;
    let vol = seed_volume(&pool, title, "V0953").await;
    let doubt = seed_label(&pool, "Code douteux").await;

    let res = f
        .agent
        .ok(
            TOOL,
            on_volume(
                vol,
                json!({"label_name": "Code douteux", "note": "  The ISBN belongs to another book. "}),
            ),
        )
        .await;
    assert_eq!(res["status"], "pending");
    assert_eq!(res["kind"], "label_attach");
    assert_eq!(res["item_count"], 1);
    let id = res["proposal_id"].as_u64().unwrap();

    let (target_table, target_id, values, key): (String, u64, String, String) = sqlx::query_as(
        "SELECT i.target_table, i.target_id, CAST(i.proposed_values AS CHAR), \
         CAST(p.equivalence_key AS CHAR) \
         FROM proposal_items i JOIN proposals p ON p.id = i.proposal_id WHERE p.id = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((target_table.as_str(), target_id), ("volumes", vol));
    let values: Value = serde_json::from_str(&values).unwrap();
    assert_eq!(
        values,
        json!({"line": "label", "label_id": doubt, "label_name": "Code douteux",
               "note": "The ISBN belongs to another book."})
    );
    assert_eq!(key, format!("label:volumes/{vol}:{doubt}"));
    assert!(
        volume_labels(&pool, vol).await.is_empty(),
        "catalog unchanged"
    );

    // The agent's history names the subject.
    let mine = f
        .agent
        .ok("list_my_proposals", json!({"kind": "label_attach"}))
        .await;
    assert_eq!(
        mine["items"][0]["subject"],
        json!({"volume_id": vol, "label_id": doubt})
    );

    // A title target, by label id.
    let on_title = file(
        &f,
        json!({"target": {"title_id": title}, "label_id": doubt, "sources": [SRC]}),
    )
    .await;
    assert_eq!(status_of(&pool, on_title).await, "pending");
}

#[sqlx::test(migrations = "./migrations")]
async fn refusals_explain_and_file_nothing(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Objectif Lune").await;
    let vol = seed_volume(&pool, title, "V0954").await;
    let gone = seed_volume(&pool, title, "V0955").await;
    sqlx::query("UPDATE volumes SET deleted_at = NOW() WHERE id = ?")
        .bind(gone)
        .execute(&pool)
        .await
        .unwrap();
    let doubt = seed_label(&pool, "Code douteux").await;
    let attached = seed_label(&pool, "Déjà là").await;
    attach_by_hand(&pool, "volume_labels", "volume_id", vol, attached).await;

    let m = f
        .agent
        .err(TOOL, on_volume(999_999, json!({"label_id": doubt})))
        .await;
    assert!(m.contains("No volume with id 999999"), "{m}");
    let m = f
        .agent
        .err(TOOL, on_volume(gone, json!({"label_id": doubt})))
        .await;
    assert!(m.contains(&format!("No volume with id {gone}")), "{m}");
    let m = f
        .agent
        .err(
            TOOL,
            json!({"target": {"title_id": 999_999}, "label_id": doubt, "sources": [SRC]}),
        )
        .await;
    assert!(m.contains("No title with id 999999"), "{m}");
    let m = f
        .agent
        .err(TOOL, on_volume(vol, json!({"label_id": 999_999})))
        .await;
    assert!(m.contains("No label with id 999999"), "{m}");
    // The name must match exactly; the message lists the labels.
    let m = f
        .agent
        .err(TOOL, on_volume(vol, json!({"label_name": "code douteux"})))
        .await;
    assert!(
        m.contains("\"Code douteux\"") && m.contains("list_labels"),
        "{m}"
    );
    let m = f
        .agent
        .err(TOOL, on_volume(vol, json!({"label_id": attached})))
        .await;
    assert!(m.contains("already carries the label \"Déjà là\""), "{m}");
    let m = f
        .agent
        .err(
            TOOL,
            on_volume(vol, json!({"label_id": doubt, "note": "x".repeat(501)})),
        )
        .await;
    assert!(m.contains("at most 500"), "{m}");
    let m = f
        .agent
        .err(
            TOOL,
            json!({"target": {"volume_id": vol}, "label_id": doubt, "sources": ["ftp://x.y/z"]}),
        )
        .await;
    assert!(m.contains("Source 1 was refused"), "{m}");

    // Pending duplicate.
    let first = file(&f, on_volume(vol, json!({"label_id": doubt}))).await;
    let m = f
        .agent
        .err(TOOL, on_volume(vol, json!({"label_id": doubt})))
        .await;
    assert!(
        m.contains(&format!("Proposal {first} already proposes")),
        "{m}"
    );

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM proposals")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "only the first filing went through");
}

// ─── Acceptance (AC4) and review (AC3) ──────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn accepting_attaches_the_label(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Les Cigares du pharaon").await;
    let vol = seed_volume(&pool, title, "V0956").await;
    let doubt = seed_label(&pool, "Code douteux").await;
    let id = file(
        &f,
        on_volume(
            vol,
            json!({"label_id": doubt, "note": "Barcode of another edition"}),
        ),
    )
    .await;

    // The review pages name the kind, the record, the label and the note.
    let (_, list) = f.lib.get(&f.agent.app, "/proposals").await;
    let list = text(list);
    assert!(list.contains("Étiquette : « Code douteux »"), "{list}");
    assert!(list.contains("V0956 — Les Cigares du pharaon"), "{list}");
    assert!(list.contains(&format!("href=\"/volume/{vol}\"")), "{list}");
    let (status, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    let page = text(page);
    assert!(page.contains("data-label-name"), "{page}");
    assert!(
        page.contains(&format!("href=\"/labels/{doubt}\"")),
        "{page}"
    );
    assert!(page.contains("Barcode of another edition"), "{page}");
    assert!(page.contains("V0956 — Les Cigares du pharaon"), "{page}");
    assert!(!page.contains("proposals."), "raw key leaked");
    // No edit or remove affordance for a label line.
    assert!(
        !page.contains("data-remove-line") && !page.contains("proposal-edit-"),
        "{page}"
    );

    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("porte désormais l'étiquette « Code douteux »"),
        "{body}"
    );
    assert!(body.contains(&format!("/volume/{vol}")), "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted");
    assert_eq!(volume_labels(&pool, vol).await, vec![doubt]);

    // The volume and the label pages show it.
    let (_, vpage) = f.lib.get(&f.agent.app, &format!("/volume/{vol}")).await;
    assert!(vpage.contains("Code douteux"), "{vpage}");
    let (_, lpage) = f.lib.get(&f.agent.app, &format!("/labels/{doubt}")).await;
    assert!(lpage.contains("V0956"), "{lpage}");
}

#[sqlx::test(migrations = "./migrations")]
async fn accepting_on_a_title_brings_a_detached_link_back(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "L'Oreille cassée").await;
    let doubt = seed_label(&pool, "Code douteux").await;
    attach_by_hand(&pool, "title_labels", "title_id", title, doubt).await;
    sqlx::query("UPDATE title_labels SET deleted_at = NOW() WHERE title_id = ?")
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    let id = file(
        &f,
        json!({"target": {"title_id": title}, "label_id": doubt, "sources": [SRC]}),
    )
    .await;
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted");
    assert_eq!(title_labels(&pool, title).await, vec![doubt]);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM title_labels WHERE title_id = ?")
        .bind(title)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        rows, 1,
        "the detached link was brought back, not duplicated"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_volume_makes_the_proposal_stale(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Le Sceptre d'Ottokar").await;
    let vol = seed_volume(&pool, title, "V0957").await;
    let doubt = seed_label(&pool, "Code douteux").await;
    let id = file(&f, on_volume(vol, json!({"label_id": doubt}))).await;
    sqlx::query("UPDATE volumes SET deleted_at = NOW() WHERE id = ?")
        .bind(vol)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("le titre ou le volume qu'elle marque a été supprimé"),
        "{body}"
    );
    assert_eq!(status_of(&pool, id).await, "stale");
    assert!(volume_labels(&pool, vol).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_label_makes_the_proposal_stale(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Le Crabe aux pinces d'or").await;
    let vol = seed_volume(&pool, title, "V0958").await;
    let doubt = seed_label(&pool, "Code douteux").await;
    let id = file(&f, on_volume(vol, json!({"label_id": doubt}))).await;
    sqlx::query("UPDATE labels SET deleted_at = NOW() WHERE id = ?")
        .bind(doubt)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("l'étiquette « Code douteux » a été supprimée"),
        "{body}"
    );
    assert_eq!(status_of(&pool, id).await, "stale");
    assert!(volume_labels(&pool, vol).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_label_attached_meanwhile_makes_the_proposal_stale(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "L'Étoile mystérieuse").await;
    let vol = seed_volume(&pool, title, "V0959").await;
    let doubt = seed_label(&pool, "Code douteux").await;
    let id = file(&f, on_volume(vol, json!({"label_id": doubt}))).await;
    attach_by_hand(&pool, "volume_labels", "volume_id", vol, doubt).await;
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("déjà sur la notice"), "{body}");
    assert_eq!(status_of(&pool, id).await, "stale");
    assert_eq!(volume_labels(&pool, vol).await, vec![doubt]);
}

// ─── Rejection, resubmission delay and revocation (AC5) ─────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_rejection_blocks_the_same_label_on_the_same_record(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Le Secret de la Licorne").await;
    let vol = seed_volume(&pool, title, "V0960").await;
    let doubt = seed_label(&pool, "Code douteux").await;
    let check = seed_label(&pool, "À vérifier").await;
    let id = file(&f, on_volume(vol, json!({"label_id": doubt}))).await;
    reject(&f, &pool, id).await;
    assert_eq!(status_of(&pool, id).await, "rejected");
    assert!(volume_labels(&pool, vol).await.is_empty());

    let m = f
        .agent
        .err(TOOL, on_volume(vol, json!({"label_id": doubt})))
        .await;
    assert!(
        m.contains(&format!("label {doubt} on volume {vol}")) && m.contains("Not doubtful"),
        "{m}"
    );
    let mine = f
        .agent
        .ok("list_my_proposals", json!({"status": "rejected"}))
        .await;
    assert!(mine["items"][0]["resubmittable_from"].is_string(), "{mine}");
    // Another label, or the same label on the title: not equivalent.
    file(&f, on_volume(vol, json!({"label_id": check}))).await;
    file(
        &f,
        json!({"target": {"title_id": title}, "label_id": doubt, "sources": [SRC]}),
    )
    .await;
}

#[sqlx::test(migrations = "./migrations")]
async fn revoking_the_key_stales_its_label_proposals(pool: DbPool) {
    use mybibli::services::proposals::ProposalService;
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Tintin au Congo").await;
    let vol = seed_volume(&pool, title, "V0961").await;
    let doubt = seed_label(&pool, "Code douteux").await;
    let id = file(&f, on_volume(vol, json!({"label_id": doubt}))).await;
    let (key_id, version): (u64, i32) = sqlx::query_as(
        "SELECT k.id, k.version FROM api_keys k JOIN proposals p ON p.api_key_id = k.id \
         WHERE p.id = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let revoked = ProposalService::revoke_key(&pool, key_id, version)
        .await
        .unwrap()
        .expect("revoked");
    assert!(
        revoked.staged_files.is_empty(),
        "no staged file for a label"
    );
    assert_eq!(status_of(&pool, id).await, "stale");
}

// ─── Lock scope (epic 12 retro A1) ──────────────────────────────────

// An acceptance on one record holds row locks on its proposal, its
// record, its label and its link only: a decision on a proposal about
// another title — rejection or acceptance — must not wait for them.
#[sqlx::test(migrations = "./migrations")]
async fn an_acceptance_on_one_record_does_not_lock_proposals_of_another(pool: DbPool) {
    use mybibli::services::acceptance::AcceptOutcome;
    use mybibli::services::label_acceptance;
    use mybibli::services::proposals::{Decision, ProposalService};
    let f = fx(&pool).await;
    let a = seed_title(&pool, "Titre A").await;
    let b = seed_title(&pool, "Titre B").await;
    let va = seed_volume(&pool, a, "V0962").await;
    let vb = seed_volume(&pool, b, "V0963").await;
    let doubt = seed_label(&pool, "Code douteux").await;
    let on_a = file(&f, on_volume(va, json!({"label_id": doubt}))).await;
    let on_b = file(&f, on_volume(vb, json!({"label_id": doubt}))).await;
    let on_b_title = file(
        &f,
        json!({"target": {"title_id": b}, "label_id": doubt, "sources": [SRC]}),
    )
    .await;

    // Transaction 1: the acceptance on A, kept open.
    let mut tx1 = pool.begin().await.unwrap();
    let v = version_of(&pool, on_a).await;
    let outcome = label_acceptance::accept_in(&mut tx1, on_a, f.lib_id, v)
        .await
        .unwrap();
    assert!(matches!(outcome, AcceptOutcome::Accepted(_)), "{outcome:?}");

    // Transaction 2: decisions on B — must not wait for A's locks.
    let mut tx2 = pool.begin().await.unwrap();
    sqlx::query("SET SESSION innodb_lock_wait_timeout = 2")
        .execute(&mut *tx2)
        .await
        .unwrap();
    let v = version_of(&pool, on_b).await;
    let outcome = label_acceptance::accept_in(&mut tx2, on_b, f.lib_id, v)
        .await
        .expect("accepting a label on another record must not wait for A's locks");
    assert!(matches!(outcome, AcceptOutcome::Accepted(_)), "{outcome:?}");
    let _ = ProposalService::transition_in(
        &mut tx2,
        on_b_title,
        &Decision::Reject {
            by: f.lib_id,
            reason: None,
        },
    )
    .await
    .expect("rejecting a proposal on another title must not wait for A's locks");
    tx2.commit().await.unwrap();
    tx1.commit().await.unwrap();
    assert_eq!(status_of(&pool, on_a).await, "accepted");
    assert_eq!(status_of(&pool, on_b).await, "accepted");
    assert_eq!(status_of(&pool, on_b_title).await, "rejected");
    assert_eq!(volume_labels(&pool, va).await, vec![doubt]);
    assert_eq!(volume_labels(&pool, vb).await, vec![doubt]);
}
