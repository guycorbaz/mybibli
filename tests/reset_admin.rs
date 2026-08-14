//! CR #459 — `MYBIBLI_RESET_ADMIN` startup hatch. DB-backed integration
//! tests for `services::admin_reset::reset_admin_password`: the reset +
//! session-invalidation transaction, and the three refusal paths
//! (unknown username, deactivated account, non-admin role). The
//! exit-non-zero half of the design lives in `main()` and is covered by
//! the pure `parse_reset_request` unit tests plus manual procedure docs.

use mybibli::db::DbPool;
use mybibli::services::admin_reset::reset_admin_password;
use mybibli::services::password::{hash_password, verify_password};

async fn insert_user(pool: &DbPool, username: &str, role: &str, deleted: bool) -> u64 {
    let hash = hash_password("old-password").expect("hash");
    let deleted_expr = if deleted { "NOW()" } else { "NULL" };
    let row: (u64,) = sqlx::query_as(&format!(
        "INSERT INTO users (username, password_hash, role, deleted_at) \
         VALUES (?, ?, ?, {deleted_expr}) RETURNING id"
    ))
    .bind(username)
    .bind(&hash)
    .bind(role)
    .fetch_one(pool)
    .await
    .expect("insert user");
    row.0
}

async fn insert_session(pool: &DbPool, user_id: u64, token: &str) {
    sqlx::query("INSERT INTO sessions (token, user_id, csrf_token, data) VALUES (?, ?, ?, '{}')")
        .bind(token)
        .bind(user_id)
        .bind(format!("csrf-{token}"))
        .execute(pool)
        .await
        .expect("insert session");
}

#[sqlx::test(migrations = "./migrations")]
async fn reset_changes_password_and_kills_sessions(pool: DbPool) {
    let user_id = insert_user(&pool, "locked-admin", "admin", false).await;
    insert_session(&pool, user_id, "reset-hatch-session-1").await;
    insert_session(&pool, user_id, "reset-hatch-session-2").await;

    let outcome = reset_admin_password(&pool, "locked-admin")
        .await
        .expect("reset should succeed");

    assert_eq!(outcome.user_id, user_id);
    assert_eq!(outcome.username, "locked-admin");
    assert_eq!(outcome.sessions_killed, 2);

    let (stored_hash, version): (String, i32) =
        sqlx::query_as("SELECT password_hash, version FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .expect("fetch user");
    assert!(
        verify_password(&outcome.password, &stored_hash),
        "generated password must verify against the stored hash"
    );
    assert!(
        !verify_password("old-password", &stored_hash),
        "old password must no longer verify"
    );
    assert_eq!(version, 2, "optimistic-locking version must be bumped");

    let (live_sessions,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sessions WHERE user_id = ? AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .expect("count sessions");
    assert_eq!(live_sessions, 0, "no live session may survive the reset");
}

#[sqlx::test(migrations = "./migrations")]
async fn reset_writes_an_audit_row(pool: DbPool) {
    let user_id = insert_user(&pool, "audited-admin", "admin", false).await;

    reset_admin_password(&pool, "audited-admin")
        .await
        .expect("reset should succeed");

    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM admin_audit \
         WHERE action = 'admin_password_reset_hatch' AND entity_id = ?",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .expect("count audit rows");
    assert_eq!(count, 1, "the reset must leave a forensics row");
}

#[sqlx::test(migrations = "./migrations")]
async fn unknown_username_is_refused_without_side_effects(pool: DbPool) {
    let err = reset_admin_password(&pool, "nobody-here")
        .await
        .expect_err("unknown username must be refused");
    assert!(
        err.to_string().contains("no user named"),
        "unexpected error: {err}"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn deactivated_admin_is_refused_and_untouched(pool: DbPool) {
    let user_id = insert_user(&pool, "gone-admin", "admin", true).await;

    let err = reset_admin_password(&pool, "gone-admin")
        .await
        .expect_err("deactivated account must be refused");
    assert!(
        err.to_string().contains("deactivated"),
        "unexpected error: {err}"
    );

    let (stored_hash, version): (String, i32) =
        sqlx::query_as("SELECT password_hash, version FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .expect("fetch user");
    assert!(
        verify_password("old-password", &stored_hash),
        "a refused reset must not change the password"
    );
    assert_eq!(version, 1, "a refused reset must not bump the version");
}

#[sqlx::test(migrations = "./migrations")]
async fn non_admin_role_is_refused(pool: DbPool) {
    insert_user(&pool, "plain-librarian", "librarian", false).await;

    let err = reset_admin_password(&pool, "plain-librarian")
        .await
        .expect_err("non-admin account must be refused");
    assert!(
        err.to_string().contains("not 'admin'"),
        "unexpected error: {err}"
    );
}
