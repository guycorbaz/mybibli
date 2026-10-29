//! Story 11-17 (#493) — accepting a series proposal (FR138, FR140,
//! FR146): `series_create` and `series_attach` applied in one
//! transaction, staleness by version and by catalog state (#521, #529),
//! rollback, refusals, through the real router.
//!
//! Proposals are filed by an agent through the real MCP tools, so their
//! items carry the shapes of story 11-12.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test proposal_accept_series

mod common;

use axum::http::StatusCode;
use common::mcp::*;
use common::web::{Browser, PW_HASH, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::expected_tome::ExpectedTomeOrigin;
use mybibli::services::expected_tomes::{ExpectedTomeInput, ExpectedTomeService};
use mybibli::services::series_acceptance::{self, AcceptConflict, AcceptOutcome, StaleCause};
use serde_json::{Value, json};

const SRC: &str = "https://example.org/series";
/// A valid ISBN-13 (978-2-07-036024-6).
const ISBN_A: &str = "9782070360246";
/// Another valid ISBN-13 (978-0-13-468599-1).
const ISBN_B: &str = "9780134685991";

struct Fx {
    agent: Agent,
    lib: Browser,
    lib_id: u64,
}

async fn fx(pool: &DbPool) -> Fx {
    let agent = Agent::new(pool).await;
    seed_admin(pool, "pa-admin").await;
    let lib_id =
        sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
            .bind("pa-lib")
            .bind(PW_HASH)
            .execute(pool)
            .await
            .unwrap()
            .last_insert_id();
    let lib = Browser::login(&agent.app, "pa-lib").await;
    Fx { agent, lib, lib_id }
}

async fn file(f: &Fx, tool: &str, args: Value) -> u64 {
    f.agent.ok(tool, args).await["proposal_id"]
        .as_u64()
        .expect("proposal_id")
}

async fn version_of(pool: &DbPool, id: u64) -> i32 {
    sqlx::query_scalar("SELECT version FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// `(status, decided_by, decided_at set, applied_series_id)`.
async fn decision(pool: &DbPool, id: u64) -> (String, Option<i64>, bool, Option<i64>) {
    sqlx::query_as(
        "SELECT status, CAST(decided_by AS SIGNED), decided_at IS NOT NULL, \
         CAST(applied_series_id AS SIGNED) FROM proposals WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Apostrophes come back escaped (`&#x27;`): put them back so the
/// French copy can be asserted as written.
fn text(body: String) -> String {
    body.replace("&#x27;", "'")
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

/// Live `(title_id, position, is_omnibus)` of a series, by position.
async fn positions(pool: &DbPool, series_id: u64) -> Vec<(u64, i32, bool)> {
    sqlx::query_as(
        "SELECT title_id, position_number, is_omnibus FROM title_series \
         WHERE series_id = ? AND deleted_at IS NULL ORDER BY position_number",
    )
    .bind(series_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Live `(position, title, isbn, origin, settled_title_id)` of a series.
async fn tomes(
    pool: &DbPool,
    series_id: u64,
) -> Vec<(i32, String, Option<String>, String, Option<u64>)> {
    sqlx::query_as(
        "SELECT position, title, isbn, origin, settled_title_id FROM series_expected_tomes \
         WHERE series_id = ? AND deleted_at IS NULL ORDER BY position",
    )
    .bind(series_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn series_named(pool: &DbPool, name: &str) -> Option<u64> {
    sqlx::query_scalar("SELECT id FROM series WHERE name = ? AND deleted_at IS NULL")
        .bind(name)
        .fetch_optional(pool)
        .await
        .unwrap()
}

async fn closed_series(pool: &DbPool, name: &str, total: i32) -> u64 {
    sqlx::query(
        "INSERT INTO series (name, series_type, total_volume_count) VALUES (?, 'closed', ?)",
    )
    .bind(name)
    .bind(total)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id()
}

async fn assign(pool: &DbPool, title: u64, series: u64, position: i32) {
    sqlx::query("INSERT INTO title_series (title_id, series_id, position_number) VALUES (?, ?, ?)")
        .bind(title)
        .bind(series)
        .bind(position)
        .execute(pool)
        .await
        .unwrap();
}

// ─── series_create ────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn accepting_a_creation_applies_every_line(pool: DbPool) {
    let f = fx(&pool).await;
    let t1 = seed_title(&pool, "Tome un").await;
    let t2 = seed_title(&pool, "Intégrale deux-trois").await;
    // Carries the ISBN of the tome proposed at position 4: settled at once.
    let owned = add_title(&pool, "Tome quatre", "book", "Roman", Some(ISBN_A)).await;
    let id = file(
        &f,
        "propose_series_create",
        json!({
            "name": "Les Aventures <de> Test",
            "series_type": "closed",
            "total_volume_count": 5,
            "titles": [
                {"title_id": t1, "position": 1},
                {"title_id": t2, "position": 2, "is_omnibus": true, "end_position": 3}
            ],
            "expected_tomes": [
                {"position": 4, "title": "Tome quatre", "isbn": ISBN_A},
                {"position": 5, "title": "Tome cinq"}
            ],
            "sources": [SRC]
        }),
    )
    .await;

    // The modal summarises what is applied.
    let v = version_of(&pool, id).await;
    let res = f
        .lib
        .send(
            &f.agent.app,
            "GET",
            &format!("/proposals/{id}/accept-modal?version={v}"),
            "",
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let modal = body_string(res).await;
    assert!(
        modal.contains(&format!("hx-post=\"/proposals/{id}/accept\"")),
        "{modal}"
    );
    assert!(
        modal.contains("Les Aventures &#60;de&#62; Test")
            || modal.contains("Les Aventures &lt;de&gt; Test"),
        "{modal}"
    );
    assert!(
        modal.contains("Titres placés dans la série : 2 — tomes attendus ajoutés : 2."),
        "{modal}"
    );
    assert!(!modal.contains("<de>"), "agent text escaped");

    let res = f
        .lib
        .send(
            &f.agent.app,
            "POST",
            &format!("/proposals/{id}/accept"),
            &format!("version={v}"),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["hx-trigger-after-swap"], "modal-close");
    let body = body_string(res).await;
    assert!(body.contains("data-proposal-status=\"accepted\""), "{body}");
    assert!(!body.contains("id=\"proposal-accept-button\""));
    assert!(!body.contains("id=\"proposal-reject-button\""));
    assert!(body.contains("pa-lib"), "decider shown");
    assert!(
        !body.contains("<de>"),
        "agent text escaped in the body and the entry"
    );
    assert!(
        body.contains("Série « Les Aventures &#60;de&#62; Test » créée")
            || body.contains("Série « Les Aventures &lt;de&gt; Test » créée"),
        "{body}"
    );

    let sid = series_named(&pool, "Les Aventures <de> Test")
        .await
        .expect("series created");
    assert!(
        body.contains(&format!("href=\"/series/{sid}\"")),
        "link to the series"
    );
    let (st, total): (String, Option<i32>) =
        sqlx::query_as("SELECT series_type, total_volume_count FROM series WHERE id = ?")
            .bind(sid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((st.as_str(), total), ("closed", Some(5)));
    assert_eq!(
        positions(&pool, sid).await,
        vec![
            (t1, 1, false),
            (t2, 2, true),
            (t2, 3, true),
            (owned, 4, false)
        ]
    );
    let tomes = tomes(&pool, sid).await;
    assert_eq!(tomes.len(), 2);
    assert_eq!(
        (
            tomes[0].0,
            tomes[0].2.as_deref(),
            tomes[0].3.as_str(),
            tomes[0].4
        ),
        (4, Some(ISBN_A), "proposal", Some(owned))
    );
    assert_eq!(
        (tomes[1].0, tomes[1].3.as_str(), tomes[1].4),
        (5, "proposal", None)
    );

    let (status, by, at, applied) = decision(&pool, id).await;
    assert_eq!(status, "accepted");
    assert_eq!(by, Some(f.lib_id as i64));
    assert!(at);
    assert_eq!(applied, Some(sid as i64));

    // The page, reloaded, links the created series.
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert!(page.contains(&format!("href=\"/series/{sid}\"")));
    assert!(!page.contains("id=\"proposal-accept-button\""));
}

#[sqlx::test(migrations = "./migrations")]
async fn the_feedback_names_the_result_and_links_the_series(pool: DbPool) {
    let f = fx(&pool).await;
    let t1 = seed_title(&pool, "Seul").await;
    let id = file(
        &f,
        "propose_series_create",
        json!({"name": "Résultat", "series_type": "open",
               "titles": [{"title_id": t1, "position": 1}],
               "expected_tomes": [{"position": 2, "title": "Deux"}], "sources": [SRC]}),
    )
    .await;
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK);
    let sid = series_named(&pool, "Résultat").await.unwrap();
    assert!(body.contains("hx-swap-oob"), "{body}");
    assert!(body.contains("data-feedback-variant=\"success\""));
    assert!(
        body.contains(&format!("Proposition n° {id} acceptée.")),
        "{body}"
    );
    assert!(
        body.contains("titres placés : 1, tomes attendus ajoutés : 1"),
        "{body}"
    );
    assert!(body.contains(&format!("href=\"/series/{sid}\"")));
    assert!(body.contains("Ouvrir la série"));
}

#[sqlx::test(migrations = "./migrations")]
async fn a_tome_whose_book_is_placed_elsewhere_is_skipped(pool: DbPool) {
    // #529-1: the tome's ISBN belongs to a title the same proposal puts
    // at another position.
    let f = fx(&pool).await;
    let t = add_title(&pool, "Le vrai tome", "book", "Roman", Some(ISBN_B)).await;
    let id = file(
        &f,
        "propose_series_create",
        json!({"name": "Doublon interne", "series_type": "open",
               "titles": [{"title_id": t, "position": 1}],
               "expected_tomes": [{"position": 2, "title": "Le même", "isbn": ISBN_B}],
               "sources": [SRC]}),
    )
    .await;
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let sid = series_named(&pool, "Doublon interne").await.unwrap();
    assert_eq!(positions(&pool, sid).await, vec![(t, 1, false)]);
    assert!(tomes(&pool, sid).await.is_empty(), "tome skipped");
    assert!(
        body.contains("« Le même » (position 2) n'a pas été ajouté"),
        "{body}"
    );
    assert!(
        body.contains("« Le vrai tome » est déjà à la position 1"),
        "{body}"
    );
    assert_eq!(decision(&pool, id).await.0, "accepted");
}

// ─── series_attach ────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn accepting_an_attach_completes_the_series(pool: DbPool) {
    let f = fx(&pool).await;
    let sid = closed_series(&pool, "Thorgal", 6).await;
    let t0 = seed_title(&pool, "Thorgal 1").await;
    assign(&pool, t0, sid, 1).await;
    let t2 = seed_title(&pool, "Thorgal 2").await;
    let id = file(
        &f,
        "propose_series_attach",
        json!({"series_id": sid,
               "titles": [{"title_id": t2, "position": 2}],
               "expected_tomes": [{"position": 3, "title": "Thorgal 3", "isbn": ISBN_A}],
               "sources": [SRC]}),
    )
    .await;
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("Série « Thorgal » complétée"), "{body}");
    assert_eq!(
        positions(&pool, sid).await,
        vec![(t0, 1, false), (t2, 2, false)]
    );
    let tomes = tomes(&pool, sid).await;
    assert_eq!(tomes.len(), 1);
    assert_eq!((tomes[0].0, tomes[0].3.as_str()), (3, "proposal"));
    let (status, by, _, applied) = decision(&pool, id).await;
    assert_eq!(
        (status.as_str(), by, applied),
        ("accepted", Some(f.lib_id as i64), None)
    );
}

// ─── Staleness ────────────────────────────────────────────────────

async fn assert_stale_and_untouched(pool: &DbPool, id: u64, body: &str) {
    assert!(body.contains("data-proposal-status=\"stale\""), "{body}");
    assert!(body.contains("data-feedback-variant=\"warning\""), "{body}");
    assert!(
        body.contains(&format!(
            "La proposition n° {id} est désormais périmée et rien n'a été ajouté au catalogue"
        )),
        "{body}"
    );
    let (status, by, at, applied) = decision(pool, id).await;
    assert_eq!(
        (status.as_str(), by, at, applied),
        ("stale", None, true, None)
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_title_edited_since_filing_makes_it_stale(pool: DbPool) {
    let f = fx(&pool).await;
    let t = seed_title(&pool, "Modifié").await;
    let id = file(
        &f,
        "propose_series_create",
        json!({"name": "Jamais créée", "series_type": "open",
               "titles": [{"title_id": t, "position": 1}], "sources": [SRC]}),
    )
    .await;
    sqlx::query("UPDATE titles SET subtitle = 'x', version = version + 1 WHERE id = ?")
        .bind(t)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK);
    assert_stale_and_untouched(&pool, id, &body).await;
    assert!(
        body.contains("modifié ou supprimé après le dépôt"),
        "{body}"
    );
    assert_eq!(series_named(&pool, "Jamais créée").await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_series_makes_an_attach_stale(pool: DbPool) {
    let f = fx(&pool).await;
    let sid = closed_series(&pool, "Effacée", 4).await;
    let t = seed_title(&pool, "Orphelin").await;
    let id = file(
        &f,
        "propose_series_attach",
        json!({"series_id": sid, "titles": [{"title_id": t, "position": 1}], "sources": [SRC]}),
    )
    .await;
    soft_delete(&pool, "series", sid).await;
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK);
    assert_stale_and_untouched(&pool, id, &body).await;
    assert!(positions(&pool, sid).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_position_assigned_by_hand_makes_it_stale(pool: DbPool) {
    // #521: a `title_series` write bumps no version; the state check
    // finds it.
    let f = fx(&pool).await;
    let sid = closed_series(&pool, "Occupée", 5).await;
    let t = seed_title(&pool, "Proposé").await;
    let other = seed_title(&pool, "Arrivé avant").await;
    let id = file(
        &f,
        "propose_series_attach",
        json!({"series_id": sid, "titles": [{"title_id": t, "position": 2}],
               "expected_tomes": [{"position": 3, "title": "Trois"}], "sources": [SRC]}),
    )
    .await;
    assign(&pool, other, sid, 2).await;
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK);
    assert_stale_and_untouched(&pool, id, &body).await;
    assert!(
        body.contains("la position 2 est maintenant occupée par « Arrivé avant »"),
        "{body}"
    );
    assert_eq!(positions(&pool, sid).await, vec![(other, 2, false)]);
    assert!(tomes(&pool, sid).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_title_attached_by_hand_makes_it_stale(pool: DbPool) {
    // #529-3: the same title placed in the series by another route.
    let f = fx(&pool).await;
    let sid = closed_series(&pool, "Déjà là", 5).await;
    let t = seed_title(&pool, "Voyageur").await;
    let id = file(
        &f,
        "propose_series_attach",
        json!({"series_id": sid, "titles": [{"title_id": t, "position": 2}], "sources": [SRC]}),
    )
    .await;
    assign(&pool, t, sid, 4).await;
    let (_, body) = accept(&f, &pool, id).await;
    assert_stale_and_untouched(&pool, id, &body).await;
    assert!(
        body.contains("« Voyageur » est déjà dans la série, à la position 4"),
        "{body}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_tome_entered_by_hand_makes_it_stale(pool: DbPool) {
    let f = fx(&pool).await;
    let sid = closed_series(&pool, "Attendue", 5).await;
    let id = file(
        &f,
        "propose_series_attach",
        json!({"series_id": sid, "expected_tomes": [{"position": 3, "title": "Trois"}],
               "sources": [SRC]}),
    )
    .await;
    ExpectedTomeService::create(
        &pool,
        sid,
        &ExpectedTomeInput {
            position: 3,
            title: "Trois (à la main)",
            isbn: None,
        },
        ExpectedTomeOrigin::Manual,
    )
    .await
    .unwrap();
    let (_, body) = accept(&f, &pool, id).await;
    assert_stale_and_untouched(&pool, id, &body).await;
    assert!(
        body.contains("la position 3 a maintenant le tome attendu « Trois (à la main) »"),
        "{body}"
    );
    assert_eq!(tomes(&pool, sid).await.len(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_series_created_meanwhile_makes_a_creation_stale(pool: DbPool) {
    // #529-2: another key's creation of the same series was accepted (or
    // a librarian created it by hand) — case- and accent-insensitive.
    let f = fx(&pool).await;
    let id = file(
        &f,
        "propose_series_create",
        json!({"name": "Élan", "series_type": "open",
               "expected_tomes": [{"position": 1, "title": "Un"}], "sources": [SRC]}),
    )
    .await;
    sqlx::query("INSERT INTO series (name, series_type) VALUES ('elan', 'open')")
        .execute(&pool)
        .await
        .unwrap();
    let (_, body) = accept(&f, &pool, id).await;
    assert_stale_and_untouched(&pool, id, &body).await;
    assert!(
        body.contains("une série nommée « elan » existe désormais"),
        "{body}"
    );
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM series_expected_tomes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

// ─── Rollback and refusals ────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_failure_while_applying_rolls_everything_back(pool: DbPool) {
    let f = fx(&pool).await;
    let t = seed_title(&pool, "Rollback").await;
    let id = file(
        &f,
        "propose_series_create",
        json!({"name": "Annulée", "series_type": "open",
               "titles": [{"title_id": t, "position": 1}],
               "expected_tomes": [{"position": 2, "title": "Deux"}], "sources": [SRC]}),
    )
    .await;
    // The tome insert — the last write — fails.
    sqlx::query(
        "CREATE TRIGGER fail_tome_insert BEFORE INSERT ON series_expected_tomes \
         FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'injected failure'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    sqlx::query("DROP TRIGGER fail_tome_insert")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body.contains("rien n'a été modifié et elle est toujours en attente"),
        "{body}"
    );
    assert!(!body.contains("injected"), "no database text");
    assert_eq!(series_named(&pool, "Annulée").await, None);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM title_series WHERE title_id = ?")
        .bind(t)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let (status, by, at, applied) = decision(&pool, id).await;
    assert_eq!(
        (status.as_str(), by, at, applied),
        ("pending", None, false, None)
    );

    // Once the failure is gone, the same proposal is accepted.
    let (status, _) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK);
    assert!(series_named(&pool, "Annulée").await.is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn refusals_change_nothing(pool: DbPool) {
    let f = fx(&pool).await;
    let id = file(
        &f,
        "propose_series_create",
        json!({"name": "Refus", "series_type": "open",
               "expected_tomes": [{"position": 1, "title": "Un"}], "sources": [SRC]}),
    )
    .await;
    let path = format!("/proposals/{id}/accept");
    let v = version_of(&pool, id).await;

    // A version that moved, or none.
    for form in [
        format!("version={}", v + 1),
        String::new(),
        "version=abc".to_string(),
        "version=99999999999".to_string(),
    ] {
        let (status, body) = f.lib.post_form(&f.agent.app, &path, &form).await;
        let body = text(body);
        assert_eq!(status, StatusCode::CONFLICT, "{form}");
        assert!(
            body.contains("a changé depuis que vous l'avez ouverte"),
            "{body}"
        );
    }
    // The modal needs the page's version.
    for q in ["", "?version=", "?version=abc"] {
        let res = f
            .lib
            .send(
                &f.agent.app,
                "GET",
                &format!("/proposals/{id}/accept-modal{q}"),
                "",
                true,
            )
            .await;
        assert_eq!(res.status(), StatusCode::CONFLICT, "{q}");
    }
    // Not through HTMX.
    let res = f
        .lib
        .send(
            &f.agent.app,
            "GET",
            &format!("/proposals/{id}/accept-modal?version={v}"),
            "",
            false,
        )
        .await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    let (status, _) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK);
    // A second accept is refused, naming the status.
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body.contains("Acceptée") || body.contains("acceptée"),
        "{body}"
    );
    assert_eq!(decision(&pool, id).await.0, "accepted");
}

#[sqlx::test(migrations = "./migrations")]
async fn csrf_and_role_are_required(pool: DbPool) {
    let f = fx(&pool).await;
    let id = file(
        &f,
        "propose_series_create",
        json!({"name": "Gardée", "series_type": "open",
               "expected_tomes": [{"position": 1, "title": "Un"}], "sources": [SRC]}),
    )
    .await;
    let v = version_of(&pool, id).await;
    let path = format!("/proposals/{id}/accept");
    let forged = Browser {
        cookie: f.lib.cookie.clone(),
        csrf: "forged".into(),
    };
    let (status, _) = forged
        .post_form(&f.agent.app, &path, &format!("version={v}"))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let anon = Browser::anonymous(&f.agent.app).await;
    let res = anon
        .send(&f.agent.app, "POST", &path, &format!("version={v}"), true)
        .await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(decision(&pool, id).await.0, "pending");
    assert_eq!(series_named(&pool, "Gardée").await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn rejected_and_metadata_proposals_cannot_be_accepted(pool: DbPool) {
    let f = fx(&pool).await;
    let id = file(
        &f,
        "propose_series_create",
        json!({"name": "Refusée", "series_type": "open",
               "expected_tomes": [{"position": 1, "title": "Un"}], "sources": [SRC]}),
    )
    .await;
    let v = version_of(&pool, id).await;
    let (status, _) = f
        .lib
        .post_form(
            &f.agent.app,
            &format!("/proposals/{id}/reject"),
            &format!("version={v}"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body.contains("Refusée") || body.contains("refusée"),
        "{body}"
    );
    assert_eq!(series_named(&pool, "Refusée").await, None);

    // A metadata proposal shows no Accept button and its modal refuses.
    let t = seed_title(&pool, "Métadonnées").await;
    let meta = file(
        &f,
        "propose_title_metadata",
        json!({"title_id": t, "fields": {"subtitle": "S"}, "sources": [SRC]}),
    )
    .await;
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{meta}")).await;
    assert!(page.contains("id=\"proposal-reject-button\""));
    assert!(!page.contains("id=\"proposal-accept-button\""));
    let mv = version_of(&pool, meta).await;
    let res = f
        .lib
        .send(
            &f.agent.app,
            "GET",
            &format!("/proposals/{meta}/accept-modal?version={mv}"),
            "",
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let (status, _) = accept(&f, &pool, meta).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(decision(&pool, meta).await.0, "pending");

    // A pending series proposal shows both buttons.
    let id2 = file(
        &f,
        "propose_series_create",
        json!({"name": "Ouverte", "series_type": "open",
               "expected_tomes": [{"position": 1, "title": "Un"}], "sources": [SRC]}),
    )
    .await;
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id2}")).await;
    assert!(page.contains("id=\"proposal-accept-button\""));
    assert!(page.contains(&format!(
        "hx-get=\"/proposals/{id2}/accept-modal?version={}\"",
        version_of(&pool, id2).await
    )));
}

// ─── Two proposals for the same series (#529-2, #529-3) ───────────

/// File `a`, then `b`, as if two keys filed them at the same instant:
/// the pending-duplicate rule is sidestepped by hiding `a` while `b` is
/// filed (resubmission delay off, restored).
async fn file_both(f: &Fx, pool: &DbPool, tool: &str, a: Value, b: Value) -> (u64, u64) {
    sqlx::query(
        "UPDATE settings SET setting_value = '0' \
         WHERE setting_key = 'proposal_resubmission_delay_days'",
    )
    .execute(pool)
    .await
    .unwrap();
    let first = file(f, tool, a).await;
    sqlx::query("UPDATE proposals SET status = 'rejected' WHERE id = ?")
        .bind(first)
        .execute(pool)
        .await
        .unwrap();
    let second = file(f, tool, b).await;
    sqlx::query("UPDATE proposals SET status = 'pending' WHERE id = ?")
        .bind(first)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE settings SET setting_value = '90' \
         WHERE setting_key = 'proposal_resubmission_delay_days'",
    )
    .execute(pool)
    .await
    .unwrap();
    (first, second)
}

#[sqlx::test(migrations = "./migrations")]
async fn the_second_of_two_attaches_on_the_same_positions_goes_stale(pool: DbPool) {
    let f = fx(&pool).await;
    let sid = closed_series(&pool, "Disputée", 5).await;
    let ta = seed_title(&pool, "Premier arrivé").await;
    let tb = seed_title(&pool, "Second arrivé").await;
    let (a, b) = file_both(
        &f,
        &pool,
        "propose_series_attach",
        json!({"series_id": sid, "titles": [{"title_id": ta, "position": 2}], "sources": [SRC]}),
        json!({"series_id": sid, "titles": [{"title_id": tb, "position": 2}], "sources": [SRC]}),
    )
    .await;
    assert_eq!(accept(&f, &pool, a).await.0, StatusCode::OK);
    let (status, body) = accept(&f, &pool, b).await;
    assert_eq!(status, StatusCode::OK);
    assert_stale_and_untouched(&pool, b, &body).await;
    assert!(
        body.contains("la position 2 est maintenant occupée par « Premier arrivé »"),
        "{body}"
    );
    assert_eq!(positions(&pool, sid).await, vec![(ta, 2, false)]);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_second_of_two_creations_of_one_name_goes_stale(pool: DbPool) {
    let f = fx(&pool).await;
    let (a, b) = file_both(
        &f,
        &pool,
        "propose_series_create",
        json!({"name": "Jumelle", "series_type": "open",
               "expected_tomes": [{"position": 1, "title": "Un"}], "sources": [SRC]}),
        json!({"name": "jumelle", "series_type": "open",
               "expected_tomes": [{"position": 2, "title": "Deux"}], "sources": [SRC]}),
    )
    .await;
    assert_eq!(accept(&f, &pool, a).await.0, StatusCode::OK);
    let (_, body) = accept(&f, &pool, b).await;
    assert_stale_and_untouched(&pool, b, &body).await;
    assert!(
        body.contains("une série nommée « Jumelle » existe désormais"),
        "{body}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_creations_of_one_name_create_one_series(pool: DbPool) {
    concurrent_creations(&pool, false).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn concurrent_creations_queue_on_a_non_empty_series_table(pool: DbPool) {
    // With rows to scan, the second locking read waits on the first
    // instead of deadlocking on the supremum gap.
    concurrent_creations(&pool, true).await;
}

async fn concurrent_creations(pool: &DbPool, other_series: bool) {
    let pool = pool.clone();
    if other_series {
        closed_series(&pool, "Sans rapport", 3).await;
    }
    // The same-name check is a locking read: the two acceptances are
    // serialised (or one is retried after a deadlock), never both applied.
    let f = fx(&pool).await;
    let (a, b) = file_both(
        &f,
        &pool,
        "propose_series_create",
        json!({"name": "Course", "series_type": "open",
               "expected_tomes": [{"position": 1, "title": "Un"}], "sources": [SRC]}),
        json!({"name": "Course", "series_type": "open",
               "expected_tomes": [{"position": 2, "title": "Deux"}], "sources": [SRC]}),
    )
    .await;
    let (va, vb) = (version_of(&pool, a).await, version_of(&pool, b).await);
    let (ra, rb) = tokio::join!(
        series_acceptance::accept(&pool, a, f.lib_id, va),
        series_acceptance::accept(&pool, b, f.lib_id, vb),
    );
    let outcomes = [ra.expect("a"), rb.expect("b")];
    let accepted = outcomes
        .iter()
        .filter(|o| matches!(o, AcceptOutcome::Accepted(_)))
        .count();
    let stale = outcomes
        .iter()
        .filter(|o| {
            matches!(
                o,
                AcceptOutcome::Stale(StaleCause::Conflict(AcceptConflict::SeriesExists { .. }))
            )
        })
        .count();
    assert_eq!((accepted, stale), (1, 1), "{outcomes:?}");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM series WHERE name = 'Course'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn any_owner_of_the_isbn_already_in_the_series_skips_the_tome(pool: DbPool) {
    // Two titles share the ISBN (two copies or editions); the first by id
    // is not in the series, the second sits at position 1: the tome must
    // not attach the first one at position 2.
    let f = fx(&pool).await;
    let sid = closed_series(&pool, "Éditions", 4).await;
    let first = add_title(&pool, "Première édition", "book", "Roman", Some(ISBN_A)).await;
    let second = add_title(&pool, "Seconde édition", "book", "Roman", Some(ISBN_A)).await;
    assert!(first < second);
    assign(&pool, second, sid, 1).await;
    let id = file(
        &f,
        "propose_series_attach",
        json!({"series_id": sid,
               "expected_tomes": [{"position": 2, "title": "Le même livre", "isbn": ISBN_A}],
               "sources": [SRC]}),
    )
    .await;
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(positions(&pool, sid).await, vec![(second, 1, false)]);
    assert!(tomes(&pool, sid).await.is_empty());
    assert!(
        body.contains("« Seconde édition » est déjà à la position 1"),
        "{body}"
    );
}
