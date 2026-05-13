//! Integration test for issue #173: production gate against the dev
//! seed migrations.
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

use mybibli::services::seed_gate;
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
async fn count_seeded_total(pool: &MySqlPool) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE username IN ('admin', 'librarian')",
    )
    .fetch_one(pool)
    .await
    .expect("count all seeded users")
}

#[sqlx::test(migrations = "./migrations")]
async fn gate_disabled_soft_deletes_both_seeded_users(pool: MySqlPool) {
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

    // Post-condition: both rows soft-deleted, both still physically present.
    assert_eq!(removed, 2, "two seeded users should have been soft-deleted");
    assert_eq!(count_seeded_active(&pool).await, 0);
    assert_eq!(count_seeded_total(&pool).await, 2);
}

#[sqlx::test(migrations = "./migrations")]
async fn gate_enabled_preserves_seeded_users(pool: MySqlPool) {
    // Pre-condition: fresh seed.
    assert_eq!(count_seeded_active(&pool).await, 2);

    // Act: env set to true (dev / E2E branch).
    let removed = seed_gate::apply_with(&pool, true)
        .await
        .expect("seed gate should succeed");

    // Post-condition: both rows still active.
    assert_eq!(removed, 0, "no rows should be touched when seed gate is opt-in");
    assert_eq!(count_seeded_active(&pool).await, 2);
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
    // is left intact.
    assert_eq!(removed, 1, "only the un-rotated librarian should be soft-deleted");

    let rows: Vec<(String, Option<chrono::NaiveDateTime>)> = sqlx::query_as(
        "SELECT username, CAST(deleted_at AS DATETIME) AS deleted_at \
           FROM users \
          WHERE username IN ('admin', 'librarian') \
          ORDER BY username",
    )
    .fetch_all(&pool)
    .await
    .expect("read seeded user state");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, "admin");
    assert!(rows[0].1.is_none(), "rotated admin should NOT be soft-deleted");
    assert_eq!(rows[1].0, "librarian");
    assert!(rows[1].1.is_some(), "un-rotated librarian should be soft-deleted");
}

#[sqlx::test(migrations = "./migrations")]
async fn gate_disabled_is_idempotent(pool: MySqlPool) {
    // First run: soft-deletes both.
    let first = seed_gate::apply_with(&pool, false).await.unwrap();
    assert_eq!(first, 2);

    // Second run: no rows match the `deleted_at IS NULL` predicate any
    // more, so no work to do. Important for prod reboots — operators
    // who don't rotate after the wizard runs would otherwise still see
    // the gate fire on every boot. (It still fires; it just no-ops.)
    let second = seed_gate::apply_with(&pool, false).await.unwrap();
    assert_eq!(second, 0);
}
