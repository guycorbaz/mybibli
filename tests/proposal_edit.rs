//! Story 11-19 (#493) — editing a pending proposal before deciding it
//! (FR139): line edits with the filing rules, line removal, the trail
//! (agent's values kept, who and when), optimistic locking on the
//! proposal version, nothing left to apply, acceptance of the edited
//! values — through the real router. Proposals are filed by an agent
//! through the real MCP tools (item shapes of 11-12 / 11-13).
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test proposal_edit

mod common;

use axum::http::StatusCode;
use common::mcp::*;
use common::web::{Browser, PW_HASH, body_string, seed_admin};
use mybibli::db::DbPool;
use serde_json::{Value, json};

const SRC: &str = "https://example.org/edit";

struct Fx {
    agent: Agent,
    lib: Browser,
}

async fn fx(pool: &DbPool) -> Fx {
    let agent = Agent::new(pool).await;
    seed_admin(pool, "pe-admin").await;
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
        .bind("pe-lib")
        .bind(PW_HASH)
        .execute(pool)
        .await
        .unwrap();
    let lib = Browser::login(&agent.app, "pe-lib").await;
    Fx { agent, lib }
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

async fn status_of(pool: &DbPool, id: u64) -> String {
    sqlx::query_scalar("SELECT status FROM proposals WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Every item id of a proposal (removed ones included), filing order.
async fn item_ids(pool: &DbPool, id: u64) -> Vec<u64> {
    sqlx::query_scalar("SELECT id FROM proposal_items WHERE proposal_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap()
}

struct Item {
    proposed: Value,
    original: Option<Value>,
    edited_by: Option<String>,
    edited: bool,
    removed: bool,
    version: i32,
}

async fn item(pool: &DbPool, item_id: u64) -> Item {
    let (proposed, original, edited_by, edited, removed, version): (
        String,
        Option<String>,
        Option<String>,
        bool,
        bool,
        i32,
    ) = sqlx::query_as(
        "SELECT CAST(proposed_values AS CHAR), CAST(original_values AS CHAR), \
         CAST(edited_by_name AS CHAR), edited_at IS NOT NULL, deleted_at IS NOT NULL, version \
         FROM proposal_items WHERE id = ?",
    )
    .bind(item_id)
    .fetch_one(pool)
    .await
    .unwrap();
    Item {
        proposed: serde_json::from_str(&proposed).unwrap(),
        original: original.map(|o| serde_json::from_str(&o).unwrap()),
        edited_by,
        edited,
        removed,
        version,
    }
}

/// Apostrophes come back escaped (`&#x27;` / `&#39;`): put them back so
/// the French copy can be asserted as written.
fn text(body: String) -> String {
    body.replace("&#x27;", "'").replace("&#39;", "'")
}

async fn post(f: &Fx, path: &str, body: &str) -> (StatusCode, String) {
    let (s, b) = f.lib.post_form(&f.agent.app, path, body).await;
    (s, text(b))
}

async fn edit(f: &Fx, id: u64, item_id: u64, version: i32, fields: &str) -> (StatusCode, String) {
    post(
        f,
        &format!("/proposals/{id}/items/{item_id}"),
        &format!("version={version}&{fields}"),
    )
    .await
}

async fn get(f: &Fx, path: &str) -> (StatusCode, String) {
    let res = f.lib.send(&f.agent.app, "GET", path, "", true).await;
    let status = res.status();
    (status, text(body_string(res).await))
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

/// A closed creation: two titles (positions 1 and 2) and a tome at 4.
async fn creation(f: &Fx, pool: &DbPool) -> (u64, u64, u64) {
    let t1 = seed_title(pool, "Premier").await;
    let t2 = seed_title(pool, "Deuxième").await;
    let id = file(
        f,
        "propose_series_create",
        json!({
            "name": "Série à corriger",
            "series_type": "closed",
            "total_volume_count": 5,
            "titles": [
                {"title_id": t1, "position": 1},
                {"title_id": t2, "position": 2}
            ],
            "expected_tomes": [{"position": 4, "title": "Tome quatre"}],
            "sources": [SRC]
        }),
    )
    .await;
    (id, t1, t2)
}

#[sqlx::test(migrations = "./migrations")]
async fn an_edit_keeps_the_agent_values_and_bumps_the_version(pool: DbPool) {
    let f = fx(&pool).await;
    let (id, t1, t2) = creation(&f, &pool).await;
    let items = item_ids(&pool, id).await;
    let key_before: Option<String> =
        sqlx::query_scalar("SELECT CAST(equivalence_key AS CHAR) FROM proposals WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();

    // The page offers an edit form per line and no removal of the series line.
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    for item_id in &items {
        assert!(
            page.contains(&format!("hx-post=\"/proposals/{id}/items/{item_id}\"")),
            "{page}"
        );
    }
    assert!(!page.contains(&format!("/items/{}/remove-modal", items[0])));
    assert!(page.contains(&format!("/items/{}/remove-modal", items[1])));
    assert!(page.contains("data-action=\"inline-form-add-toggle\""));
    assert!(page.contains("data-action=\"inline-form-slot-cancel\""));

    // Title 1 moves from 1 to 3, as an omnibus 3–3 is refused (span ok).
    let v = version_of(&pool, id).await;
    let (status, body) = edit(&f, id, items[1], v, "position=3").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.contains(&format!("Ligne de la proposition n° {id} modifiée.")),
        "{body}"
    );
    assert!(body.contains("id=\"proposal-detail-body\""));
    // The re-rendered body carries the new version.
    assert!(body.contains(&format!("version={}", v + 1)), "{body}");
    let line = item(&pool, items[1]).await;
    assert_eq!(
        line.proposed,
        json!({"line": "title", "position": 3, "is_omnibus": false})
    );
    assert_eq!(
        line.original,
        Some(json!({"line": "title", "position": 1, "is_omnibus": false}))
    );
    assert_eq!(line.edited_by.as_deref(), Some("pe-lib"));
    assert!(line.edited && !line.removed);
    assert_eq!(line.version, 2);
    assert_eq!(version_of(&pool, id).await, v + 1);

    // A second edit keeps the first original.
    let (status, _) = edit(&f, id, items[1], v + 1, "position=5").await;
    assert_eq!(status, StatusCode::OK);
    let line = item(&pool, items[1]).await;
    assert_eq!(line.proposed["position"], json!(5));
    assert_eq!(line.original.unwrap()["position"], json!(1));

    // The page shows the trail under the line.
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    let page = text(page);
    assert!(page.contains("Modifiée par pe-lib le "), "{page}");
    assert!(
        page.contains("L'assistant proposait : Position 1 : Premier"),
        "{page}"
    );

    // Another librarian's save with the page's old version is refused.
    let (status, body) = edit(&f, id, items[2], v, "position=4").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.contains("Cet enregistrement a été modifié par un autre utilisateur"),
        "{body}"
    );
    assert_eq!(item(&pool, items[2]).await.proposed["position"], json!(2));
    // A missing or garbled version is refused the same way.
    for bad in ["", "version=x&"] {
        let (status, _) = post(
            &f,
            &format!("/proposals/{id}/items/{}", items[2]),
            &format!("{bad}position=4"),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "{bad}");
    }

    // The series line: name, type and total.
    let v = version_of(&pool, id).await;
    let (status, body) = edit(
        &f,
        id,
        items[0],
        v,
        "name=S%C3%A9rie+corrig%C3%A9e&series_type=closed&total=6",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        item(&pool, items[0]).await.proposed,
        json!({"line": "series", "name": "Série corrigée", "series_type": "closed", "total_volume_count": 6})
    );
    // The tome: position, title, ISBN (normalised).
    let v = version_of(&pool, id).await;
    let (status, body) = edit(
        &f,
        id,
        items[3],
        v,
        "position=6&title=Tome+six&isbn=978-2-07-036024-6",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        item(&pool, items[3]).await.proposed,
        json!({"line": "expected_tome", "position": 6, "title": "Tome six", "isbn": "9782070360246"})
    );

    // The equivalence key is the one filed (D-11-15-2).
    let key_after: Option<String> =
        sqlx::query_scalar("SELECT CAST(equivalence_key AS CHAR) FROM proposals WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(key_before, key_after);

    // Accepting applies the edited values.
    let v = version_of(&pool, id).await;
    let (status, body) = post(
        &f,
        &format!("/proposals/{id}/accept"),
        &format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted");
    let (series_id, total): (u64, Option<i32>) = sqlx::query_as(
        "SELECT id, total_volume_count FROM series WHERE name = 'Série corrigée' AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(total, Some(6));
    let placed: Vec<(u64, i32)> = sqlx::query_as(
        "SELECT title_id, position_number FROM title_series WHERE series_id = ? \
         AND deleted_at IS NULL ORDER BY position_number",
    )
    .bind(series_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(placed, vec![(t2, 2), (t1, 5)]);
    let tomes: Vec<(i32, String)> = sqlx::query_as(
        "SELECT position, title FROM series_expected_tomes WHERE series_id = ? AND deleted_at IS NULL",
    )
    .bind(series_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(tomes, vec![(6, "Tome six".to_string())]);

    // A decided proposal keeps its trail and offers no edit.
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    let page = text(page);
    assert!(page.contains("Modifiée par pe-lib"), "{page}");
    assert!(!page.contains("data-action=\"inline-form-add-toggle\""));
    let (status, body) = edit(&f, id, items[1], v + 1, "position=4").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("a déjà été tranchée"), "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn edits_follow_the_filing_rules(pool: DbPool) {
    let f = fx(&pool).await;
    let (id, _, _) = creation(&f, &pool).await;
    let items = item_ids(&pool, id).await;
    let v = version_of(&pool, id).await;

    let refused = [
        // The tome's position, by a title line.
        (
            items[1],
            "position=4",
            "La position 4 est déjà utilisée par une autre ligne",
        ),
        // Out of range, garbled, above the total.
        (
            items[1],
            "position=0",
            "La position est comprise entre 1 et 10000",
        ),
        (
            items[1],
            "position=abc",
            "La position est comprise entre 1 et 10000",
        ),
        (
            items[2],
            "position=7",
            "La position 7 dépasse les 5 volumes de la série",
        ),
        // An omnibus without its last position.
        (
            items[1],
            "position=3&is_omnibus=on",
            "Une intégrale se termine",
        ),
        // Series line.
        (
            items[0],
            "name=+&series_type=open",
            "Le nom de la série est obligatoire",
        ),
        (
            items[0],
            "name=S&series_type=closed",
            "Une série fermée a besoin de son nombre de volumes",
        ),
        (
            items[0],
            "name=S&series_type=closed&total=3",
            "La position 4 dépasse les 3 volumes de la série",
        ),
        // Tome rules.
        (items[3], "position=4&title=+", "Le titre est obligatoire"),
        (
            items[3],
            "position=4&title=T&isbn=123",
            "ISBN-13 ou ISBN-10 valide",
        ),
    ];
    for (item_id, fields, message) in refused {
        let (status, body) = edit(&f, id, item_id, v, fields).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{fields}: {body}");
        assert!(body.contains(message), "{fields}: {body}");
    }
    // Nothing was saved.
    assert_eq!(version_of(&pool, id).await, v);
    for item_id in &items {
        assert!(!item(&pool, *item_id).await.edited);
    }

    // A creation may not take the name of an active series.
    closed_series(&pool, "Déjà là", 3).await;
    let (status, body) = edit(
        &f,
        id,
        items[0],
        v,
        "name=d%C3%A9j%C3%A0+l%C3%A0&series_type=open",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.contains("Une série nommée « Déjà là » existe déjà"),
        "{body}"
    );

    // Submitting the stored values records nothing.
    let (status, body) = edit(&f, id, items[1], v, "position=1").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("Rien n'a changé"), "{body}");
    assert_eq!(version_of(&pool, id).await, v);
    assert!(!item(&pool, items[1]).await.edited);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_attach_edit_is_checked_against_the_series(pool: DbPool) {
    let f = fx(&pool).await;
    let series = closed_series(&pool, "Série existante", 8).await;
    let holder = seed_title(&pool, "Occupant").await;
    assign(&pool, holder, series, 1).await;
    let t = seed_title(&pool, "Nouveau").await;
    let id = file(
        &f,
        "propose_series_attach",
        json!({
            "series_id": series,
            "titles": [{"title_id": t, "position": 2}],
            "expected_tomes": [{"position": 3, "title": "Tome trois"}],
            "sources": [SRC]
        }),
    )
    .await;
    let items = item_ids(&pool, id).await;
    let v = version_of(&pool, id).await;

    // The series line of an attach has nothing to edit.
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert!(!page.contains(&format!("hx-post=\"/proposals/{id}/items/{}\"", items[0])));
    let (status, _) = edit(&f, id, items[0], v, "name=X").await;
    assert_eq!(status, StatusCode::CONFLICT);

    // A position held in the series, a position above its total.
    let (status, body) = edit(&f, id, items[1], v, "position=1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("déjà occupée par « Occupant »"), "{body}");
    let (status, body) = edit(&f, id, items[2], v, "position=9&title=Tome+neuf").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("dépasse les 8 volumes"), "{body}");
    // An allowed move.
    let (status, body) = edit(&f, id, items[2], v, "position=5&title=Tome+cinq").await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[sqlx::test(migrations = "./migrations")]
async fn removed_lines_stay_on_record_and_an_empty_attach_can_only_be_rejected(pool: DbPool) {
    let f = fx(&pool).await;
    let series = closed_series(&pool, "Série à compléter", 8).await;
    let t = seed_title(&pool, "À placer").await;
    let id = file(
        &f,
        "propose_series_attach",
        json!({
            "series_id": series,
            "titles": [{"title_id": t, "position": 2}],
            "expected_tomes": [{"position": 3, "title": "Tome trois"}],
            "sources": [SRC]
        }),
    )
    .await;
    let items = item_ids(&pool, id).await;

    // The series line cannot be removed.
    let v = version_of(&pool, id).await;
    let (status, body) = get(
        &f,
        &format!(
            "/proposals/{id}/items/{}/remove-modal?version={v}",
            items[0]
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.contains("La ligne de la série ne peut pas être retirée"),
        "{body}"
    );
    let (status, _) = post(
        &f,
        &format!("/proposals/{id}/items/{}/remove", items[0]),
        &format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // The modal names the line; the removal is recorded.
    let (status, modal) = get(
        &f,
        &format!(
            "/proposals/{id}/items/{}/remove-modal?version={v}",
            items[2]
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{modal}");
    assert!(modal.contains(&format!(
        "hx-post=\"/proposals/{id}/items/{}/remove\"",
        items[2]
    )));
    assert!(modal.contains("Tome trois"), "{modal}");
    let res = f
        .lib
        .send(
            &f.agent.app,
            "POST",
            &format!("/proposals/{id}/items/{}/remove", items[2]),
            &format!("version={v}"),
            true,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()
            .get("hx-trigger-after-swap")
            .and_then(|h| h.to_str().ok()),
        Some("modal-close")
    );
    let body = text(body_string(res).await);
    assert!(
        body.contains(&format!("Ligne retirée de la proposition n° {id}.")),
        "{body}"
    );
    assert!(body.contains("Lignes retirées avant la décision"), "{body}");
    assert!(body.contains("retirée par pe-lib le "), "{body}");
    let removed = item(&pool, items[2]).await;
    assert!(removed.removed && removed.edited);
    assert_eq!(removed.original, Some(removed.proposed.clone()));
    assert_eq!(version_of(&pool, id).await, v + 1);
    // The list counts live lines only.
    let (_, list) = f.lib.get(&f.agent.app, "/proposals").await;
    assert!(list.contains(&format!("/proposals/{id}")), "{list}");

    // Removing the title leaves only the series line: reject only.
    let (status, _) = post(
        &f,
        &format!("/proposals/{id}/items/{}/remove", items[1]),
        &format!("version={}", v + 1),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v = version_of(&pool, id).await;
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert!(!page.contains("id=\"proposal-accept-button\""), "{page}");
    assert!(page.contains("id=\"proposal-reject-button\""));
    let (status, body) = get(&f, &format!("/proposals/{id}/accept-modal?version={v}")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("plus aucune ligne à appliquer"), "{body}");
    let (status, body) = post(
        &f,
        &format!("/proposals/{id}/accept"),
        &format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("plus aucune ligne à appliquer"), "{body}");
    assert_eq!(status_of(&pool, id).await, "pending");
    let (status, _) = post(
        &f,
        &format!("/proposals/{id}/reject"),
        &format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_of(&pool, id).await, "rejected");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_removed_line_is_not_applied_nor_checked(pool: DbPool) {
    let f = fx(&pool).await;
    let (id, t1, t2) = creation(&f, &pool).await;
    let items = item_ids(&pool, id).await;
    // Title 1 changes after filing: its line would make the proposal stale.
    sqlx::query("UPDATE titles SET version = version + 1 WHERE id = ?")
        .bind(t1)
        .execute(&pool)
        .await
        .unwrap();
    let v = version_of(&pool, id).await;
    let (status, _) = post(
        &f,
        &format!("/proposals/{id}/items/{}/remove", items[1]),
        &format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = post(
        &f,
        &format!("/proposals/{id}/accept"),
        &format!("version={}", v + 1),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted");
    let placed: Vec<u64> = sqlx::query_scalar(
        "SELECT ts.title_id FROM title_series ts JOIN series s ON s.id = ts.series_id \
         WHERE s.name = 'Série à corriger' AND ts.deleted_at IS NULL",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(placed, vec![t2]);
}

#[sqlx::test(migrations = "./migrations")]
async fn metadata_edits_are_validated_and_accepted(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Titre à compléter").await;
    let id = file(
        &f,
        "propose_title_metadata",
        json!({
            "title_id": title,
            "fields": {
                "subtitle": "Sous-titre de l'agent",
                "publisher": "Éditeur",
                "contributors": [{"name": "Auteur Erroné", "role": "Auteur"}]
            },
            "sources": [SRC]
        }),
    )
    .await;
    let items = item_ids(&pool, id).await;
    let (subtitle, publisher, contributors) = (items[0], items[1], items[2]);
    let auteur = role_id(&pool, "Auteur").await;
    let v = version_of(&pool, id).await;

    // The contributor form offers the live roles, the proposed one selected.
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert!(
        page.contains(&format!("<option value=\"{auteur}\" selected>")),
        "{page}"
    );

    // Filing rules: blank, too long, control characters.
    let long = "a".repeat(501);
    for (fields, message) in [
        ("value=+", "Sous-titre : une valeur est obligatoire"),
        (
            &*format!("value={long}"),
            "Sous-titre : au plus 500 caractères",
        ),
        ("value=a%0Ab", "pas de retour à la ligne"),
    ] {
        let (status, body) = edit(&f, id, subtitle, v, fields).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{fields}: {body}");
        assert!(body.contains(message), "{fields}: {body}");
    }
    let (status, body) = edit(
        &f,
        id,
        contributors,
        v,
        "contributor_name_0=X&contributor_role_0=99999",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("choisissez un rôle dans la liste"), "{body}");

    // The subtitle and the contributor's name are corrected.
    let (status, body) = edit(&f, id, subtitle, v, "value=Sous-titre+corrig%C3%A9").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let line = item(&pool, subtitle).await;
    assert_eq!(line.proposed["proposed"], json!("Sous-titre corrigé"));
    // `current` is kept as filed (the staleness baseline).
    assert_eq!(line.proposed["current"], Value::Null);
    assert_eq!(
        line.original.unwrap()["proposed"],
        json!("Sous-titre de l'agent")
    );
    let (status, body) = edit(
        &f,
        id,
        contributors,
        v + 1,
        &format!("contributor_name_0=Auteur+Correct&contributor_role_0={auteur}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        item(&pool, contributors).await.proposed["proposed"],
        json!([{"name": "Auteur Correct", "role": "Auteur", "role_id": auteur}])
    );

    // The publisher line is removed; the last line could not be.
    let (status, _) = post(
        &f,
        &format!("/proposals/{id}/items/{publisher}/remove"),
        &format!("version={}", v + 2),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let v = version_of(&pool, id).await;
    let (status, body) = post(
        &f,
        &format!("/proposals/{id}/accept"),
        &format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(status_of(&pool, id).await, "accepted");
    let (sub, pubr): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT subtitle, publisher FROM titles WHERE id = ?")
            .bind(title)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(sub.as_deref(), Some("Sous-titre corrigé"));
    assert_eq!(pubr, None);
    let credited: Vec<String> = sqlx::query_scalar(
        "SELECT c.name FROM title_contributors tc JOIN contributors c ON c.id = tc.contributor_id \
         WHERE tc.title_id = ? AND tc.deleted_at IS NULL",
    )
    .bind(title)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(credited, vec!["Auteur Correct".to_string()]);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_last_metadata_line_and_a_cover_line_stay(pool: DbPool) {
    let f = fx(&pool).await;
    let title = seed_title(&pool, "Une seule ligne").await;
    let id = file(
        &f,
        "propose_title_metadata",
        json!({"title_id": title, "fields": {"subtitle": "Seul"}, "sources": [SRC]}),
    )
    .await;
    let items = item_ids(&pool, id).await;
    let v = version_of(&pool, id).await;
    let (_, page) = f.lib.get(&f.agent.app, &format!("/proposals/{id}")).await;
    assert!(!page.contains("/remove-modal"), "{page}");
    let (status, body) = post(
        &f,
        &format!("/proposals/{id}/items/{}/remove", items[0]),
        &format!("version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.contains("C'est la dernière ligne de la proposition"),
        "{body}"
    );

    // A cover proposal (inserted as filed by 11-14): no edit, no removal.
    let key_id: u64 = sqlx::query_scalar("SELECT id FROM api_keys ORDER BY id DESC LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let cover = sqlx::query(
        "INSERT INTO proposals (kind, api_key_id, sources) VALUES ('cover', ?, '[\"https://example.org/c\"]')",
    )
    .bind(key_id)
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id();
    let cover_item = sqlx::query(
        "INSERT INTO proposal_items (proposal_id, target_table, target_id, target_version, proposed_values) \
         VALUES (?, 'titles', ?, 1, ?)",
    )
    .bind(cover)
    .bind(title)
    .bind(
        json!({"line": "cover", "staged_file": "x.jpg", "current": null, "replaces_existing": false})
            .to_string(),
    )
    .execute(&pool)
    .await
    .unwrap()
    .last_insert_id();
    let (_, page) = f
        .lib
        .get(&f.agent.app, &format!("/proposals/{cover}"))
        .await;
    assert!(!page.contains("inline-form-add-toggle"), "{page}");
    assert!(!page.contains("/remove-modal"), "{page}");
    let (status, body) = edit(&f, cover, cover_item, 1, "value=x").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.contains("Cette ligne ne peut pas être modifiée"),
        "{body}"
    );
    let (status, body) = post(
        &f,
        &format!("/proposals/{cover}/items/{cover_item}/remove"),
        "version=1",
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.contains("Une proposition de couverture n'a qu'une ligne"),
        "{body}"
    );

    // An item of another proposal is unknown here.
    let (status, _) = edit(&f, id, cover_item, v, "value=x").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "./migrations")]
async fn csrf_and_role_are_required(pool: DbPool) {
    let f = fx(&pool).await;
    let (id, _, _) = creation(&f, &pool).await;
    let items = item_ids(&pool, id).await;
    let v = version_of(&pool, id).await;
    let anon = Browser::anonymous(&f.agent.app).await;
    for path in [
        format!("/proposals/{id}/items/{}", items[1]),
        format!("/proposals/{id}/items/{}/remove", items[1]),
    ] {
        let res = anon
            .send(
                &f.agent.app,
                "POST",
                &path,
                &format!("version={v}&position=3"),
                true,
            )
            .await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER, "{path}");
    }
    // A forged CSRF token.
    let forged = Browser {
        cookie: f.lib.cookie.clone(),
        csrf: "forged".into(),
    };
    let (status, _) = forged
        .post_form(
            &f.agent.app,
            &format!("/proposals/{id}/items/{}", items[1]),
            &format!("version={v}&position=3"),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(version_of(&pool, id).await, v);
    assert!(!item(&pool, items[1]).await.edited);
}

#[sqlx::test(migrations = "./migrations")]
async fn stale_proposals_gone_series_and_unchanged_values_are_refused(pool: DbPool) {
    let f = fx(&pool).await;
    // A metadata value edited back to the title's value at filing.
    let title = seed_title(&pool, "Daté").await;
    sqlx::query("UPDATE titles SET publication_date = '1987-06-15' WHERE id = ?")
        .bind(title)
        .execute(&pool)
        .await
        .unwrap();
    let id = file(
        &f,
        "propose_title_metadata",
        json!({"title_id": title, "fields": {"publication_year": 1988, "subtitle": "S"}, "sources": [SRC]}),
    )
    .await;
    let items = item_ids(&pool, id).await;
    let v = version_of(&pool, id).await;
    let year_item = {
        let mut found = None;
        for i in &items {
            if item(&pool, *i).await.proposed["field"] == json!("publication_year") {
                found = Some(*i);
            }
        }
        found.unwrap()
    };
    let (status, body) = edit(&f, id, year_item, v, "value=1987").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("c'est déjà la valeur du titre"), "{body}");
    assert!(!item(&pool, year_item).await.edited);

    // A stale proposal refuses edits and removals.
    sqlx::query("UPDATE proposals SET status = 'stale', decided_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = edit(&f, id, year_item, v, "value=1990").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("périmée"), "{body}");
    let (status, body) = get(
        &f,
        &format!("/proposals/{id}/items/{year_item}/remove-modal?version={v}"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // An attach whose series was deleted since filing.
    let series = closed_series(&pool, "Bientôt supprimée", 8).await;
    let t = seed_title(&pool, "Orphelin").await;
    let id = file(
        &f,
        "propose_series_attach",
        json!({"series_id": series, "titles": [{"title_id": t, "position": 2}], "sources": [SRC]}),
    )
    .await;
    soft_delete(&pool, "series", series).await;
    let items = item_ids(&pool, id).await;
    let v = version_of(&pool, id).await;
    let (status, body) = edit(&f, id, items[1], v, "position=3").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body.contains("La série de cette proposition n'existe plus"),
        "{body}"
    );
    assert_eq!(version_of(&pool, id).await, v);
}
