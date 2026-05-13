//! Integration tests for issue #68: dedicated SYSTEM user for
//! audit-trail attribution.
//!
//! Three properties to verify:
//!   1. `UserModel::find_system_user_id` returns the migration-planted
//!      row id (round-trip with the auto-purge audit code path).
//!   2. The `POST /login` query refuses to match the SYSTEM row even
//!      after lifting `active` to `TRUE` — defense in depth.
//!   3. The seed gate (issue #173) does NOT touch the SYSTEM row.
//!
//! To run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test system_user

use mybibli::models::user::UserModel;
use mybibli::services::seed_gate;
use sqlx::MySqlPool;

#[sqlx::test(migrations = "./migrations")]
async fn find_system_user_id_returns_seeded_row(pool: MySqlPool) {
    let id = UserModel::find_system_user_id(&pool)
        .await
        .expect("SYSTEM user must exist after migrations");

    // Cross-check: the row at that id is the SYSTEM row.
    let (username, role): (String, String) =
        sqlx::query_as("SELECT username, role FROM users WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .expect("read SYSTEM row by id");

    assert_eq!(username, "SYSTEM");
    assert_eq!(role, "system");
}

#[sqlx::test(migrations = "./migrations")]
async fn login_query_excludes_system_role(pool: MySqlPool) {
    // Defense-in-depth: even if an operator manually flips active=TRUE on
    // the SYSTEM row (an unlikely mistake, but a real one), the login
    // query must still refuse the role.
    sqlx::query("UPDATE users SET active = TRUE WHERE role = 'system'")
        .execute(&pool)
        .await
        .expect("flip active on SYSTEM row");

    // Replicate the exact query shape from `routes::auth::login`.
    let hit: Option<(u64,)> = sqlx::query_as(
        "SELECT id FROM users \
         WHERE username = ? AND active = TRUE AND deleted_at IS NULL \
           AND role IN ('admin', 'librarian')",
    )
    .bind("SYSTEM")
    .fetch_optional(&pool)
    .await
    .expect("login query");

    assert!(
        hit.is_none(),
        "login query must NEVER return the SYSTEM row, even with active=TRUE"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn seed_gate_leaves_system_user_alone(pool: MySqlPool) {
    // The seed gate (issue #173) soft-deletes seeded admin/librarian
    // rows. It must NOT touch SYSTEM — that row is the attribution
    // anchor for the audit trail and removing it would break the
    // auto-purge fallback.
    let before = UserModel::find_system_user_id(&pool)
        .await
        .expect("SYSTEM exists before gate");

    seed_gate::apply_with(&pool, false)
        .await
        .expect("gate runs cleanly");

    let after = UserModel::find_system_user_id(&pool)
        .await
        .expect("SYSTEM still exists after gate");

    assert_eq!(before, after, "SYSTEM user id must not change");

    let (deleted_at,): (Option<chrono::NaiveDateTime>,) = sqlx::query_as(
        "SELECT CAST(deleted_at AS DATETIME) FROM users WHERE id = ?",
    )
    .bind(after)
    .fetch_one(&pool)
    .await
    .expect("read SYSTEM deleted_at");

    assert!(
        deleted_at.is_none(),
        "seed gate must not soft-delete the SYSTEM row"
    );
}
