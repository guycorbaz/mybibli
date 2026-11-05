//! Story 15-5 (#564, FR165) — `volume_value` proposals: the `get_title`
//! read path, filing through the MCP tool `propose_volume_value`, the
//! review screen, the removable list-price line, acceptance (amounts and
//! date written in one transaction, stale per field) and the lock scope of
//! an acceptance (epic 12 retro A1).
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test proposal_volume_value

mod common;

use axum::http::StatusCode;
use common::mcp::*;
use common::web::{Browser, PW_HASH, seed_admin};
use mybibli::db::DbPool;
use serde_json::{Value, json};

const SRC: &str = "https://example.org/cote";

struct Fx {
    agent: Agent,
    lib: Browser,
    lib_id: u64,
}

async fn fx(pool: &DbPool) -> Fx {
    let agent = Agent::new(pool).await;
    seed_admin(pool, "vv-admin").await;
    let lib_id =
        sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
            .bind("vv-lib")
            .bind(PW_HASH)
            .execute(pool)
            .await
            .unwrap()
            .last_insert_id();
    let lib = Browser::login(&agent.app, "vv-lib").await;
    Fx { agent, lib, lib_id }
}

/// A title with one volume; returns `(title id, volume id)`.
async fn volume(pool: &DbPool, title: &str, code: &str) -> (u64, u64) {
    let t = seed_title(pool, title).await;
    let v = sqlx::query("INSERT INTO volumes (label, title_id) VALUES (?, ?)")
        .bind(code)
        .bind(t)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id();
    (t, v)
}

fn value(amount: Value) -> Value {
    json!({"amount": amount, "currency": "CHF"})
}

async fn file(f: &Fx, args: Value) -> u64 {
    f.agent.ok("propose_volume_value", args).await["proposal_id"]
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

/// `(current value, its currency, purchase price, its currency, dated)`.
async fn amounts(
    pool: &DbPool,
    id: u64,
) -> (Option<String>, Option<String>, Option<String>, Option<String>, bool) {
    sqlx::query_as(
        "SELECT CAST(current_value AS CHAR), current_value_currency, \
                CAST(purchase_price AS CHAR), purchase_currency, \
                current_value_updated_at IS NOT NULL \
         FROM volumes WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
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

async fn post(f: &Fx, path: String, body: String) -> (StatusCode, String) {
    let (status, body) = f.lib.post_form(&f.agent.app, &path, &body).await;
    (status, text(body))
}

async fn accept(f: &Fx, pool: &DbPool, id: u64) -> (StatusCode, String) {
    let v = version_of(pool, id).await;
    post(f, format!("/proposals/{id}/accept"), format!("version={v}")).await
}

async fn reject(f: &Fx, pool: &DbPool, id: u64) {
    let v = version_of(pool, id).await;
    let (status, _) = post(f, format!("/proposals/{id}/reject"), format!("version={v}")).await;
    assert_eq!(status, StatusCode::OK);
}

// ─── Read path and filing ───────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_agent_reads_the_amounts_and_files_a_value_with_a_list_price(pool: DbPool) {
    let f = fx(&pool).await;
    let (title, vol) = volume(&pool, "Le Lotus bleu", "V0501").await;
    sqlx::query("UPDATE volumes SET edition_comment = 'EO 1946, dos toilé' WHERE id = ?")
        .bind(vol)
        .execute(&pool)
        .await
        .unwrap();
    let doc = f.agent.ok("get_title", json!({"id": title})).await;
    assert_eq!(doc["default_currency"], "CHF");
    let v = &doc["volumes"][0];
    assert_eq!(v["id"], vol);
    assert_eq!(v["edition_comment"], "EO 1946, dos toilé");
    assert_eq!(v["purchase_price"], Value::Null);
    assert_eq!(v["current_value"], Value::Null);

    let id = file(
        &f,
        json!({"volume_id": vol, "current_value": value(json!(180.5)),
               "list_price": value(json!(12)), "note": "Bon état", "sources": [SRC]}),
    )
    .await;
    let lines = items(&pool, id).await;
    assert_eq!(lines.len(), 2);
    let cv = &lines[0].1;
    assert_eq!(cv["line"], "current_value");
    assert_eq!((cv["amount_cents"].clone(), cv["currency"].clone()), (json!(18050), json!("CHF")));
    assert_eq!(cv["current"]["amount_cents"], Value::Null);
    assert_eq!(cv["note"], "Bon état");
    assert_eq!(
        lines[1].1,
        json!({"line": "list_price", "amount_cents": 1200, "currency": "CHF"})
    );
    let (kind, key): (String, String) =
        sqlx::query_as("SELECT kind, CAST(equivalence_key AS CHAR) FROM proposals WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(kind, "volume_value");
    assert_eq!(key, format!("volume_value:volumes/{vol}:18050CHF:list:1200CHF"));
    // The catalog is untouched.
    assert_eq!(amounts(&pool, vol).await, (None, None, None, None, false));
    // The agent's history names the volume.
    let page = f.agent.ok("list_my_proposals", json!({"kind": "volume_value"})).await;
    assert_eq!(page["items"][0]["subject"], json!({"volume_id": vol}));
}

#[sqlx::test(migrations = "./migrations")]
async fn refusals_write_nothing_and_say_why(pool: DbPool) {
    let f = fx(&pool).await;
    let (_, vol) = volume(&pool, "Tintin au Tibet", "V0502").await;
    let (_, bought) = volume(&pool, "Coke en stock", "V0503").await;
    sqlx::query(
        "UPDATE volumes SET purchase_price = 9.90, purchase_currency = 'CHF', \
         current_value = 15.00, current_value_currency = 'CHF', \
         current_value_updated_at = NOW() WHERE id = ?",
    )
    .bind(bought)
    .execute(&pool)
    .await
    .unwrap();
    let (_, gone) = volume(&pool, "Gone", "V0504").await;
    soft_delete(&pool, "volumes", gone).await;
    let before = proposal_count(&pool).await;
    let cases: Vec<(Value, &str)> = vec![
        (json!({"volume_id": 999_999, "current_value": value(json!(5))}), "999999"),
        (json!({"volume_id": gone, "current_value": value(json!(5))}), "No volume"),
        (json!({"volume_id": vol, "current_value": value(json!(5.555))}), "at most two decimals"),
        (json!({"volume_id": vol, "current_value": value(json!(0))}), "positive amount"),
        (
            json!({"volume_id": vol, "current_value": {"amount": 5, "currency": "EUR"}}),
            "records values in CHF",
        ),
        (
            json!({"volume_id": bought, "current_value": value(json!(20)),
                   "list_price": value(json!(9))}),
            "already has a purchase price",
        ),
        (json!({"volume_id": bought, "current_value": value(json!(15))}), "already has this current value"),
    ];
    for (mut args, needle) in cases {
        args["sources"] = json!([SRC]);
        let msg = f.agent.err("propose_volume_value", args.clone()).await;
        assert!(msg.contains(needle), "{args}: {msg}");
    }
    assert_eq!(proposal_count(&pool).await, before);

    // One pending value proposal per volume; a rejection blocks the same
    // amounts only.
    let args = json!({"volume_id": vol, "current_value": value(json!(30)), "sources": [SRC]});
    let id = file(&f, args.clone()).await;
    let msg = f.agent.err("propose_volume_value", args.clone()).await;
    assert!(msg.contains(&format!("Proposal {id}")), "{msg}");
    reject(&f, &pool, id).await;
    let msg = f.agent.err("propose_volume_value", args).await;
    assert!(msg.contains("was rejected on"), "{msg}");
    file(
        &f,
        json!({"volume_id": vol, "current_value": value(json!(32)), "sources": [SRC]}),
    )
    .await;
}

// ─── Review and acceptance ──────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_review_shows_the_volume_and_acceptance_writes_the_amounts(pool: DbPool) {
    let f = fx(&pool).await;
    let (_, vol) = volume(&pool, "L'Affaire Tournesol", "V0505").await;
    sqlx::query("UPDATE volumes SET edition_comment = 'Réédition 1980' WHERE id = ?")
        .bind(vol)
        .execute(&pool)
        .await
        .unwrap();
    let id = file(
        &f,
        json!({"volume_id": vol, "current_value": value(json!(45.5)),
               "list_price": value(json!(9.9)), "note": "Coins frottés", "sources": [SRC]}),
    )
    .await;
    let (status, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    let page = text(page);
    assert!(page.contains("data-volume-value"), "{page}");
    assert!(page.contains("45.50 CHF") || page.contains("45,50 CHF"), "{page}");
    assert!(page.contains("data-list-price"), "{page}");
    assert!(page.contains("Réédition 1980"), "{page}");
    assert!(page.contains("Coins frottés"), "{page}");
    assert!(page.contains("V0505"), "{page}");

    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("V0505"), "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted");
    assert_eq!(
        amounts(&pool, vol).await,
        (
            Some("45.50".into()),
            Some("CHF".into()),
            Some("9.90".into()),
            Some("CHF".into()),
            true
        )
    );
    let audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_audit WHERE action = 'volume_value_update' AND entity_id = ?",
    )
    .bind(vol)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn acceptance_goes_stale_when_an_amount_changed(pool: DbPool) {
    let f = fx(&pool).await;
    let (_, a) = volume(&pool, "Valeur changée", "V0506").await;
    let (_, b) = volume(&pool, "Prix saisi", "V0507").await;
    let (_, c) = volume(&pool, "Supprimé", "V0508").await;
    let on_a = file(&f, json!({"volume_id": a, "current_value": value(json!(10)), "sources": [SRC]})).await;
    let on_b = file(
        &f,
        json!({"volume_id": b, "current_value": value(json!(10)),
               "list_price": value(json!(5)), "sources": [SRC]}),
    )
    .await;
    let on_c = file(&f, json!({"volume_id": c, "current_value": value(json!(10)), "sources": [SRC]})).await;
    sqlx::query(
        "UPDATE volumes SET current_value = 7.00, current_value_currency = 'CHF' WHERE id = ?",
    )
    .bind(a)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE volumes SET purchase_price = 4.00, purchase_currency = 'CHF' WHERE id = ?")
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();
    soft_delete(&pool, "volumes", c).await;
    // The review warns that the value changed since filing.
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{on_a}")).await;
    assert!(text(page).contains("data-value-changed"));
    for (id, needle_en, needle_fr) in [
        (on_a, "current value changed", "valeur actuelle du volume a changé"),
        (on_b, "purchase price was set", "prix d'achat a été saisi"),
        (on_c, "volume or its title was deleted", "volume ou son titre a été supprimé"),
    ] {
        let (status, body) = accept(&f, &pool, id).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body.contains(needle_en) || body.contains(needle_fr), "{body}");
        assert_eq!(status_of(&pool, id).await, "stale");
    }
    assert_eq!(amounts(&pool, a).await.0.as_deref(), Some("7.00"));
    assert_eq!(amounts(&pool, b).await.0, None);

    // Review round 1: the library's currency changed since filing.
    let (_, d) = volume(&pool, "Devise", "V0516").await;
    let on_d = file(&f, json!({"volume_id": d, "current_value": value(json!(10)), "sources": [SRC]})).await;
    sqlx::query("UPDATE settings SET setting_value = 'EUR' WHERE setting_key = 'default_currency'")
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = accept(&f, &pool, on_d).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("EUR"), "{body}");
    assert_eq!(status_of(&pool, on_d).await, "stale");
    assert_eq!(amounts(&pool, d).await.0, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_list_price_line_is_removable_and_the_value_line_is_not(pool: DbPool) {
    let f = fx(&pool).await;
    let (_, vol) = volume(&pool, "Retrait", "V0509").await;
    let id = file(
        &f,
        json!({"volume_id": vol, "current_value": value(json!(20)),
               "list_price": value(json!(8)), "sources": [SRC]}),
    )
    .await;
    let lines = items(&pool, id).await;
    let (value_item, list_item) = (lines[0].0, lines[1].0);
    let v = version_of(&pool, id).await;
    let (status, _) = post(
        &f,
        format!("/proposals/{id}/items/{value_item}/remove"),
        format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    // Not editable either (D6).
    let (status, _) = post(
        &f,
        format!("/proposals/{id}/items/{value_item}"),
        format!("version={v}&amount=21"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, body) = post(
        &f,
        format!("/proposals/{id}/items/{list_item}/remove"),
        format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let left = items(&pool, id).await;
    assert_eq!(left.len(), 1);
    // Review round 1: the fingerprint no longer names the list price.
    assert_eq!(left[0].1["fingerprint"], "2000CHF");
    let (status, _) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK);
    let (cv, _, pp, _, dated) = amounts(&pool, vol).await;
    assert_eq!((cv.as_deref(), pp, dated), (Some("20.00"), None, true));
}

// Review round 1: a rejection after the list price was removed blocks the
// value alone; removing the list price of an unchanged value is refused.
#[sqlx::test(migrations = "./migrations")]
async fn a_rejection_blocks_what_the_librarian_rejected(pool: DbPool) {
    let f = fx(&pool).await;
    let (_, vol) = volume(&pool, "Rejet", "V0515").await;
    let id = file(
        &f,
        json!({"volume_id": vol, "current_value": value(json!(20)),
               "list_price": value(json!(8)), "sources": [SRC]}),
    )
    .await;
    let list_item = items(&pool, id).await[1].0;
    let v = version_of(&pool, id).await;
    let (status, _) = post(
        &f,
        format!("/proposals/{id}/items/{list_item}/remove"),
        format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    reject(&f, &pool, id).await;
    let key: String =
        sqlx::query_scalar("SELECT CAST(equivalence_key AS CHAR) FROM proposals WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(key, format!("volume_value:volumes/{vol}:2000CHF"));
    let msg = f
        .agent
        .err(
            "propose_volume_value",
            json!({"volume_id": vol, "current_value": value(json!(20)), "sources": [SRC]}),
        )
        .await;
    assert!(msg.contains("was rejected on"), "{msg}");

    // The same value as the current one, with a list price: the list price
    // cannot be removed (nothing would remain to apply).
    sqlx::query(
        "UPDATE volumes SET current_value = 30.00, current_value_currency = 'CHF' WHERE id = ?",
    )
    .bind(vol)
    .execute(&pool)
    .await
    .unwrap();
    let id = file(
        &f,
        json!({"volume_id": vol, "current_value": value(json!(30)),
               "list_price": value(json!(8)), "sources": [SRC]}),
    )
    .await;
    let list_item = items(&pool, id).await[1].0;
    // Review pass 2: the page offers no Remove button for it.
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert!(!page.contains(&format!("data-remove-line=\"{list_item}\"")));
    let v = version_of(&pool, id).await;
    let (status, _) = post(
        &f,
        format!("/proposals/{id}/items/{list_item}/remove"),
        format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(items(&pool, id).await.len(), 2);
}

// ─── Lock scope (epic 12 retro A1) ──────────────────────────────────

// An acceptance on one volume holds row locks on its proposal, its items
// and the volume row: a decision on a proposal about another volume —
// acceptance or rejection — must not wait for them.
#[sqlx::test(migrations = "./migrations")]
async fn a_value_acceptance_on_one_volume_does_not_lock_proposals_of_another(pool: DbPool) {
    use mybibli::services::acceptance::AcceptOutcome;
    use mybibli::services::proposals::{Decision, ProposalService};
    use mybibli::services::volume_value_acceptance;
    let f = fx(&pool).await;
    let (_, a) = volume(&pool, "Volume A", "V0510").await;
    let (_, b) = volume(&pool, "Volume B", "V0511").await;
    let (_, c) = volume(&pool, "Volume C", "V0512").await;
    let on_a = file(&f, json!({"volume_id": a, "current_value": value(json!(1)), "sources": [SRC]})).await;
    let on_b = file(&f, json!({"volume_id": b, "current_value": value(json!(2)), "sources": [SRC]})).await;
    let on_c = file(&f, json!({"volume_id": c, "current_value": value(json!(3)), "sources": [SRC]})).await;

    let mut tx1 = pool.begin().await.unwrap();
    let v = version_of(&pool, on_a).await;
    let outcome = volume_value_acceptance::accept_in(&mut tx1, on_a, f.lib_id, v)
        .await
        .unwrap();
    assert!(matches!(outcome, AcceptOutcome::Accepted(_)), "{outcome:?}");

    let mut tx2 = pool.begin().await.unwrap();
    sqlx::query("SET SESSION innodb_lock_wait_timeout = 2")
        .execute(&mut *tx2)
        .await
        .unwrap();
    let v = version_of(&pool, on_b).await;
    let outcome = volume_value_acceptance::accept_in(&mut tx2, on_b, f.lib_id, v)
        .await
        .expect("accepting a value of another volume must not wait for A's locks");
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
    .expect("rejecting a proposal on another volume must not wait for A's locks");
    tx2.commit().await.unwrap();
    tx1.commit().await.unwrap();
    assert_eq!(status_of(&pool, on_a).await, "accepted");
    assert_eq!(status_of(&pool, on_b).await, "accepted");
    assert_eq!(status_of(&pool, on_c).await, "rejected");
    assert_eq!(amounts(&pool, b).await.0.as_deref(), Some("2.00"));
}

// ─── Failures and access ────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_failure_while_applying_rolls_everything_back(pool: DbPool) {
    let f = fx(&pool).await;
    let (_, vol) = volume(&pool, "Annulé", "V0513").await;
    let id = file(&f, json!({"volume_id": vol, "current_value": value(json!(9)), "sources": [SRC]})).await;
    sqlx::query(
        "CREATE TRIGGER fail_volume_update BEFORE UPDATE ON volumes \
         FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'injected failure'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    sqlx::query("DROP TRIGGER fail_volume_update")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(!body.contains("injected"), "no database text");
    assert_eq!(status_of(&pool, id).await, "pending");
    assert_eq!(amounts(&pool, vol).await, (None, None, None, None, false));
}

#[sqlx::test(migrations = "./migrations")]
async fn csrf_and_role_are_required(pool: DbPool) {
    let f = fx(&pool).await;
    let (_, vol) = volume(&pool, "Gardé", "V0514").await;
    let id = file(
        &f,
        json!({"volume_id": vol, "current_value": value(json!(9)),
               "list_price": value(json!(4)), "sources": [SRC]}),
    )
    .await;
    let list_item = items(&pool, id).await[1].0;
    let v = version_of(&pool, id).await;
    let forged = Browser {
        cookie: f.lib.cookie.clone(),
        csrf: "forged".into(),
    };
    let anon = Browser::anonymous(&f.agent.app).await;
    for path in [
        format!("/proposals/{id}/accept"),
        format!("/proposals/{id}/items/{list_item}/remove"),
    ] {
        let body = format!("version={v}");
        let (status, _) = forged.post_form(&f.agent.app, &path, &body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        let res = anon.send(&f.agent.app, "POST", &path, &body, true).await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER, "{path}");
    }
    assert_eq!(status_of(&pool, id).await, "pending");
    assert_eq!(items(&pool, id).await.len(), 2);
    assert_eq!(amounts(&pool, vol).await.0, None);
}
