//! Story 11-16 (#493) — the proposals page: list, detail, staged cover,
//! rejection through the UX-DR8 modal (FR137, FR138 reject, FR146).
//!
//! Drives the full router against a fresh `#[sqlx::test]` database.
//! Proposals are filed by an agent through the real MCP tools, so the
//! items carry the shapes of stories 11-12..11-14.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test proposals_page

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use common::mcp::*;
use common::web::{Browser, PW_HASH, body_string, get_anonymous, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::api_key::ApiKeyScope;
use mybibli::services::cover_staging;
use serde_json::{Value, json};
use tower::ServiceExt;

const SRC: &str = "https://example.org/notice?a=1&b=2";

struct Fixture {
    _dir: tempfile::TempDir,
    staging: std::path::PathBuf,
    agent: Agent,
}

async fn fixture(pool: &DbPool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let covers = dir.path().join("covers");
    let app = router_with_covers(pool, &covers);
    Fixture {
        staging: cover_staging::staging_dir_for(&covers),
        _dir: dir,
        agent: Agent {
            app,
            key: seed_key(pool, ApiKeyScope::Propose).await,
        },
    }
}

async fn seed_librarian(pool: &DbPool, username: &str) -> u64 {
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
        .bind(username)
        .bind(PW_HASH)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

/// An admin exists (setup gate) and a librarian is logged in.
async fn librarian(pool: &DbPool, f: &Fixture) -> (Browser, u64) {
    seed_admin(pool, "pp-admin").await;
    let id = seed_librarian(pool, "pp-lib").await;
    (Browser::login(&f.agent.app, "pp-lib").await, id)
}

fn id_of(doc: &Value) -> u64 {
    doc["proposal_id"].as_u64().expect("proposal_id")
}

async fn file_metadata(f: &Fixture, title_id: u64, fields: Value) -> u64 {
    id_of(
        &f.agent
            .ok(
                "propose_title_metadata",
                json!({"title_id": title_id, "fields": fields, "sources": [SRC]}),
            )
            .await,
    )
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::DynamicImage::new_rgb8(w, h)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

async fn file_cover(f: &Fixture, title_id: u64) -> u64 {
    id_of(
        &f.agent
            .ok(
                "propose_cover",
                json!({
                    "title_id": title_id,
                    "image_base64": base64::engine::general_purpose::STANDARD.encode(png(40, 60)),
                    "media_type": "image/png",
                    "sources": [SRC]
                }),
            )
            .await,
    )
}

async fn staged_file_of(pool: &DbPool, id: u64) -> String {
    sqlx::query_scalar(
        "SELECT CAST(JSON_UNQUOTE(JSON_EXTRACT(proposed_values, '$.staged_file')) AS CHAR) \
         FROM proposal_items WHERE proposal_id = ?",
    )
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

/// `(status, decided_by, decided_at is set, rejection_reason)`.
async fn decision_of(pool: &DbPool, id: u64) -> (String, Option<i64>, bool, Option<String>) {
    sqlx::query_as(
        "SELECT status, CAST(decided_by AS SIGNED), decided_at IS NOT NULL, rejection_reason \
         FROM proposals WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn add_series(pool: &DbPool, name: &str) -> u64 {
    sqlx::query("INSERT INTO series (name, series_type) VALUES (?, 'open')")
        .bind(name)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_id()
}

fn form(version: i32, reason: &str) -> String {
    format!(
        "version={version}&reason={}",
        percent_encoding::utf8_percent_encode(reason, percent_encoding::NON_ALPHANUMERIC)
    )
}

// ─── Access ───────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_visitors_are_sent_to_login(pool: DbPool) {
    let f = fixture(&pool).await;
    seed_admin(&pool, "pp-admin").await;
    let title = seed_title(&pool, "Accès").await;
    let id = file_metadata(&f, title, json!({"subtitle": "S"})).await;
    for path in [
        "/proposals".to_string(),
        format!("/proposals/{id}"),
        format!("/proposals/{id}/cover"),
    ] {
        let res = get_anonymous(&f.agent.app, &path).await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER, "{path}");
        let to = res.headers()["location"].to_str().unwrap().to_string();
        assert!(to.starts_with("/login"), "{path} → {to}");
    }
    let anon = Browser::anonymous(&f.agent.app).await;
    let res = anon
        .send(
            &f.agent.app,
            "POST",
            &format!("/proposals/{id}/reject"),
            "version=1",
            true,
        )
        .await;
    assert_ne!(res.status(), StatusCode::OK);
    assert_eq!(decision_of(&pool, id).await.0, "pending");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_librarian_opens_the_page_with_a_feedback_list(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, _) = librarian(&pool, &f).await;
    let (status, html) = lib.get(&f.agent.app, "/proposals").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("id=\"feedback-list\""));
    assert!(html.contains("id=\"proposals-empty\""), "empty state");
    assert!(!html.contains("proposals."), "raw locale key leaked");
}

// ─── List ─────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_list_filters_by_status_and_kind_and_names_targets(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, lib_id) = librarian(&pool, &f).await;
    let title = seed_title(&pool, "Le <Titre> & co").await;
    let gone = seed_title(&pool, "Disparu").await;
    let meta = file_metadata(&f, title, json!({"subtitle": "Sous-titre"})).await;
    let cover = file_cover(&f, title).await;
    let created = id_of(
        &f.agent
            .ok(
                "propose_series_create",
                json!({"name": "Yakari", "series_type": "open", "sources": [SRC]}),
            )
            .await,
    );
    let orphan = file_metadata(&f, gone, json!({"publisher": "Dupuis"})).await;
    soft_delete(&pool, "titles", gone).await;
    mybibli::services::proposals::ProposalService::transition(
        &pool,
        created,
        &mybibli::services::proposals::Decision::Reject {
            by: lib_id,
            reason: None,
        },
    )
    .await
    .unwrap();

    let row = |id: u64| format!("data-proposal-id=\"{id}\"");
    // Default: pending only.
    let (_, html) = lib.get(&f.agent.app, "/proposals").await;
    for id in [meta, cover, orphan] {
        assert!(html.contains(&row(id)), "pending {id}");
    }
    assert!(!html.contains(&row(created)), "rejected hidden by default");
    assert!(html.contains("Le &#60;Titre&#62; &#38; co"), "escaped title");
    assert!(html.contains(&format!("href=\"/title/{title}\"")));
    assert!(
        !html.contains(&format!("href=\"/title/{gone}\"")),
        "deleted target not linked"
    );
    assert!(html.contains("mcp-propose"), "key label");
    assert!(html.contains("data-proposal-status=\"pending\""));

    let (_, html) = lib.get(&f.agent.app, "/proposals?status=rejected").await;
    assert!(html.contains(&row(created)));
    assert!(html.contains("Yakari"));
    assert!(!html.contains(&row(meta)));

    let (_, html) = lib
        .get(&f.agent.app, "/proposals?status=all&kind=cover")
        .await;
    assert!(html.contains(&row(cover)));
    assert!(!html.contains(&row(meta)));
    assert!(!html.contains(&row(created)));
    assert!(html.contains("value=\"cover\" selected"));

    // Unknown values fall back to the defaults.
    let (status, html) = lib
        .get(&f.agent.app, "/proposals?status=zzz&kind=zzz&page=x")
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(&row(meta)));
    assert!(!html.contains(&row(created)));
}

#[sqlx::test(migrations = "./migrations")]
async fn the_list_paginates_and_keeps_the_filters(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, _) = librarian(&pool, &f).await;
    let title = seed_title(&pool, "Page").await;
    let first = file_metadata(&f, title, json!({"subtitle": "S"})).await;
    let key_id: u64 = sqlx::query_scalar("SELECT api_key_id FROM proposals WHERE id = ?")
        .bind(first)
        .fetch_one(&pool)
        .await
        .unwrap();
    for _ in 0..25 {
        sqlx::query("INSERT INTO proposals (kind, api_key_id, sources) VALUES ('cover', ?, '[]')")
            .bind(key_id)
            .execute(&pool)
            .await
            .unwrap();
    }
    let (_, html) = lib.get(&f.agent.app, "/proposals").await;
    assert_eq!(html.matches("data-proposal-id=").count(), 25);
    assert!(html.contains("/proposals?status=pending&#38;kind=all&#38;page=2"));
    let (_, html) = lib
        .get(&f.agent.app, "/proposals?status=pending&kind=all&page=2")
        .await;
    assert_eq!(html.matches("data-proposal-id=").count(), 1);
    assert!(
        html.contains(&format!("data-proposal-id=\"{first}\"")),
        "oldest last"
    );
    assert!(html.contains("rel=\"prev\""));
}

// ─── Detail ───────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_detail_shows_items_escaped_and_sources_in_a_new_tab(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, _) = librarian(&pool, &f).await;
    let title = seed_title(&pool, "Détail").await;
    let id = file_metadata(
        &f,
        title,
        json!({"description": "<script>alert(1)</script>\nLigne 2", "publication_year": 1999}),
    )
    .await;
    let (status, html) = lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("id=\"feedback-list\""));
    assert!(html.contains("&#60;script&#62;alert(1)&#60;/script&#62;"));
    assert!(
        !html.contains("<script>alert(1)"),
        "agent value rendered as HTML"
    );
    assert!(html.contains(
        "href=\"https://example.org/notice?a=1&#38;b=2\" target=\"_blank\" rel=\"noopener noreferrer\""
    ));
    assert_eq!(html.matches("data-line=\"field\"").count(), 2);
    assert!(html.contains(">1999<"), "proposed year");
    assert!(
        html.contains("id=\"proposal-reject-button\""),
        "pending → Reject"
    );
    assert!(!html.contains("hx-confirm"));
    assert!(!html.contains("proposals."), "raw locale key leaked");

    let (status, _) = lib.get(&f.agent.app, "/proposals/999999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_detail_renders_series_lines(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, _) = librarian(&pool, &f).await;
    let series = add_series(&pool, "Thorgal").await;
    let t = seed_title(&pool, "Thorgal 2").await;
    let attach = id_of(
        &f.agent
            .ok(
                "propose_series_attach",
                json!({
                    "series_id": series,
                    "titles": [{"title_id": t, "position": 2}],
                    "expected_tomes": [{"position": 3, "title": "Thorgal <3>", "isbn": null}],
                    "sources": [SRC]
                }),
            )
            .await,
    );
    let (_, html) = lib.get(&f.agent.app, &format!("/proposals/{attach}")).await;
    assert!(html.contains("data-line=\"series_target\""));
    assert!(html.contains(&format!("href=\"/series/{series}\"")));
    assert!(html.contains("data-line=\"title\""));
    assert!(html.contains(&format!("href=\"/title/{t}\"")));
    assert!(html.contains("data-line=\"tome\""));
    assert!(html.contains("Thorgal &#60;3&#62;"));

    let create = id_of(
        &f.agent
            .ok(
                "propose_series_create",
                json!({"name": "Blake & Mortimer", "series_type": "closed", "total_volume_count": 30, "sources": [SRC]}),
            )
            .await,
    );
    let (_, html) = lib.get(&f.agent.app, &format!("/proposals/{create}")).await;
    assert!(html.contains("data-line=\"series_new\""));
    assert!(html.contains("Blake &#38; Mortimer"));
    assert!(html.contains(">30<"));
}

// ─── Staged cover ─────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_staged_cover_is_served_to_librarians_only_by_proposal(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, _) = librarian(&pool, &f).await;
    let title = seed_title(&pool, "Couverture").await;
    let cover = file_cover(&f, title).await;
    let meta = file_metadata(&f, title, json!({"subtitle": "S"})).await;
    let name = staged_file_of(&pool, cover).await;

    let res = lib
        .send(
            &f.agent.app,
            "GET",
            &format!("/proposals/{cover}/cover"),
            "",
            false,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "image/jpeg");
    assert_eq!(res.headers()["cache-control"], "no-store");
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(body.to_vec(), std::fs::read(f.staging.join(&name)).unwrap());

    let (_, html) = lib.get(&f.agent.app, &format!("/proposals/{cover}")).await;
    assert!(html.contains(&format!("src=\"/proposals/{cover}/cover\"")));
    assert!(
        !html.contains(&name),
        "the staged name never reaches the page"
    );

    for path in [
        format!("/proposals/{meta}/cover"),
        "/proposals/999999/cover".to_string(),
        format!("/covers/.staging/{name}"),
    ] {
        let res = lib.send(&f.agent.app, "GET", &path, "", false).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}");
    }

    // A stored name that is not a staged name is never turned into a path.
    sqlx::query(
        "UPDATE proposal_items SET proposed_values = JSON_SET(proposed_values, '$.staged_file', '../x.jpg') \
         WHERE proposal_id = ?",
    )
    .bind(cover)
    .execute(&pool)
    .await
    .unwrap();
    let res = lib
        .send(
            &f.agent.app,
            "GET",
            &format!("/proposals/{cover}/cover"),
            "",
            false,
        )
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

// ─── Reject ───────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn the_reject_modal_is_a_ux_dr8_modal_with_a_reason(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, _) = librarian(&pool, &f).await;
    let title = seed_title(&pool, "Modale").await;
    let id = file_metadata(&f, title, json!({"subtitle": "S"})).await;
    let path = format!("/proposals/{id}/reject-modal");

    let res = lib.send(&f.agent.app, "GET", &path, "", false).await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);

    let res = lib.send(&f.agent.app, "GET", &path, "", true).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html = body_string(res).await;
    assert!(html.contains("<dialog open aria-modal=\"true\""));
    assert!(html.contains(&format!("hx-post=\"/proposals/{id}/reject\"")));
    assert!(html.contains("name=\"_csrf_token\""));
    assert!(html.contains(&format!(
        "name=\"version\" value=\"{}\"",
        version_of(&pool, id).await
    )));
    assert!(html.contains("<textarea id=\"proposal-reject-reason\" name=\"reason\""));
    assert!(html.contains("maxlength=\"1000\""));
    assert!(!html.contains("hx-confirm"));
}

#[sqlx::test(migrations = "./migrations")]
async fn rejecting_with_a_reason_records_the_decision(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, lib_id) = librarian(&pool, &f).await;
    let title = seed_title(&pool, "Refus").await;
    let id = file_metadata(&f, title, json!({"subtitle": "S"})).await;
    let v = version_of(&pool, id).await;

    let res = lib
        .send(
            &f.agent.app,
            "POST",
            &format!("/proposals/{id}/reject"),
            &form(v, "  Mauvaise <édition>  "),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["hx-trigger-after-swap"], "modal-close");
    assert!(res.headers().get("hx-trigger").is_none());
    let html = body_string(res).await;
    assert!(html.contains("<div id=\"proposal-detail-body\" data-proposal-status=\"rejected\""));
    assert!(html.contains("Mauvaise &#60;édition&#62;"));
    assert!(html.contains("pp-lib"), "decider shown");
    assert!(
        !html.contains("proposal-reject-button"),
        "no Reject once decided"
    );
    assert!(html.contains("<div id=\"feedback-list\" hx-swap-oob=\"beforeend\">"));

    let (status, by, at, reason) = decision_of(&pool, id).await;
    assert_eq!(status, "rejected");
    assert_eq!(by, Some(lib_id as i64));
    assert!(at);
    assert_eq!(reason.as_deref(), Some("Mauvaise <édition>"));

    // Already decided: refused, nothing changes.
    let res = lib
        .send(
            &f.agent.app,
            "POST",
            &format!("/proposals/{id}/reject"),
            &form(v + 1, "encore"),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        decision_of(&pool, id).await.3.as_deref(),
        Some("Mauvaise <édition>")
    );

    // The modal of a decided proposal is refused too.
    let res = lib
        .send(
            &f.agent.app,
            "GET",
            &format!("/proposals/{id}/reject-modal"),
            "",
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
}

#[sqlx::test(migrations = "./migrations")]
async fn rejecting_without_a_reason_stores_none(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, lib_id) = librarian(&pool, &f).await;
    let title = seed_title(&pool, "Sans motif").await;
    let id = file_metadata(&f, title, json!({"subtitle": "S"})).await;
    let v = version_of(&pool, id).await;
    let (status, html) = lib
        .post_form(
            &f.agent.app,
            &format!("/proposals/{id}/reject"),
            &form(v, "   "),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("id=\"proposal-reason\""));
    assert_eq!(
        decision_of(&pool, id).await,
        ("rejected".to_string(), Some(lib_id as i64), true, None)
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn refused_rejections_change_nothing(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, _) = librarian(&pool, &f).await;
    let title = seed_title(&pool, "Refusés").await;
    let id = file_metadata(&f, title, json!({"subtitle": "S"})).await;
    let v = version_of(&pool, id).await;
    let path = format!("/proposals/{id}/reject");

    // Too long (1001 characters, multi-byte).
    let (status, html) = lib
        .post_form(&f.agent.app, &path, &form(v, &"é".repeat(1001)))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(html.contains("1000"), "{html}");
    // Exactly 1000 would be accepted; first check the other refusals.

    // A version that moved.
    let (status, _) = lib.post_form(&f.agent.app, &path, &form(v + 7, "x")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    // Missing version reads as 0.
    let (status, _) = lib.post_form(&f.agent.app, &path, "reason=x").await;
    assert_eq!(status, StatusCode::CONFLICT);
    // Unknown proposal.
    let (status, _) = lib
        .post_form(&f.agent.app, "/proposals/999999/reject", &form(1, ""))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // No CSRF token.
    let res = f
        .agent
        .app
        .clone()
        .oneshot(
            Request::post(&path)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", format!("session={}", lib.cookie))
                .header("HX-Request", "true")
                .body(Body::from(form(v, "x")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    assert_eq!(decision_of(&pool, id).await.0, "pending");
    assert_eq!(version_of(&pool, id).await, v);

    let (status, _) = lib
        .post_form(&f.agent.app, &path, &form(v, &"é".repeat(1000)))
        .await;
    assert_eq!(status, StatusCode::OK, "1000 characters are accepted");
}

#[sqlx::test(migrations = "./migrations")]
async fn rejecting_a_cover_proposal_discards_its_staged_file(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, _) = librarian(&pool, &f).await;
    let title = seed_title(&pool, "Couverture refusée").await;
    let id = file_cover(&f, title).await;
    let name = staged_file_of(&pool, id).await;
    assert!(f.staging.join(&name).exists());

    let (status, html) = lib
        .post_form(
            &f.agent.app,
            &format!("/proposals/{id}/reject"),
            &form(version_of(&pool, id).await, "Floue"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!f.staging.join(&name).exists(), "staged cover discarded");
    assert_eq!(decision_of(&pool, id).await.0, "rejected");
    // The page now says the image is gone; the route answers 404.
    assert!(
        !html.contains("proposal-staged-cover"),
        "no broken image once decided"
    );
    let res = lib
        .send(
            &f.agent.app,
            "GET",
            &format!("/proposals/{id}/cover"),
            "",
            false,
        )
        .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn after_a_web_rejection_the_agent_is_refused_and_sees_the_reason(pool: DbPool) {
    let f = fixture(&pool).await;
    let (lib, _) = librarian(&pool, &f).await;
    let title = seed_title(&pool, "Renvoi").await;
    let id = file_metadata(&f, title, json!({"subtitle": "S"})).await;
    let (status, _) = lib
        .post_form(
            &f.agent.app,
            &format!("/proposals/{id}/reject"),
            &form(version_of(&pool, id).await, "Pas cette édition"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let msg = f
        .agent
        .err(
            "propose_title_metadata",
            json!({"title_id": title, "fields": {"subtitle": "Autre"}, "sources": [SRC]}),
        )
        .await;
    assert!(msg.contains("Pas cette édition"), "{msg}");
    let page = f
        .agent
        .ok("list_my_proposals", json!({"status": "rejected"}))
        .await;
    assert_eq!(page["items"][0]["rejection_reason"], "Pas cette édition");
    assert!(page["items"][0]["resubmittable_from"].is_string());
}
