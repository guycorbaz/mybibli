//! Integration tests for issue #69: `users` is in `ALLOWED_TABLES`.
//!
//! Verifies the end-to-end effect of adding `users` to the soft-delete
//! whitelist: deactivated users appear in the Trash listing, the
//! permanent-delete service accepts the `users` table, and the
//! auto-purge scheduler hard-deletes users whose `deleted_at` is older
//! than 30 days.
//!
//! Handler-level coverage of the self-delete and last-active-admin
//! guards (Task 8 Scenario 3) lives in the Playwright E2E suite — see
//! `tests/e2e/specs/journeys/admin-users-trash.spec.ts` (to be added).
//!
//! To run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test users_in_allowed_tables

use mybibli::models::trash::TrashModel;
use mybibli::services::auto_purge::AutoPurgeService;
use mybibli::services::soft_delete::ALLOWED_TABLES;
use mybibli::services::trash::TrashService;
use sqlx::MySqlPool;

async fn create_test_user(pool: &MySqlPool, username: &str, role: &str) -> u64 {
    sqlx::query(
        "INSERT INTO users (username, password_hash, role, active) \
         VALUES (?, 'placeholder-hash', ?, TRUE)",
    )
    .bind(username)
    .bind(role)
    .execute(pool)
    .await
    .expect("insert test user");

    let (id,): (u64,) = sqlx::query_as("SELECT id FROM users WHERE username = ?")
        .bind(username)
        .fetch_one(pool)
        .await
        .expect("read test user id");
    id
}

async fn soft_delete_user(pool: &MySqlPool, id: u64) {
    sqlx::query("UPDATE users SET deleted_at = NOW() WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .expect("soft-delete user");
}

#[test]
fn users_is_in_allowed_tables() {
    assert!(
        ALLOWED_TABLES.contains(&"users"),
        "issue #69: users must be in ALLOWED_TABLES so the trash flow + \
         the existing self-delete / last-active-admin guards become reachable"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn deactivated_user_shows_up_in_trash_listing(pool: MySqlPool) {
    let user_id = create_test_user(&pool, "trash_user_69_a", "librarian").await;
    soft_delete_user(&pool, user_id).await;

    let entries = TrashModel::list_trash(&pool, 1, None, None)
        .await
        .expect("list trash succeeds");

    let user_entry = entries
        .iter()
        .find(|e| e.table_name == "users" && e.id == user_id);
    let user_entry = user_entry.expect("deactivated user must appear in trash");
    assert_eq!(user_entry.item_name, "trash_user_69_a");
}

#[sqlx::test(migrations = "./migrations")]
async fn trash_filtered_by_users_table_returns_only_users(pool: MySqlPool) {
    let user_id = create_test_user(&pool, "trash_user_69_b", "librarian").await;
    soft_delete_user(&pool, user_id).await;

    let entries = TrashModel::list_trash(&pool, 1, Some("users"), None)
        .await
        .expect("list trash filtered by users");

    assert!(
        entries.iter().all(|e| e.table_name == "users"),
        "filter `users` must only return rows from the users table"
    );
    assert!(entries.iter().any(|e| e.id == user_id));
}

#[sqlx::test(migrations = "./migrations")]
async fn permanent_delete_service_accepts_users_table(pool: MySqlPool) {
    let user_id = create_test_user(&pool, "trash_user_69_c", "librarian").await;
    soft_delete_user(&pool, user_id).await;

    // Read the version for the optimistic-locking check.
    let (version,): (i32,) = sqlx::query_as("SELECT version FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("read version");

    let deleted = TrashService::permanent_delete(&pool, "users", user_id, version)
        .await
        .expect("permanent_delete must accept users now that it's in ALLOWED_TABLES");

    assert_eq!(deleted.item_name, "trash_user_69_c");

    // Row must be physically gone after the hard delete.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("count after delete");
    assert_eq!(count, 0, "permanent_delete should physically remove the row");
}

#[sqlx::test(migrations = "./migrations")]
async fn auto_purge_hard_deletes_soft_deleted_users_after_30_days(pool: MySqlPool) {
    sqlx::query(
        "INSERT INTO users (username, password_hash, role, active, deleted_at) \
         VALUES ('old_user_69', 'placeholder-hash', 'librarian', FALSE, \
                 NOW() - INTERVAL 31 DAY)",
    )
    .execute(&pool)
    .await
    .expect("insert 31-day-old soft-deleted user");

    let stats = AutoPurgeService::run_purge(&pool)
        .await
        .expect("run_purge cleanly");

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = 'old_user_69'")
            .fetch_one(&pool)
            .await
            .expect("count after purge");
    assert_eq!(count, 0, "31-day-old soft-deleted user must be hard-purged");

    assert_eq!(
        stats.per_table.get("users").copied(),
        Some(1),
        "per_table should record one users-table deletion"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn auto_purge_does_not_touch_system_user(pool: MySqlPool) {
    // SYSTEM row has deleted_at IS NULL, so the auto-purge predicate
    // (`deleted_at < NOW() - 30 DAY`) must NEVER match it.
    AutoPurgeService::run_purge(&pool)
        .await
        .expect("run_purge cleanly");

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role = 'system'")
            .fetch_one(&pool)
            .await
            .expect("count system users post-purge");
    assert_eq!(count, 1, "SYSTEM user must never be touched by auto-purge");
}
