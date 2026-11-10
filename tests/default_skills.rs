//! Story 17-6 (#656, FR177) — the MCP skills mybibli ships with: offered
//! once per database at startup, inactive, never over a name an admin
//! holds (live or in the Trash), never re-created once deleted, marked
//! "provided by mybibli" until an admin changes them.
//!
//! Run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test default_skills

mod common;

use common::mcp::router;
use common::web::{Browser, seed_admin};
use mybibli::db::DbPool;
use mybibli::models::mcp_skill::McpSkillModel;
use mybibli::services::default_skills::{DEFAULT_SKILLS, DefaultsReport, insert_defaults};
use mybibli::services::mcp_skills::{served_skill, served_skills, validate};

/// The admin pages answer in the instance's default language (French on
/// a fresh database); the mention is matched in English or French.
fn count_provided(html: &str) -> usize {
    html.matches("provided by mybibli").count() + html.matches("fourni par mybibli").count()
}

fn all_names() -> Vec<&'static str> {
    DEFAULT_SKILLS.iter().map(|d| d.name).collect()
}

async fn admin_create(pool: &DbPool, name: &str, body: &str, active: bool) -> u64 {
    let skill = validate(name, "The admin's own", body, active).unwrap();
    let author: Option<u64> = sqlx::query_scalar("SELECT id FROM users WHERE username = 'owner'")
        .fetch_optional(pool)
        .await
        .unwrap();
    McpSkillModel::create(pool, &skill, author).await.unwrap()
}

async fn markers(pool: &DbPool) -> Vec<String> {
    sqlx::query_scalar("SELECT name FROM data_repairs WHERE name LIKE 'default-skill:%' ORDER BY name")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn a_fresh_database_gets_every_default_inactive_and_unserved(pool: DbPool) {
    let report = insert_defaults(&pool).await;
    assert_eq!(report.inserted, all_names());
    assert!(report.name_taken.is_empty() && report.already_offered.is_empty());

    let rows = McpSkillModel::list_all(&pool).await.unwrap();
    assert_eq!(rows.len(), DEFAULT_SKILLS.len());
    for (row, shipped) in rows.iter().zip(DEFAULT_SKILLS) {
        assert_eq!(row.name, shipped.name);
        assert_eq!(row.description, shipped.description);
        assert_eq!(row.body, shipped.body, "stored exactly as shipped");
        assert!(!row.active, "{}: inserted inactive", row.name);
        assert!(row.updated_by_name.is_none());
        assert!(row.provided_by_mybibli);
        assert_eq!(row.version, 1);
        assert!(served_skill(&pool, &row.name).await.unwrap().is_none(), "{}", row.name);
    }
    assert_eq!(served_skills(&pool).await.unwrap().total, 0, "nothing served while inactive");
    assert_eq!(markers(&pool).await.len(), DEFAULT_SKILLS.len(), "one marker per default");
}

#[sqlx::test(migrations = "./migrations")]
async fn a_second_startup_inserts_nothing(pool: DbPool) {
    insert_defaults(&pool).await;
    let report = insert_defaults(&pool).await;
    assert_eq!(
        report,
        DefaultsReport { already_offered: all_names(), ..Default::default() }
    );
    assert_eq!(McpSkillModel::list_all(&pool).await.unwrap().len(), DEFAULT_SKILLS.len());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_name_held_by_a_live_or_trashed_skill_is_never_overwritten(pool: DbPool) {
    seed_admin(&pool, "owner").await;
    // The admin's own live skill, active, under a default's name …
    let live = admin_create(&pool, "comics-covers", "# Mine", true).await;
    // … and another one in the Trash.
    let trashed = admin_create(&pool, "comics-title-spelling", "# Trashed", false).await;
    McpSkillModel::soft_delete(&pool, trashed, 1).await.unwrap();

    let report = insert_defaults(&pool).await;
    assert_eq!(report.name_taken, ["comics-covers", "comics-title-spelling"]);
    assert_eq!(report.inserted, ["comics-series-attach", "comics-series-create"]);

    let mine = McpSkillModel::find_by_id(&pool, live).await.unwrap().unwrap();
    assert_eq!((mine.body.as_str(), mine.active, mine.version), ("# Mine", true, 1));
    assert_eq!(mine.description, "The admin's own");
    assert!(!mine.provided_by_mybibli, "the admin's row is not marked");
    assert_eq!(mine.updated_by_name.as_deref(), Some("owner"), "author untouched");
    let (body, deleted): (String, bool) = sqlx::query_as(
        "SELECT body, deleted_at IS NOT NULL FROM mcp_skills WHERE id = ?",
    )
    .bind(trashed)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((body.as_str(), deleted), ("# Trashed", true), "the trashed skill stays as it was");

    // Offered once: the skip is not retried, even after the name frees up.
    McpSkillModel::soft_delete(&pool, live, 1).await.unwrap();
    sqlx::query("DELETE FROM mcp_skills WHERE id IN (?, ?)")
        .bind(live)
        .bind(trashed)
        .execute(&pool)
        .await
        .unwrap();
    let again = insert_defaults(&pool).await;
    assert!(again.inserted.is_empty(), "{again:?}");
    assert_eq!(markers(&pool).await.len(), DEFAULT_SKILLS.len());
}

#[sqlx::test(migrations = "./migrations")]
async fn a_deleted_default_never_comes_back(pool: DbPool) {
    insert_defaults(&pool).await;
    let rows = McpSkillModel::list_all(&pool).await.unwrap();
    let covers = rows.iter().find(|s| s.name == "comics-covers").unwrap().id;
    let spelling = rows.iter().find(|s| s.name == "comics-title-spelling").unwrap().id;

    // One in the Trash, one purged for good.
    McpSkillModel::soft_delete(&pool, covers, 1).await.unwrap();
    sqlx::query("DELETE FROM mcp_skills WHERE id = ?")
        .bind(spelling)
        .execute(&pool)
        .await
        .unwrap();

    // Restarted.
    let report = insert_defaults(&pool).await;
    assert!(report.inserted.is_empty(), "{report:?}");
    let names: Vec<_> = McpSkillModel::list_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, ["comics-series-attach", "comics-series-create"]);
}

#[sqlx::test(migrations = "./migrations")]
async fn an_admin_change_replaces_the_mention_and_activation_serves_it(pool: DbPool) {
    let app = router(&pool);
    seed_admin(&pool, "defaults_admin").await;
    let admin = Browser::login(&app, "defaults_admin").await;
    insert_defaults(&pool).await;
    let rows = McpSkillModel::list_all(&pool).await.unwrap();
    let covers = rows.iter().find(|s| s.name == "comics-covers").unwrap();
    let attach = rows.iter().find(|s| s.name == "comics-series-attach").unwrap();

    // The admin list shows the mention for every untouched default.
    let res = admin.send(&app, "GET", "/admin/skills", "", true).await;
    let html = common::web::body_string(res).await;
    assert_eq!(count_provided(&html), DEFAULT_SKILLS.len(), "{html}");
    assert!(!html.contains("author unknown") && !html.contains("auteur inconnu"), "{html}");

    // Activation is a change: the admin becomes the author, and the agent
    // is now served the default.
    let (status, body) = admin
        .post_form(&app, &format!("/admin/skills/{}/active", covers.id), "version=1&active=1")
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body}");
    let s = McpSkillModel::find_by_id(&pool, covers.id).await.unwrap().unwrap();
    assert!(s.active && !s.provided_by_mybibli);
    assert_eq!(s.updated_by_name.as_deref(), Some("defaults_admin"));
    let served = served_skill(&pool, "comics-covers").await.unwrap().unwrap();
    assert_eq!(served.body, covers.body);

    // A text edit through the editor, by the admin.
    let (status, body) = admin
        .post_form(
            &app,
            &format!("/admin/skills/{}", attach.id),
            "name=comics-series-attach&description=Mine&body=%23%20Edited&version=1",
        )
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body}");
    let s = McpSkillModel::find_by_id(&pool, attach.id).await.unwrap().unwrap();
    assert!(!s.provided_by_mybibli && !s.active, "an edit clears the mention");
    assert_eq!(s.body, "# Edited");
    assert_eq!(s.updated_by_name.as_deref(), Some("defaults_admin"));

    // Deactivating the activated default keeps the admin as author.
    let (status, body) = admin
        .post_form(&app, &format!("/admin/skills/{}/active", covers.id), "version=2&active=0")
        .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body}");
    let s = McpSkillModel::find_by_id(&pool, covers.id).await.unwrap().unwrap();
    assert!(!s.active && !s.provided_by_mybibli);

    let res = admin.send(&app, "GET", "/admin/skills", "", true).await;
    let html = common::web::body_string(res).await;
    assert_eq!(count_provided(&html), DEFAULT_SKILLS.len() - 2);
    assert_eq!(
        html.matches("by defaults_admin").count() + html.matches("par defaults_admin").count(),
        2,
        "{html}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn a_restored_default_keeps_its_mention(pool: DbPool) {
    insert_defaults(&pool).await;
    let covers = McpSkillModel::list_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.name == "comics-covers")
        .unwrap();
    McpSkillModel::soft_delete(&pool, covers.id, covers.version).await.unwrap();
    mybibli::services::trash::TrashService::restore(&pool, "mcp_skills", covers.id, covers.version)
        .await
        .unwrap();
    let s = McpSkillModel::find_by_id(&pool, covers.id).await.unwrap().unwrap();
    assert!(s.provided_by_mybibli && !s.active && s.updated_by_name.is_none());
}
