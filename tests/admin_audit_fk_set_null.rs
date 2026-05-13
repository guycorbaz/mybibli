//! Integration tests for issue #70: `admin_audit.user_id` FK
//! migrated from `ON DELETE CASCADE` to `ON DELETE SET NULL`, and
//! every production call site captures `user_username` +
//! `user_role` in the JSON `details` payload at insert time.
//!
//! To run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test admin_audit_fk_set_null

use mybibli::models::admin_audit::AdminAuditModel;
use serde_json::json;
use sqlx::MySqlPool;

#[sqlx::test(migrations = "./migrations")]
async fn fk_is_set_null_after_migration(pool: MySqlPool) {
    // Verify the FK constraint name + DELETE_RULE post-migration.
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT CONSTRAINT_NAME, DELETE_RULE \
           FROM information_schema.REFERENTIAL_CONSTRAINTS \
          WHERE TABLE_NAME = 'admin_audit' \
            AND REFERENCED_TABLE_NAME = 'users'",
    )
    .fetch_optional(&pool)
    .await
    .expect("query FK metadata");

    let (constraint_name, delete_rule) = row.expect("admin_audit -> users FK exists");
    assert_eq!(constraint_name, "admin_audit_user_fk");
    assert_eq!(delete_rule, "SET NULL");
}

#[sqlx::test(migrations = "./migrations")]
async fn user_id_column_is_nullable_after_migration(pool: MySqlPool) {
    let (is_nullable,): (String,) = sqlx::query_as(
        "SELECT IS_NULLABLE FROM information_schema.COLUMNS \
          WHERE TABLE_NAME = 'admin_audit' AND COLUMN_NAME = 'user_id'",
    )
    .fetch_one(&pool)
    .await
    .expect("query column metadata");

    assert_eq!(is_nullable, "YES", "admin_audit.user_id must be NULLable");
}

#[sqlx::test(migrations = "./migrations")]
async fn audit_row_survives_user_hard_delete(pool: MySqlPool) {
    // Set up: create a temporary admin user, write an audit row
    // attributed to them, then hard-delete the user. The audit row
    // must survive with `user_id = NULL` (and the JSON details
    // payload preserves the attribution).
    sqlx::query(
        "INSERT INTO users (username, password_hash, role, active) \
         VALUES ('temp_admin_70', 'placeholder-hash', 'admin', TRUE)",
    )
    .execute(&pool)
    .await
    .expect("create temp user");

    let (user_id,): (u64,) =
        sqlx::query_as("SELECT id FROM users WHERE username = 'temp_admin_70'")
            .fetch_one(&pool)
            .await
            .expect("read temp user id");

    let entry = AdminAuditModel::create(
        &pool,
        user_id,
        "permanent_delete_from_trash",
        Some("titles"),
        Some(42),
        Some(json!({
            "user_username": "temp_admin_70",
            "user_role": "admin",
            "item_name": "Test Item",
        })),
    )
    .await
    .expect("write audit row");

    // Sanity: row exists and points to user_id.
    let (before_user_id,): (Option<i64>,) =
        sqlx::query_as("SELECT CAST(user_id AS SIGNED) FROM admin_audit WHERE id = ?")
            .bind(entry.id as i64)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before_user_id, Some(user_id as i64));

    // Hard-delete the temp user. With the old CASCADE FK this would
    // wipe the audit row; with SET NULL it must survive.
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("hard-delete temp user");

    let (after_user_id, details_json): (Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT CAST(user_id AS SIGNED), CAST(details AS CHAR) \
           FROM admin_audit WHERE id = ?",
    )
    .bind(entry.id as i64)
    .fetch_one(&pool)
    .await
    .expect("read audit row post-delete");

    assert_eq!(
        after_user_id, None,
        "audit row must survive with user_id = NULL after FK SET NULL"
    );

    let details: serde_json::Value =
        serde_json::from_str(&details_json.expect("details still present")).unwrap();
    assert_eq!(details["user_username"], "temp_admin_70");
    assert_eq!(details["user_role"], "admin");
    assert_eq!(details["item_name"], "Test Item");
}

#[sqlx::test(migrations = "./migrations")]
async fn auto_purge_audit_details_carry_system_attribution(pool: MySqlPool) {
    use mybibli::services::auto_purge::AutoPurgeService;

    let stats = AutoPurgeService::run_purge(&pool)
        .await
        .expect("run_purge cleanly on empty DB");
    // Empty DB: no rows to purge, but the audit row is still written.
    let _ = stats;

    let (details_json,): (Option<String>,) = sqlx::query_as(
        "SELECT CAST(details AS CHAR) FROM admin_audit \
          WHERE action = 'auto_purge' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("read latest auto_purge audit row");

    let details: serde_json::Value =
        serde_json::from_str(&details_json.expect("auto_purge audit details NOT NULL")).unwrap();
    assert_eq!(details["user_username"], "SYSTEM");
    assert_eq!(details["user_role"], "system");
}
