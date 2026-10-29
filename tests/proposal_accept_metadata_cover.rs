//! Story 11-18 (#493) — accepting a `metadata` or `cover` proposal
//! (FR138, FR140, FR144, FR146): fields and contributors written in one
//! transaction, the staged cover promoted after the commit, staleness by
//! version and by the values recorded at filing (#521), the missing
//! staged file refused (#533), re-download protection (#232), and the
//! removed-contributor regression (#531) — through the real router.
//!
//! Proposals are filed by an agent through the real MCP tools, so their
//! items carry the shapes of stories 11-13 / 11-14.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test proposal_accept_metadata_cover

mod common;

use axum::Router;
use axum::http::StatusCode;
use base64::Engine;
use common::mcp::*;
use common::web::{Browser, PW_HASH, seed_admin};
use mybibli::db::DbPool;
use mybibli::metadata::provider::MetadataResult;
use mybibli::models::api_key::ApiKeyScope;
use mybibli::services::contributor::ContributorService;
use mybibli::services::cover_staging;
use mybibli::tasks::metadata_fetch::{update_cover_image_url, update_title_from_metadata};
use serde_json::{Value, json};

const SRC: &str = "https://example.org/notice";

struct Fx {
    _dir: tempfile::TempDir,
    covers: std::path::PathBuf,
    staging: std::path::PathBuf,
    agent: Agent,
    lib: Browser,
    lib_id: u64,
}

async fn fx(pool: &DbPool) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let covers = dir.path().join("covers");
    let app: Router = router_with_covers(pool, &covers);
    let agent = Agent {
        app,
        key: seed_key(pool, ApiKeyScope::Propose).await,
    };
    seed_admin(pool, "am-admin").await;
    let lib_id =
        sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
            .bind("am-lib")
            .bind(PW_HASH)
            .execute(pool)
            .await
            .unwrap()
            .last_insert_id();
    let lib = Browser::login(&agent.app, "am-lib").await;
    Fx {
        staging: cover_staging::staging_dir_for(&covers),
        covers,
        _dir: dir,
        agent,
        lib,
        lib_id,
    }
}

async fn file(f: &Fx, tool: &str, args: Value) -> u64 {
    f.agent.ok(tool, args).await["proposal_id"]
        .as_u64()
        .expect("proposal_id")
}

async fn file_metadata(f: &Fx, title_id: u64, fields: Value) -> u64 {
    file(
        f,
        "propose_title_metadata",
        json!({"title_id": title_id, "fields": fields, "sources": [SRC]}),
    )
    .await
}

fn png(w: u32, h: u32, shade: u8) -> Vec<u8> {
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        w,
        h,
        image::Rgb([shade, shade, shade]),
    ))
    .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
    .unwrap();
    out
}

async fn file_cover(f: &Fx, title_id: u64, shade: u8) -> u64 {
    file(
        f,
        "propose_cover",
        json!({
            "title_id": title_id,
            "image_base64": base64::engine::general_purpose::STANDARD.encode(png(40, 60, shade)),
            "media_type": "image/png",
            "sources": [SRC]
        }),
    )
    .await
}

async fn version_of(pool: &DbPool, id: u64) -> i32 {
    sqlx::query_scalar("SELECT version FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// `(status, decided_by)`.
async fn decision(pool: &DbPool, id: u64) -> (String, Option<i64>) {
    sqlx::query_as("SELECT status, CAST(decided_by AS SIGNED) FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Apostrophes come back escaped: put them back for the French copy.
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

/// `(subtitle, publisher, publication_date, dewey_code, manually_edited, version)`.
type Fields = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i32,
);

async fn fields(pool: &DbPool, title_id: u64) -> Fields {
    sqlx::query_as(
        "SELECT subtitle, publisher, CAST(DATE_FORMAT(publication_date, '%Y-%m-%d') AS CHAR), \
         dewey_code, CAST(manually_edited_fields AS CHAR), version FROM titles WHERE id = ?",
    )
    .bind(title_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn edited(json: &Option<String>) -> Vec<String> {
    serde_json::from_str(json.as_deref().unwrap_or("[]")).unwrap()
}

/// Live `(contributor name, role name)` credits of a title, sorted.
async fn credits(pool: &DbPool, title_id: u64) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT c.name, r.name FROM title_contributors tc \
         JOIN contributors c ON c.id = tc.contributor_id \
         JOIN contributor_roles r ON r.id = tc.role_id \
         WHERE tc.title_id = ? AND tc.deleted_at IS NULL ORDER BY c.name, r.name",
    )
    .bind(title_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn cover_url(pool: &DbPool, title_id: u64) -> Option<String> {
    sqlx::query_scalar("SELECT cover_image_url FROM titles WHERE id = ?")
        .bind(title_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn staged_name(pool: &DbPool, proposal_id: u64) -> String {
    let raw: String = sqlx::query_scalar(
        "SELECT CAST(proposed_values AS CHAR) FROM proposal_items WHERE proposal_id = ?",
    )
    .bind(proposal_id)
    .fetch_one(pool)
    .await
    .unwrap();
    serde_json::from_str::<Value>(&raw).unwrap()["staged_file"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn dated_title(pool: &DbPool, name: &str) -> u64 {
    let id = seed_title(pool, name).await;
    sqlx::query(
        "UPDATE titles SET publication_date = '1987-05-04', dewey_code = '843' WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    id
}

// ─── Metadata ─────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn accepting_metadata_writes_the_fields_and_protects_them(pool: DbPool) {
    let f = fx(&pool).await;
    let title = dated_title(&pool, "Le Lotus bleu").await;
    let before = fields(&pool, title).await;
    let id = file_metadata(
        &f,
        title,
        json!({"subtitle": "Les aventures  de Tintin", "publisher": "Casterman",
               "publication_year": 1946, "dewey_code": "741.5"}),
    )
    .await;

    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("Proposition n° "), "{body}");
    assert!(body.contains("mis à jour"), "{body}");
    assert!(body.contains(&format!("/title/{title}")), "{body}");
    assert!(!body.contains("proposal-accept-button"), "{body}");

    let (status_s, decided_by) = decision(&pool, id).await;
    assert_eq!(status_s, "accepted");
    assert_eq!(decided_by, Some(f.lib_id as i64));
    let after = fields(&pool, title).await;
    assert_eq!(after.0.as_deref(), Some("Les aventures de Tintin"));
    assert_eq!(after.1.as_deref(), Some("Casterman"));
    assert_eq!(after.2.as_deref(), Some("1946-01-01"));
    assert_eq!(after.3.as_deref(), Some("741.5"));
    assert_eq!(after.5, before.5 + 1, "version bumped once");
    let marks = edited(&after.4);
    for m in ["subtitle", "publisher", "publication_date", "dewey_code"] {
        assert!(marks.contains(&m.to_string()), "{m} in {marks:?}");
    }

    // #232 — a later automatic re-download leaves the accepted fields.
    let metadata = MetadataResult {
        title: Some("Le Lotus bleu".into()),
        subtitle: Some("Autre".into()),
        publisher: Some("Autre éditeur".into()),
        publication_date: Some("2001".into()),
        dewey_code: Some("000".into()),
        ..MetadataResult::default()
    };
    update_title_from_metadata(&pool, title, &metadata)
        .await
        .unwrap();
    let refetched = fields(&pool, title).await;
    assert_eq!(refetched.0, after.0);
    assert_eq!(refetched.1, after.1);
    assert_eq!(refetched.2, after.2);
    assert_eq!(refetched.3, after.3);

    // The page now shows the decision and no action.
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert!(page.contains("data-proposal-status=\"accepted\""));
    assert!(!page.contains("proposal-accept-button"));
}

#[sqlx::test(migrations = "./migrations")]
async fn contributors_are_reused_created_skipped_and_reactivated(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Les Cigares du pharaon").await;
    let jacobs = add_contributor(&pool, "Edgar P. Jacobs").await;
    let martin = add_contributor(&pool, "Jacques Martin").await;
    let removed = credit(&pool, title, martin, "Traducteur").await;
    soft_delete(&pool, "title_contributors", removed).await;
    let id = file_metadata(
        &f,
        title,
        json!({"contributors": [
            {"name": "edgar p. jacobs", "role": "Illustrateur"},
            {"name": "Bob de Moor", "role": "Auteur"},
            {"name": "Jacques Martin", "role": "Traducteur"},
            {"name": "Edgar P. Jacobs", "role": "Auteur"}
        ]}),
    )
    .await;
    // Credited by hand after filing (junction writes bump no version).
    credit(&pool, title, jacobs, "Auteur").await;
    let version_before = fields(&pool, title).await.5;

    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(decision(&pool, id).await.0, "accepted");
    assert!(body.contains("Bob de Moor"), "{body}");
    assert!(body.contains("Déjà crédités"), "{body}");

    assert_eq!(
        credits(&pool, title).await,
        vec![
            ("Bob de Moor".to_string(), "Auteur".to_string()),
            ("Edgar P. Jacobs".to_string(), "Auteur".to_string()),
            ("Edgar P. Jacobs".to_string(), "Illustrateur".to_string()),
            ("Jacques Martin".to_string(), "Traducteur".to_string()),
        ]
    );
    // Reused by the collation (case), not duplicated; one new contributor.
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contributors WHERE name = ?")
        .bind("Edgar P. Jacobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let bob: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM contributors WHERE name = 'Bob de Moor'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(bob, 1);
    // #531 — the removed credit is the same row, live again.
    let live: Option<i64> = sqlx::query_scalar(
        "SELECT CAST(id AS SIGNED) FROM title_contributors WHERE id = ? AND deleted_at IS NULL",
    )
    .bind(removed)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert_eq!(live, Some(removed as i64));
    // Contributor-only acceptance: no scalar field, no version bump.
    assert_eq!(fields(&pool, title).await.5, version_before);
}

#[sqlx::test(migrations = "./migrations")]
async fn metadata_goes_stale_when_the_title_was_edited(pool: DbPool) {
    let f = fx(&pool).await;
    let title = dated_title(&pool, "Tintin au Tibet").await;
    let id = file_metadata(&f, title, json!({"publisher": "Casterman"})).await;
    sqlx::query("UPDATE titles SET subtitle = 'x', version = version + 1 WHERE id = ?")
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("rien n'a été modifié dans le catalogue"),
        "{body}"
    );
    assert!(
        body.contains("le titre a été modifié ou supprimé"),
        "{body}"
    );
    assert_eq!(decision(&pool, id).await, ("stale".to_string(), None));
    assert_eq!(fields(&pool, title).await.1, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn metadata_goes_stale_when_a_field_changed_without_a_version_bump(pool: DbPool) {
    let f = fx(&pool).await;
    let title = dated_title(&pool, "L'Île noire").await;
    let id = file_metadata(
        &f,
        title,
        json!({"publisher": "Casterman", "subtitle": "Tintin"}),
    )
    .await;
    // A writer that bumps no version (#521).
    sqlx::query("UPDATE titles SET publisher = 'Hachette' WHERE id = ?")
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("le champ « Éditeur » du titre a été modifié"),
        "{body}"
    );
    assert_eq!(decision(&pool, id).await.0, "stale");
    let after = fields(&pool, title).await;
    assert_eq!(after.1.as_deref(), Some("Hachette"));
    assert_eq!(after.0, None, "nothing applied");
}

#[sqlx::test(migrations = "./migrations")]
async fn spacing_alone_is_no_change(pool: DbPool) {
    let f = fx(&pool).await;
    let title = dated_title(&pool, "Le Sceptre d'Ottokar").await;
    sqlx::query("UPDATE titles SET publisher = 'Casterman  Tournai' WHERE id = ?")
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    let id = file_metadata(&f, title, json!({"publisher": "Casterman (Tournai)"})).await;
    // Same text in another form: still the value recorded at filing.
    sqlx::query("UPDATE titles SET publisher = ' Casterman Tournai ' WHERE id = ?")
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(decision(&pool, id).await.0, "accepted");
    assert_eq!(
        fields(&pool, title).await.1.as_deref(),
        Some("Casterman (Tournai)")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn metadata_goes_stale_when_a_proposed_role_was_deleted(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Le Crabe aux pinces d'or").await;
    let id = file_metadata(
        &f,
        title,
        json!({"contributors": [{"name": "Nouvelle Traductrice", "role": "Traducteur"}]}),
    )
    .await;
    let role = role_id(&pool, "Traducteur").await;
    soft_delete(&pool, "contributor_roles", role).await;
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("le rôle « Traducteur » n'existe plus"),
        "{body}"
    );
    assert_eq!(decision(&pool, id).await.0, "stale");
    assert!(credits(&pool, title).await.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_malformed_stored_item_is_refused_and_stays_pending(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "On a marché sur la Lune").await;
    let id = file_metadata(&f, title, json!({"publisher": "Casterman"})).await;
    sqlx::query(
        "UPDATE proposal_items SET proposed_values = JSON_SET(proposed_values, '$.proposed', '') \
         WHERE proposal_id = ?",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(decision(&pool, id).await.0, "pending");
    assert_eq!(fields(&pool, title).await.1, None);
}

// ─── Cover ────────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn accepting_a_cover_promotes_it_and_protects_it(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Objectif Lune").await;
    let id = file_cover(&f, title, 90).await;
    let staged = staged_name(&pool, id).await;
    let staged_bytes = std::fs::read(f.staging.join(&staged)).unwrap();
    let version_before = fields(&pool, title).await.5;

    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("affiche maintenant la couverture proposée"),
        "{body}"
    );
    assert_eq!(decision(&pool, id).await.0, "accepted");
    let promoted = f.covers.join(format!("{title}.jpg"));
    assert_eq!(std::fs::read(&promoted).unwrap(), staged_bytes);
    assert!(!f.staging.join(&staged).exists(), "staged file moved");
    let url = cover_url(&pool, title).await.unwrap();
    assert!(url.starts_with(&format!("/covers/{title}.jpg?v=")), "{url}");
    let after = fields(&pool, title).await;
    assert!(edited(&after.4).contains(&"cover_image_url".to_string()));
    assert_eq!(after.5, version_before + 1);

    // A later cover re-fetch leaves it alone (#232 / #335).
    update_cover_image_url(&pool, title, Some("/covers/other.jpg"))
        .await
        .unwrap();
    assert_eq!(cover_url(&pool, title).await.as_deref(), Some(url.as_str()));

    // The detail page shows the cover now installed (D-11-16-10 note).
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert!(page.contains("id=\"proposal-accepted-cover\""), "{page}");
    assert!(page.contains(&url.replace('&', "&#38;")), "{page}");
    assert!(!page.contains("plus disponible"), "{page}");
}

#[sqlx::test(migrations = "./migrations")]
async fn accepting_a_cover_replaces_the_existing_one(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Coke en stock").await;
    std::fs::create_dir_all(&f.covers).unwrap();
    let old = f.covers.join(format!("{title}.jpg"));
    std::fs::write(&old, b"old cover").unwrap();
    sqlx::query("UPDATE titles SET cover_image_url = ? WHERE id = ?")
        .bind(format!("/covers/{title}.jpg?v=1"))
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    let id = file_cover(&f, title, 30).await;
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("qui remplace la précédente"), "{body}");
    assert_ne!(std::fs::read(&old).unwrap(), b"old cover");
    assert_ne!(
        cover_url(&pool, title).await.as_deref(),
        Some(format!("/covers/{title}.jpg?v=1").as_str())
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_cover_fetched_since_filing_makes_the_proposal_stale(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Vol 714 pour Sydney").await;
    let id = file_cover(&f, title, 60).await;
    // The background cover fetch writes the URL without a version bump.
    update_cover_image_url(&pool, title, Some("/covers/fetched.jpg"))
        .await
        .unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains("la couverture du titre a été changée"),
        "{body}"
    );
    assert_eq!(decision(&pool, id).await.0, "stale");
    assert_eq!(
        cover_url(&pool, title).await.as_deref(),
        Some("/covers/fetched.jpg")
    );
    assert!(!f.covers.join(format!("{title}.jpg")).exists());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_missing_staged_cover_is_refused_and_stays_pending(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Tintin et les Picaros").await;
    let id = file_cover(&f, title, 120).await;
    std::fs::remove_file(f.staging.join(staged_name(&pool, id).await)).unwrap();
    let (status, body) = accept(&f, &pool, id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.contains("L'image proposée n'est plus disponible"),
        "{body}"
    );
    assert_eq!(decision(&pool, id).await.0, "pending");
    assert_eq!(cover_url(&pool, title).await, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_second_cover_proposal_goes_stale_after_the_first_is_accepted(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "L'Affaire Tournesol").await;
    let first = file_cover(&f, title, 10).await;
    let second = file_cover(&f, title, 200).await;
    let metadata = file_metadata(&f, title, json!({"publisher": "Casterman"})).await;
    assert_eq!(accept(&f, &pool, first).await.0, StatusCode::OK);
    assert_eq!(decision(&pool, first).await.0, "accepted");
    let (status, body) = accept(&f, &pool, second).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(decision(&pool, second).await.0, "stale");
    // The cover bumped the title's version: the metadata one goes stale too.
    assert_eq!(accept(&f, &pool, metadata).await.0, StatusCode::OK);
    assert_eq!(decision(&pool, metadata).await.0, "stale");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_accept_modal_summarises_metadata_and_cover(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Les Bijoux de la Castafiore").await;
    let meta = file_metadata(
        &f,
        title,
        json!({"publisher": "Casterman",
               "contributors": [{"name": "Hergé", "role": "Auteur"}]}),
    )
    .await;
    let cover = file_cover(&f, title, 70).await;
    for (id, needle) in [
        (meta, "Accepter écrit ces champs"),
        (cover, "Accepter fait de l'image proposée la couverture"),
    ] {
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
        let body = text(common::web::body_string(res).await);
        assert!(body.contains(needle), "{body}");
        assert!(!body.contains("proposals."), "raw key: {body}");
    }
    // The detail page of a pending metadata proposal offers Accept.
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{meta}")).await;
    assert!(page.contains("id=\"proposal-accept-button\""));
}

// ─── #531 — the manual flow ───────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_removed_contributor_can_be_added_back_by_hand(pool: DbPool) {
    let title = seed_title(&pool, "Tintin au Congo").await;
    let role = role_id(&pool, "Auteur").await;
    ContributorService::add_to_title(&pool, title, "Hergé", role)
        .await
        .unwrap();
    let junction: u64 = sqlx::query_scalar(
        "SELECT id FROM title_contributors WHERE title_id = ? AND deleted_at IS NULL",
    )
    .bind(title)
    .fetch_one(&pool)
    .await
    .unwrap();
    ContributorService::remove_from_title(&pool, junction)
        .await
        .unwrap();
    assert!(credits(&pool, title).await.is_empty());

    ContributorService::add_to_title(&pool, title, "Hergé", role)
        .await
        .expect("a removed contributor can be added back (#531)");
    assert_eq!(
        credits(&pool, title).await,
        vec![("Hergé".to_string(), "Auteur".to_string())]
    );
    // A live credit still answers the duplicate message.
    match ContributorService::add_to_title(&pool, title, "Hergé", role).await {
        Err(mybibli::error::AppError::BadRequest(m)) => {
            assert!(m.contains("Hergé") && !m.contains("DUPLICATE"), "{m}")
        }
        other => panic!("expected the duplicate message, got {other:?}"),
    }
}

// ─── Review round 1 ───────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_cover_that_cannot_be_promoted_is_accepted_but_not_installed(pool: DbPool) {
    use mybibli::services::acceptance::{self, AcceptOutcome, Applied, CoverDirs};
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Tintin en Amérique").await;
    let id = file_cover(&f, title, 150).await;
    let staged = staged_name(&pool, id).await;
    let v = version_of(&pool, id).await;
    // The covers directory is gone: the promotion fails after the commit.
    let missing = f.covers.join("nowhere");
    let dirs = CoverDirs {
        covers_dir: &missing,
        staging_dir: Some(&f.staging),
    };
    let (_, outcome) = acceptance::accept(&pool, dirs, id, f.lib_id, v)
        .await
        .unwrap();
    match outcome {
        AcceptOutcome::Accepted(Applied::Cover(a)) => assert!(!a.installed),
        other => panic!("expected an accepted cover, got {other:?}"),
    }
    assert_eq!(decision(&pool, id).await.0, "accepted");
    assert_eq!(cover_url(&pool, title).await, None, "no URL without a file");
    assert!(f.staging.join(&staged).exists(), "staged file kept");
}

#[sqlx::test(migrations = "./migrations")]
async fn an_accepted_cover_stops_the_background_cover_download(pool: DbPool) {
    use mybibli::tasks::metadata_fetch::cover_is_manual;
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Le Temple du Soleil").await;
    assert!(!cover_is_manual(&pool, title).await);
    let id = file_cover(&f, title, 40).await;
    assert_eq!(accept(&f, &pool, id).await.0, StatusCode::OK);
    assert!(cover_is_manual(&pool, title).await);
}
