//! Integration test for issue #173: production gate against the dev
//! seed migrations — and for issue #480, which turned the gate's
//! soft-delete into a hard-delete and added the seeded `sessions`
//! row to what it removes.
//!
//! Each test runs `sqlx::migrate!("./migrations")` against a fresh
//! database (via `#[sqlx::test]`), then invokes
//! `services::seed_gate::apply_with` with both branches of the
//! `MYBIBLI_SEED_DEV_USERS` flag to verify the documented behaviour.
//!
//! To run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     SQLX_OFFLINE=true \
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test seed_gate

use mybibli::services::seed_gate::{self, DEV_SESSION_TOKEN};
use sqlx::MySqlPool;

/// Count active rows (not soft-deleted) whose username is in the
/// seeded set.
async fn count_seeded_active(pool: &MySqlPool) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users \
         WHERE username IN ('admin', 'librarian') AND deleted_at IS NULL",
    )
    .fetch_one(pool)
    .await
    .expect("count active seeded users")
}

/// Count rows whose username is in the seeded set, soft-deleted or not.
/// Post-#480 this is the assertion that matters: the rows must be gone
/// from the table, not merely marked deleted.
async fn count_seeded_total(pool: &MySqlPool) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE username IN ('admin', 'librarian')",
    )
    .fetch_one(pool)
    .await
    .expect("count all seeded users")
}

async fn count_sessions_with_token(pool: &MySqlPool, token: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM sessions WHERE token = ?")
        .bind(token)
        .fetch_one(pool)
        .await
        .expect("count sessions by token")
}

async fn seeded_admin_id(pool: &MySqlPool) -> u64 {
    sqlx::query_scalar::<_, u64>("SELECT id FROM users WHERE username = 'admin'")
        .fetch_one(pool)
        .await
        .expect("read seeded admin id")
}

#[sqlx::test(migrations = "./migrations")]
async fn gate_disabled_hard_deletes_both_seeded_users(pool: MySqlPool) {
    // Pre-condition: fresh migrations created admin + librarian, both active.
    assert_eq!(
        count_seeded_active(&pool).await,
        2,
        "migrations should seed both admin and librarian as active"
    );

    // Act: env unset (seed gate disabled — production default).
    let removed = seed_gate::apply_with(&pool, false)
        .await
        .expect("seed gate should succeed");

    // Post-condition: both rows physically gone. A soft-delete would
    // leave them in the Trash panel, one Restore click away from a
    // live administrator whose password is published in SECURITY.md.
    assert_eq!(removed, 2, "two seeded users should have been deleted");
    assert_eq!(count_seeded_active(&pool).await, 0);
    assert_eq!(
        count_seeded_total(&pool).await,
        0,
        "seeded rows must be hard-deleted, not soft-deleted (issue #480)"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn gate_disabled_removes_the_published_session_row(pool: MySqlPool) {
    // Pre-condition: the seed migration planted a session row whose
    // token is published in CLAUDE.md and in the git history.
    assert_eq!(
        count_sessions_with_token(&pool, DEV_SESSION_TOKEN).await,
        1,
        "migrations should seed the dev session row"
    );

    seed_gate::apply_with(&pool, false)
        .await
        .expect("seed gate should succeed");

    assert_eq!(
        count_sessions_with_token(&pool, DEV_SESSION_TOKEN).await,
        0,
        "the published session token must not survive the gate (issue #480)"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn gate_disabled_clears_other_sessions_of_the_seeded_users(pool: MySqlPool) {
    // `fk_sessions_user` has no CASCADE, so any session row pointing at
    // a seeded user would abort the user DELETE with 23000. Plant one
    // with a token other than the seeded one to prove the gate clears
    // them by `user_id`, not just by the known token.
    let admin_id = seeded_admin_id(&pool).await;
    sqlx::query(
        "INSERT INTO sessions (token, user_id, data, last_activity) \
         VALUES ('seed-gate-test-token-0000000000000000000000', ?, '{}', NOW())",
    )
    .bind(admin_id)
    .execute(&pool)
    .await
    .expect("plant a second session for the seeded admin");

    let removed = seed_gate::apply_with(&pool, false)
        .await
        .expect("seed gate should succeed despite the extra session row");

    assert_eq!(removed, 2);
    assert_eq!(count_seeded_total(&pool).await, 0);
    assert_eq!(
        count_sessions_with_token(&pool, "seed-gate-test-token-0000000000000000000000").await,
        0,
        "sessions of a seeded user must go with the user"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn gate_disabled_detaches_admin_audit_rather_than_cascading(pool: MySqlPool) {
    // #69 / #70 made `admin_audit.user_id` ON DELETE SET NULL so a
    // hard-deleted actor leaves their audit history behind. The gate
    // relies on that: it must not silently wipe forensics.
    let admin_id = seeded_admin_id(&pool).await;
    sqlx::query(
        "INSERT INTO admin_audit (user_id, action, entity_type, entity_id, details) \
         VALUES (?, 'seed_gate_test', 'titles', 1, '{}')",
    )
    .bind(admin_id)
    .execute(&pool)
    .await
    .expect("plant an audit row attributed to the seeded admin");

    seed_gate::apply_with(&pool, false)
        .await
        .expect("seed gate should succeed");

    let rows: Vec<(u64, Option<i64>)> = sqlx::query_as(
        "SELECT id, CAST(user_id AS SIGNED) AS user_id \
           FROM admin_audit WHERE action = 'seed_gate_test'",
    )
    .fetch_all(&pool)
    .await
    .expect("read audit rows");

    assert_eq!(rows.len(), 1, "the audit row must survive the user delete");
    assert!(
        rows[0].1.is_none(),
        "the audit row should be detached (user_id NULL), not cascaded away"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn gate_enabled_preserves_seeded_users_and_session(pool: MySqlPool) {
    // Pre-condition: fresh seed.
    assert_eq!(count_seeded_active(&pool).await, 2);

    // Act: env set to true (dev / E2E branch).
    let removed = seed_gate::apply_with(&pool, true)
        .await
        .expect("seed gate should succeed");

    // Post-condition: both rows still active, session row untouched —
    // the integration suite and the Playwright helpers depend on them.
    assert_eq!(removed, 0, "no rows should be touched when seed gate is opt-in");
    assert_eq!(count_seeded_active(&pool).await, 2);
    assert_eq!(count_sessions_with_token(&pool, DEV_SESSION_TOKEN).await, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn gate_disabled_skips_rotated_password(pool: MySqlPool) {
    // Pre-condition: rotate the admin password before the gate runs.
    // This simulates an operator who has already followed the
    // pre-1.1.0 mitigation advice (rotate the seeded credentials).
    sqlx::query(
        "UPDATE users \
            SET password_hash = '$argon2id$v=19$m=19456,t=2,p=1$rotated-salt$rotated-hash-value', \
                version = version + 1 \
          WHERE username = 'admin'",
    )
    .execute(&pool)
    .await
    .expect("rotate admin password");

    // Act: env unset.
    let removed = seed_gate::apply_with(&pool, false)
        .await
        .expect("seed gate should succeed");

    // Post-condition: only the librarian was removed; the rotated admin
    // keeps their row, live.
    assert_eq!(removed, 1, "only the un-rotated librarian should be deleted");

    let rows: Vec<(String, Option<chrono::NaiveDateTime>)> = sqlx::query_as(
        "SELECT username, CAST(deleted_at AS DATETIME) AS deleted_at \
           FROM users \
          WHERE username IN ('admin', 'librarian') \
          ORDER BY username",
    )
    .fetch_all(&pool)
    .await
    .expect("read seeded user state");

    assert_eq!(rows.len(), 1, "the un-rotated librarian row should be gone");
    assert_eq!(rows[0].0, "admin");
    assert!(rows[0].1.is_none(), "rotated admin should stay live");

    // The published session token goes regardless of whose row it
    // points at: the value itself is public.
    assert_eq!(
        count_sessions_with_token(&pool, DEV_SESSION_TOKEN).await,
        0,
        "the published session token goes even when the admin row stays"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn gate_disabled_purges_rows_an_older_version_soft_deleted(pool: MySqlPool) {
    // Upgrade path: v1.18.0 and earlier soft-deleted the seeded rows,
    // leaving them recoverable from the Trash panel. Reproduce that
    // state, then check the first boot on this version clears it.
    sqlx::query(
        "UPDATE users SET deleted_at = NOW(), version = version + 1 \
          WHERE username IN ('admin', 'librarian')",
    )
    .execute(&pool)
    .await
    .expect("simulate the v1.18.0 gate having already run");
    assert_eq!(count_seeded_total(&pool).await, 2);

    let removed = seed_gate::apply_with(&pool, false)
        .await
        .expect("seed gate should succeed");

    assert_eq!(removed, 2, "rows left behind by the old gate must be purged");
    assert_eq!(count_seeded_total(&pool).await, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn gate_disabled_is_idempotent(pool: MySqlPool) {
    // First run: deletes both.
    let first = seed_gate::apply_with(&pool, false).await.unwrap();
    assert_eq!(first, 2);

    // Second run: nothing left to match. Important for prod reboots —
    // the gate still fires on every boot; it just no-ops.
    let second = seed_gate::apply_with(&pool, false).await.unwrap();
    assert_eq!(second, 0);
}
