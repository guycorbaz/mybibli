//! Story 15-2 (#614, FR162) — MCP skills: the model (CRUD, uniqueness
//! among live rows, optimistic locking, soft delete + Trash restore, name
//! reuse, the reads story 15-3 needs) and the Admin → Skills routes (role
//! gating, escaping, the delete → Trash → restore round trip).
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test mcp_skills

mod common;

use axum::http::StatusCode;
use common::mcp::router;
use common::web::{Browser, PW_HASH, body_string, get_anonymous, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::CreateOutcome;
use mybibli::models::mcp_skill::{McpSkillModel, SkillWriteError};
use mybibli::services::mcp_skills::{ValidSkill, validate};
use mybibli::services::trash::TrashService;

fn skill(name: &str, body: &str, active: bool) -> ValidSkill {
    validate(name, &format!("About {name}"), body, active).unwrap()
}

async fn user_id(pool: &DbPool, username: &str) -> u64 {
    sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn version_of(pool: &DbPool, id: u64) -> i32 {
    sqlx::query_scalar("SELECT version FROM mcp_skills WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn create(pool: &DbPool, name: &str, body: &str, active: bool) -> u64 {
    match McpSkillModel::create(pool, &skill(name, body, active), None)
        .await
        .unwrap_or_else(|e| panic!("create {name}: {e:?}"))
    {
        CreateOutcome::Created(id) => id,
        other => panic!("expected Created, got {other:?}"),
    }
}

// ─── Model ───────────────────────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn create_list_and_find(pool: DbPool) {
    seed_admin(&pool, "skills_admin").await;
    let admin = user_id(&pool, "skills_admin").await;
    let id = match McpSkillModel::create(&pool, &skill("zeta", "# Z", true), Some(admin))
        .await
        .unwrap()
    {
        CreateOutcome::Created(id) => id,
        other => panic!("{other:?}"),
    };
    create(&pool, "alpha", "# A", false).await;

    let all = McpSkillModel::list_all(&pool).await.unwrap();
    let names: Vec<_> = all.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["alpha", "zeta"], "admin list includes inactive, by name");

    let zeta = McpSkillModel::find_by_id(&pool, id).await.unwrap().unwrap();
    assert_eq!(zeta.body, "# Z");
    assert!(zeta.active);
    assert_eq!(zeta.version, 1);
    assert_eq!(zeta.updated_by_name.as_deref(), Some("skills_admin"));

    // Story 15-3 reads: live and active only.
    let active = McpSkillModel::list_active(&pool).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "zeta");
    assert_eq!(active[0].description, "About zeta");
    assert!(McpSkillModel::find_active_by_name(&pool, "zeta").await.unwrap().is_some());
    assert!(
        McpSkillModel::find_active_by_name(&pool, "alpha").await.unwrap().is_none(),
        "an inactive skill is invisible to the agent"
    );
    assert!(McpSkillModel::find_active_by_name(&pool, "nope").await.unwrap().is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn names_are_unique_among_live_rows(pool: DbPool) {
    create(&pool, "rules", "one", true).await;
    let dup = McpSkillModel::create(&pool, &skill("rules", "two", true), None).await;
    assert!(matches!(dup, Err(SkillWriteError::NameTaken)), "{dup:?}");

    let other = create(&pool, "other", "x", true).await;
    let v = version_of(&pool, other).await;
    let rename = McpSkillModel::update(&pool, other, v, &skill("rules", "x", true), None).await;
    assert!(matches!(rename, Err(SkillWriteError::NameTaken)), "{rename:?}");
}

#[sqlx::test(migrations = "./migrations")]
async fn update_and_set_active_are_guarded_by_version(pool: DbPool) {
    seed_admin(&pool, "skills_editor").await;
    let editor = user_id(&pool, "skills_editor").await;
    let id = create(&pool, "rules", "v1", true).await;

    McpSkillModel::update(&pool, id, 1, &skill("rules-2", "v2", true), Some(editor))
        .await
        .unwrap();
    let s = McpSkillModel::find_by_id(&pool, id).await.unwrap().unwrap();
    assert_eq!((s.name.as_str(), s.body.as_str(), s.version), ("rules-2", "v2", 2));
    assert_eq!(s.updated_by_name.as_deref(), Some("skills_editor"));

    // A second save from an editor opened at version 1 loses.
    let stale = McpSkillModel::update(&pool, id, 1, &skill("rules-2", "v3", true), None).await;
    assert!(matches!(stale, Err(SkillWriteError::Stale)), "{stale:?}");
    let stale = McpSkillModel::set_active(&pool, id, 1, false, None).await;
    assert!(matches!(stale, Err(SkillWriteError::Stale)), "{stale:?}");

    McpSkillModel::set_active(&pool, id, 2, false, None).await.unwrap();
    let s = McpSkillModel::find_by_id(&pool, id).await.unwrap().unwrap();
    assert!(!s.active);
    assert_eq!(s.version, 3);
    assert!(McpSkillModel::list_active(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn soft_delete_goes_to_the_trash_and_restores(pool: DbPool) {
    let id = create(&pool, "rules", "body", true).await;

    let stale = McpSkillModel::soft_delete(&pool, id, 99).await;
    assert!(matches!(stale, Err(SkillWriteError::Stale)), "{stale:?}");
    McpSkillModel::soft_delete(&pool, id, 1).await.unwrap();

    assert!(McpSkillModel::find_by_id(&pool, id).await.unwrap().is_none());
    assert!(McpSkillModel::list_all(&pool).await.unwrap().is_empty());
    assert!(McpSkillModel::find_active_by_name(&pool, "rules").await.unwrap().is_none());
    let gone = McpSkillModel::update(&pool, id, 1, &skill("rules", "x", true), None).await;
    assert!(matches!(gone, Err(SkillWriteError::Gone)), "{gone:?}");

    // The Trash lists it under its name, and restores it.
    let trash = mybibli::models::trash::TrashModel::list_trash(&pool, 1, Some("mcp_skills"), None)
        .await
        .unwrap();
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].item_name, "rules");
    let entry = TrashService::restore(&pool, "mcp_skills", id, trash[0].version)
        .await
        .unwrap();
    assert_eq!(entry.item_name, "rules");
    assert!(McpSkillModel::find_active_by_name(&pool, "rules").await.unwrap().is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_trashed_name_is_reused_by_reactivating_the_row(pool: DbPool) {
    let id = create(&pool, "rules", "old text", true).await;
    McpSkillModel::soft_delete(&pool, id, 1).await.unwrap();

    // Renaming another skill to the trashed name is refused, explicitly.
    let other = create(&pool, "other", "x", true).await;
    let rename = McpSkillModel::update(&pool, other, 1, &skill("rules", "x", true), None).await;
    assert!(matches!(rename, Err(SkillWriteError::NameInTrash)), "{rename:?}");

    // Creating it again reactivates the trashed row with the new text.
    let outcome = McpSkillModel::create(&pool, &skill("rules", "new text", false), None)
        .await
        .unwrap();
    assert!(matches!(outcome, CreateOutcome::Reactivated(r) if r == id), "{outcome:?}");
    let s = McpSkillModel::find_by_id(&pool, id).await.unwrap().unwrap();
    assert_eq!(s.body, "new text");
    assert!(!s.active);
    assert_eq!(s.version, 2);
    // Nothing left in the Trash to collide with on a later restore.
    let trash = mybibli::models::trash::TrashModel::trash_count(&pool, Some("mcp_skills"), None)
        .await
        .unwrap();
    assert_eq!(trash, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn the_largest_body_round_trips(pool: DbPool) {
    let body = "é".repeat(mybibli::services::mcp_skills::BODY_MAX_BYTES / 2);
    let id = create(&pool, "big", &body, true).await;
    let s = McpSkillModel::find_by_id(&pool, id).await.unwrap().unwrap();
    assert_eq!(s.body, body);
}

#[sqlx::test(migrations = "./migrations")]
async fn auto_purge_hard_deletes_an_old_trashed_skill(pool: DbPool) {
    let id = create(&pool, "old", "x", true).await;
    sqlx::query("UPDATE mcp_skills SET deleted_at = NOW() - INTERVAL 31 DAY WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let stats = mybibli::services::auto_purge::AutoPurgeService::run_purge(&pool)
        .await
        .unwrap();
    assert_eq!(stats.per_table.get("mcp_skills").copied(), Some(1));
}

// ─── Routes ──────────────────────────────────────────────────────

async fn seed_librarian(pool: &DbPool, username: &str) {
    sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'librarian')")
        .bind(username)
        .bind(PW_HASH)
        .execute(pool)
        .await
        .unwrap();
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencoding(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn urlencoding(v: &str) -> String {
    percent_encoding::utf8_percent_encode(v, percent_encoding::NON_ALPHANUMERIC).to_string()
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_is_sent_to_login_and_librarian_is_refused(pool: DbPool) {
    let app = router(&pool);
    let res = get_anonymous(&app, "/admin/skills").await;
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    let location = res.headers().get("location").unwrap().to_str().unwrap();
    assert!(location.starts_with("/login?next="), "{location}");

    seed_librarian(&pool, "skills_lib").await;
    let lib = Browser::login(&app, "skills_lib").await;
    let (status, _) = lib.get(&app, "/admin?tab=skills").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = lib
        .post_form(
            &app,
            "/admin/skills",
            &form(&[("name", "x"), ("description", "d"), ("body", "b"), ("active", "1")]),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(McpSkillModel::list_all(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_creates_edits_toggles_and_deletes_through_the_routes(pool: DbPool) {
    let app = router(&pool);
    seed_admin(&pool, "skills_web").await;
    let admin = Browser::login(&app, "skills_web").await;

    // The tab renders (full page, deep link).
    let (status, html) = admin.get(&app, "/admin?tab=skills").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("id=\"admin-skills-editor\""), "editor present");
    assert!(html.contains("id=\"tab-skills\""), "tab present");

    // A malformed name is refused with a localized field hint.
    let (status, body) = admin
        .post_form(
            &app,
            "/admin/skills",
            &form(&[("name", "Bad Name"), ("description", "d"), ("body", "b")]),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!body.contains("error.mcp_skills"), "{body}");

    // Create, with HTML in the body: shown escaped, never rendered.
    let (status, body) = admin
        .post_form(
            &app,
            "/admin/skills",
            &form(&[
                ("name", "catalog-rules"),
                ("description", "How to catalog"),
                ("body", "<script>alert(1)</script>\n**Bold**"),
                ("active", "1"),
            ]),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("<script>alert(1)"), "{body}");
    assert!(body.contains("&#60;script&#62;alert(1)&#60;/script&#62;"), "{body}");
    let s = McpSkillModel::list_all(&pool).await.unwrap().remove(0);
    assert_eq!(s.updated_by_name.as_deref(), Some("skills_web"));

    // Edit form, then a save, then a stale save.
    let (status, edit) = admin.get(&app, &format!("/admin/skills/{}/edit", s.id)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(edit.contains("name=\"version\" value=\"1\""), "{edit}");
    let save = form(&[
        ("name", "catalog-rules"),
        ("description", "How to catalog, v2"),
        ("body", "v2"),
        ("active", "1"),
        ("version", "1"),
    ]);
    let (status, _) = admin.post_form(&app, &format!("/admin/skills/{}", s.id), &save).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = admin.post_form(&app, &format!("/admin/skills/{}", s.id), &save).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // Deactivate (no "active" on an edit → unticked is honoured too).
    let (status, _) = admin
        .post_form(
            &app,
            &format!("/admin/skills/{}/active", s.id),
            &form(&[("version", "2"), ("active", "0")]),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(McpSkillModel::list_active(&pool).await.unwrap().is_empty());

    // Delete through the modal's endpoint → Trash.
    let (status, modal) = admin
        .get(&app, &format!("/admin/skills/{}/delete-modal?version=3", s.id))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(modal.contains("aria-modal=\"true\""), "{modal}");
    let res = admin
        .send(&app, "POST", &format!("/admin/skills/{}/delete", s.id), "version=3", true)
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get("hx-trigger").and_then(|v| v.to_str().ok()),
        Some("modal-close")
    );
    let _ = body_string(res).await;
    assert!(McpSkillModel::find_by_id(&pool, s.id).await.unwrap().is_none());
    let (_, trash) = admin.get(&app, "/admin?tab=trash").await;
    assert!(trash.contains("catalog-rules"), "the trashed skill is listed");
}
