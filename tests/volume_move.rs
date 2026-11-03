//! Story 13-8 (#580) — move a volume to another title.
//!
//! Service: the move itself (only `title_id` and `version` change), its
//! refusals (no target, same title, deleted/unknown target, deleted volume,
//! stale version — nothing written), and an active loan that follows the
//! volume. Web: the modal (HTMX only), the picker's candidates (live titles,
//! current title excluded, title or ISBN, at most 10), the selection, the
//! POST (success shape: FeedbackEntry + OOB title row + `modal-close` after
//! the swap; a refusal keeps its body in the modal) and the role gate.
//! The web tests run in the default instance language (French).
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test volume_move

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::mcp::{add_contributor, add_title, add_volume, credit, router, soft_delete};
use common::web::{Browser, body_string, seed_admin};
use mybibli::db::DbPool;
use mybibli::error::AppError;
use mybibli::models::volume::VolumeModel;
use mybibli::models::volume_move::search_candidates;
use mybibli::services::volume::VolumeService;
use tower::ServiceExt;

/// A volume with a location, a condition comment and the audit flag, so
/// the move can be shown to leave them alone.
async fn rich_volume(pool: &DbPool, title_id: u64, label: &str) -> u64 {
    let (node_type,): (String,) =
        sqlx::query_as("SELECT name FROM location_node_types WHERE deleted_at IS NULL LIMIT 1")
            .fetch_one(pool)
            .await
            .unwrap();
    let location = sqlx::query(
        "INSERT INTO storage_locations (label, name, node_type) VALUES ('L7801', 'Shelf MV', ?)",
    )
    .bind(node_type)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_id();
    let id = add_volume(pool, title_id, label, Some(location)).await;
    sqlx::query(
        "UPDATE volumes SET edition_comment = 'first printing', under_audit_since = NOW(), \
         purchase_price = 12.50, purchase_currency = 'EUR' WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn volume(pool: &DbPool, id: u64) -> VolumeModel {
    VolumeModel::find_by_id(pool, id)
        .await
        .unwrap()
        .expect("live volume")
}

async fn title_id_of(pool: &DbPool, id: u64) -> u64 {
    let (t,): (u64,) = sqlx::query_as("SELECT title_id FROM volumes WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap();
    t
}

fn message(err: AppError) -> String {
    match err {
        AppError::BadRequest(m) | AppError::NotFound(m) | AppError::Conflict(m) => m,
        other => panic!("unexpected error {other:?}"),
    }
}

// ─── Service ─────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn a_move_changes_the_title_and_nothing_else(pool: DbPool) {
    let from = add_title(&pool, "Album partagé", "bd", "BD", Some("5410983213000")).await;
    let to = add_title(&pool, "Le vrai album", "bd", "BD", Some("9782800110264")).await;
    let id = rich_volume(&pool, from, "V7801").await;
    let before = volume(&pool, id).await;

    let moved = VolumeService::move_to_title(&pool, id, before.version, Some(to), Some(1), "en")
        .await
        .unwrap();
    assert_eq!(moved.from_title_id, from);
    assert_eq!(moved.to_title_id, to);
    assert_eq!(moved.label, "V7801");

    let after = volume(&pool, id).await;
    assert_eq!(after.title_id, to);
    assert_eq!(after.version, before.version + 1);
    assert_eq!(after.label, before.label);
    assert_eq!(after.location_id, before.location_id);
    assert_eq!(after.condition_state_id, before.condition_state_id);
    assert_eq!(after.edition_comment, before.edition_comment);
    assert_eq!(after.purchase_price, before.purchase_price);
    assert_eq!(after.purchase_currency, before.purchase_currency);
    assert_eq!(after.under_audit_since, before.under_audit_since);

    // Per-title counts are computed live.
    assert_eq!(VolumeModel::count_by_title(&pool, from).await.unwrap(), 0);
    assert_eq!(VolumeModel::count_by_title(&pool, to).await.unwrap(), 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_same_title_is_refused(pool: DbPool) {
    let from = add_title(&pool, "Seul", "book", "Roman", None).await;
    let id = add_volume(&pool, from, "V7802", None).await;
    let v = volume(&pool, id).await;
    let err = VolumeService::move_to_title(&pool, id, v.version, Some(from), None, "en")
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::BadRequest(_)), "{err:?}");
    assert!(message(err).contains("already under this title"));
    assert_eq!(volume(&pool, id).await.version, v.version);
}

#[sqlx::test(migrations = "./migrations")]
async fn no_target_is_refused(pool: DbPool) {
    let from = add_title(&pool, "Seul", "book", "Roman", None).await;
    let id = add_volume(&pool, from, "V7803", None).await;
    let v = volume(&pool, id).await;
    let err = VolumeService::move_to_title(&pool, id, v.version, None, None, "fr")
        .await
        .unwrap_err();
    assert!(message(err).contains("aucun titre n’a été choisi"));
    assert_eq!(title_id_of(&pool, id).await, from);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_or_unknown_target_is_refused(pool: DbPool) {
    let from = add_title(&pool, "Départ", "book", "Roman", None).await;
    let gone = add_title(&pool, "Supprimé", "book", "Roman", None).await;
    soft_delete(&pool, "titles", gone).await;
    let id = add_volume(&pool, from, "V7804", None).await;
    let v = volume(&pool, id).await;

    for target in [gone, 999_999] {
        let err = VolumeService::move_to_title(&pool, id, v.version, Some(target), None, "en")
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
        assert!(message(err).contains("no longer exists"));
    }
    assert_eq!(title_id_of(&pool, id).await, from);
    assert_eq!(volume(&pool, id).await.version, v.version);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_stale_version_is_a_conflict(pool: DbPool) {
    let from = add_title(&pool, "Départ", "book", "Roman", None).await;
    let to = add_title(&pool, "Arrivée", "book", "Roman", None).await;
    let id = add_volume(&pool, from, "V7805", None).await;
    let v = volume(&pool, id).await;
    // Someone else saves the volume first.
    sqlx::query("UPDATE volumes SET version = version + 1 WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();

    let err = VolumeService::move_to_title(&pool, id, v.version, Some(to), None, "en")
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Conflict(_)), "{err:?}");
    assert!(message(err).contains("changed it in the meantime"));
    assert_eq!(title_id_of(&pool, id).await, from);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_volume_is_refused(pool: DbPool) {
    let from = add_title(&pool, "Départ", "book", "Roman", None).await;
    let to = add_title(&pool, "Arrivée", "book", "Roman", None).await;
    let id = add_volume(&pool, from, "V7806", None).await;
    soft_delete(&pool, "volumes", id).await;

    let err = VolumeService::move_to_title(&pool, id, 1, Some(to), None, "en")
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
    assert!(message(err).contains("it no longer exists"));
    assert_eq!(title_id_of(&pool, id).await, from);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_active_loan_follows_the_volume(pool: DbPool) {
    seed_admin(&pool, "mv_admin").await;
    let app = router(&pool);
    let browser = Browser::login(&app, "mv_admin").await;

    let from = add_title(&pool, "Album 504", "bd", "BD", Some("5410983213000")).await;
    let to = add_title(&pool, "Album 523", "bd", "BD", Some("9782800110264")).await;
    let id = add_volume(&pool, from, "V7807", None).await;
    let borrower = sqlx::query("INSERT INTO borrowers (name) VALUES ('Emprunteur MV')")
        .execute(&pool)
        .await
        .unwrap()
        .last_insert_id();
    let loan =
        sqlx::query("INSERT INTO loans (volume_id, borrower_id, loaned_at) VALUES (?, ?, NOW())")
            .bind(id)
            .bind(borrower)
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_id();
    let loan_row = |pool: DbPool| async move {
        sqlx::query_as::<_, (u64, u64, Option<chrono::NaiveDateTime>, i32)>(
            "SELECT volume_id, borrower_id, CAST(returned_at AS DATETIME), version \
             FROM loans WHERE id = ?",
        )
        .bind(loan)
        .fetch_one(&pool)
        .await
        .unwrap()
    };
    let loan_before = loan_row(pool.clone()).await;

    let v = volume(&pool, id).await;
    let (status, body) = browser
        .post_form(
            &app,
            &format!("/volume/{id}/move"),
            &format!("title_id={to}&version={}", v.version),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(
        loan_row(pool.clone()).await,
        loan_before,
        "the loan row is untouched"
    );
    assert_eq!(title_id_of(&pool, id).await, to);

    // The target's title page lists the volume, the source's no longer does.
    let (_, target_page) = browser.get(&app, &format!("/title/{to}")).await;
    assert!(
        target_page.contains("V7807"),
        "target title page lists V7807"
    );
    let (_, source_page) = browser.get(&app, &format!("/title/{from}")).await;
    assert!(
        !source_page.contains("V7807"),
        "source title page no longer lists V7807"
    );
}

// ─── Picker ──────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn candidates_are_live_other_titles_by_text_or_isbn(pool: DbPool) {
    let current = add_title(&pool, "Spirou courant", "bd", "BD", Some("5410983213000")).await;
    let other = add_title(
        &pool,
        "Spirou et Fantasio",
        "bd",
        "BD",
        Some("9782800110264"),
    )
    .await;
    let author = add_contributor(&pool, "Franquin").await;
    credit(&pool, other, author, "Auteur").await;
    let gone = add_title(&pool, "Spirou supprimé", "bd", "BD", None).await;
    soft_delete(&pool, "titles", gone).await;

    let by_text = search_candidates(&pool, "spirou", current).await.unwrap();
    assert_eq!(
        by_text.iter().map(|c| c.id).collect::<Vec<_>>(),
        vec![other]
    );
    assert_eq!(by_text[0].primary_contributor.as_deref(), Some("Franquin"));

    // An ISBN typed with separators finds the title.
    let by_isbn = search_candidates(&pool, "978-2-8001-1026-4", current)
        .await
        .unwrap();
    assert_eq!(
        by_isbn.iter().map(|c| c.id).collect::<Vec<_>>(),
        vec![other]
    );

    assert!(
        search_candidates(&pool, "  ", current)
            .await
            .unwrap()
            .is_empty()
    );

    for n in 0..12 {
        add_title(&pool, &format!("Série longue {n:02}"), "bd", "BD", None).await;
    }
    assert_eq!(
        search_candidates(&pool, "Série longue", current)
            .await
            .unwrap()
            .len(),
        10
    );
}

// ─── Web ─────────────────────────────────────────────────────────

async fn send(
    app: &axum::Router,
    browser: &Browser,
    method: &str,
    path: &str,
    body: &str,
    modal_confirm: bool,
) -> axum::response::Response {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("cookie", format!("session={}", browser.cookie))
        .header("X-CSRF-Token", &browser.csrf)
        .header("HX-Request", "true");
    if modal_confirm {
        req = req.header("X-Modal-Confirm", "true");
    }
    app.clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn the_modal_picker_and_selection_render_escaped(pool: DbPool) {
    seed_admin(&pool, "mv_admin").await;
    let app = router(&pool);
    let browser = Browser::login(&app, "mv_admin").await;
    let from = add_title(&pool, "Départ <i>", "book", "Roman", None).await;
    let to = add_title(&pool, "Arrivée <script>", "book", "Roman", None).await;
    let id = add_volume(&pool, from, "V7808", None).await;

    // Direct navigation: 405, like the delete modal.
    let res = browser
        .send(&app, "GET", &format!("/volume/{id}/move-modal"), "", false)
        .await;
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);

    let res = send(
        &app,
        &browser,
        "GET",
        &format!("/volume/{id}/move-modal"),
        "",
        false,
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let modal = body_string(res).await;
    assert!(modal.contains(r#"<dialog open aria-modal="true""#));
    assert!(modal.contains(&format!(r#"hx-post="/volume/{id}/move""#)));
    assert!(modal.contains(r#"name="version" value="1""#));
    assert!(
        modal.contains("Départ &#60;i&#62;") || modal.contains("Départ &lt;i&gt;"),
        "{modal}"
    );
    assert!(!modal.contains("hx-confirm"));

    let res = send(
        &app,
        &browser,
        "GET",
        &format!("/volume/{id}/move/candidates?q=arriv"),
        "",
        false,
    )
    .await;
    let list = body_string(res).await;
    assert!(list.contains(&format!("title_id={to}")), "{list}");
    assert!(!list.contains("<script>"), "{list}");
    assert!(
        !list.contains(&format!("title_id={from}\"")),
        "current title excluded"
    );

    let res = send(
        &app,
        &browser,
        "GET",
        &format!("/volume/{id}/move/selection?title_id={to}"),
        "",
        false,
    )
    .await;
    let sel = body_string(res).await;
    assert!(
        sel.contains(&format!(r#"name="title_id" value="{to}""#)),
        "{sel}"
    );
    assert!(sel.contains("V7808"));
    assert!(!sel.contains("<script>"));

    // Picking the current title shows the refusal, without a title_id.
    let res = send(
        &app,
        &browser,
        "GET",
        &format!("/volume/{id}/move/selection?title_id={from}"),
        "",
        false,
    )
    .await;
    let same = body_string(res).await;
    assert!(!same.contains("name=\"title_id\""));
    assert!(same.contains("déjà rattaché à ce titre"), "{same}");
}

#[sqlx::test(migrations = "./migrations")]
async fn the_post_answers_for_the_modal(pool: DbPool) {
    seed_admin(&pool, "mv_admin").await;
    let app = router(&pool);
    let browser = Browser::login(&app, "mv_admin").await;
    let from = add_title(&pool, "Départ", "book", "Roman", None).await;
    let to = add_title(&pool, "Arrivée", "book", "Roman", None).await;
    let id = add_volume(&pool, from, "V7809", None).await;

    // A refusal from the modal's Confirm keeps its body in the modal.
    let res = send(
        &app,
        &browser,
        "POST",
        &format!("/volume/{id}/move"),
        "version=1&title_id=",
        true,
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(res.headers().get("hx-retarget").is_none());
    let body = body_string(res).await;
    assert!(body.contains("aucun titre n’a été choisi"), "{body}");

    let res = send(
        &app,
        &browser,
        "POST",
        &format!("/volume/{id}/move"),
        &format!("version=1&title_id={to}"),
        true,
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()
            .get("hx-trigger-after-swap")
            .and_then(|v| v.to_str().ok()),
        Some("modal-close")
    );
    let body = body_string(res).await;
    assert!(
        body.contains(r#"data-feedback-variant="success""#),
        "{body}"
    );
    assert!(
        body.contains("V7809 déplacé de « Départ » vers « Arrivée »."),
        "{body}"
    );
    assert!(
        body.contains(r#"id="volume-title-row" hx-swap-oob="true""#),
        "{body}"
    );
    assert!(body.contains(&format!(r#"href="/title/{to}""#)));

    // Replaying the same form: the version is stale now.
    let res = send(
        &app,
        &browser,
        "POST",
        &format!("/volume/{id}/move"),
        &format!("version=1&title_id={from}"),
        true,
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(title_id_of(&pool, id).await, to);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_anonymous_visitor_cannot_move(pool: DbPool) {
    seed_admin(&pool, "mv_admin").await;
    let app = router(&pool);
    let from = add_title(&pool, "Départ", "book", "Roman", None).await;
    let to = add_title(&pool, "Arrivée", "book", "Roman", None).await;
    let id = add_volume(&pool, from, "V7810", None).await;

    let anon = Browser::anonymous(&app).await;
    let res = anon
        .send(
            &app,
            "POST",
            &format!("/volume/{id}/move"),
            &format!("version=1&title_id={to}"),
            true,
        )
        .await;
    assert!(
        res.status() == StatusCode::SEE_OTHER
            || res.status() == StatusCode::UNAUTHORIZED
            || res.status() == StatusCode::OK && res.headers().contains_key("hx-redirect"),
        "anonymous move must bounce to login, got {}",
        res.status()
    );
    assert_eq!(title_id_of(&pool, id).await, from);

    let res = anon
        .send(
            &app,
            "GET",
            &format!("/volume/{id}/move/candidates?q=a"),
            "",
            true,
        )
        .await;
    assert_ne!(res.status(), StatusCode::OK);

    // The volume page shows no move button to an anonymous visitor.
    let (_, page) = anon.get(&app, &format!("/volume/{id}")).await;
    assert!(!page.contains("/move-modal"));
}

// ─── Review round 1 — title delete vs move, malformed form ───────

#[sqlx::test(migrations = "./migrations")]
async fn the_title_delete_refuses_a_title_with_volumes(pool: DbPool) {
    use mybibli::services::title::TitleService;
    let with = add_title(&pool, "Avec volume", "book", "Roman", None).await;
    add_volume(&pool, with, "V7811", None).await;
    let empty = add_title(&pool, "Sans volume", "book", "Roman", None).await;

    let err = TitleService::delete_without_volumes(&pool, with, "en")
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Conflict(_)), "{err:?}");
    TitleService::delete_without_volumes(&pool, empty, "en")
        .await
        .unwrap();
    let err = TitleService::delete_without_volumes(&pool, empty, "en")
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
}

/// A move holding its share lock on the target makes the title delete
/// wait; once the move commits, the delete sees the volume and refuses.
#[sqlx::test(migrations = "./migrations")]
async fn a_title_delete_waits_for_an_in_flight_move(pool: DbPool) {
    use mybibli::models::volume_move::{lock_volume_in, set_title_in, title_is_live_in};
    use mybibli::services::title::TitleService;
    let from = add_title(&pool, "Départ", "book", "Roman", None).await;
    let to = add_title(&pool, "Cible", "book", "Roman", None).await;
    let id = add_volume(&pool, from, "V7812", None).await;

    let mut move_tx = pool.begin().await.unwrap();
    let v = lock_volume_in(&mut move_tx, id).await.unwrap().unwrap();
    assert!(title_is_live_in(&mut move_tx, to).await.unwrap());
    assert_eq!(
        set_title_in(&mut move_tx, id, v.version, to).await.unwrap(),
        1
    );

    let delete_pool = pool.clone();
    let delete =
        tokio::spawn(
            async move { TitleService::delete_without_volumes(&delete_pool, to, "en").await },
        );
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(
        !delete.is_finished(),
        "the delete must wait for the move's lock"
    );

    move_tx.commit().await.unwrap();
    let err = delete.await.unwrap().unwrap_err();
    assert!(matches!(err, AppError::Conflict(_)), "{err:?}");
    assert_eq!(title_id_of(&pool, id).await, to);
    let (deleted,): (Option<chrono::NaiveDateTime>,) =
        sqlx::query_as("SELECT CAST(deleted_at AS DATETIME) FROM titles WHERE id = ?")
            .bind(to)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(deleted.is_none(), "the target title stays live");
}

/// A title delete holding its row lock makes the move wait; once the
/// delete commits, the move finds the target gone and writes nothing.
#[sqlx::test(migrations = "./migrations")]
async fn a_move_waits_for_an_in_flight_title_delete(pool: DbPool) {
    use mybibli::models::volume_move::lock_title_for_delete_in;
    use mybibli::services::soft_delete::SoftDeleteService;
    let from = add_title(&pool, "Départ", "book", "Roman", None).await;
    let to = add_title(&pool, "Cible", "book", "Roman", None).await;
    let id = add_volume(&pool, from, "V7813", None).await;

    let mut delete_tx = pool.begin().await.unwrap();
    assert!(lock_title_for_delete_in(&mut delete_tx, to).await.unwrap());
    SoftDeleteService::soft_delete_in(&mut delete_tx, "titles", to)
        .await
        .unwrap();

    let move_pool = pool.clone();
    let mv = tokio::spawn(async move {
        VolumeService::move_to_title(&move_pool, id, 1, Some(to), None, "en").await
    });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert!(
        !mv.is_finished(),
        "the move must wait for the delete's lock"
    );

    delete_tx.commit().await.unwrap();
    let err = mv.await.unwrap().unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
    assert_eq!(title_id_of(&pool, id).await, from);
}

#[sqlx::test(migrations = "./migrations")]
async fn a_malformed_version_gets_a_localized_feedback_entry(pool: DbPool) {
    seed_admin(&pool, "mv_admin").await;
    let app = router(&pool);
    let browser = Browser::login(&app, "mv_admin").await;
    let from = add_title(&pool, "Départ", "book", "Roman", None).await;
    let to = add_title(&pool, "Arrivée", "book", "Roman", None).await;
    let id = add_volume(&pool, from, "V7814", None).await;

    for body in [
        format!("version=abc&title_id={to}"),
        format!("title_id={to}"),
    ] {
        let res = send(
            &app,
            &browser,
            "POST",
            &format!("/volume/{id}/move"),
            &body,
            true,
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
        let html = body_string(res).await;
        assert!(html.contains("feedback-entry"), "{html}");
        assert!(html.contains("le formulaire était incomplet"), "{html}");
    }
    assert_eq!(title_id_of(&pool, id).await, from);

    // A 300-character query is accepted (cut to 200), not an error.
    let long = "a".repeat(300);
    let res = send(
        &app,
        &browser,
        "GET",
        &format!("/volume/{id}/move/candidates?q={long}"),
        "",
        false,
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}
