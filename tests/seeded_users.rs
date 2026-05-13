//! Integration test for story 6-2: verifies the seeded users (`admin`, `librarian`)
//! exist after running the full migration chain against a fresh DB.
//!
//! Uses `#[sqlx::test(migrations = "./migrations")]` so migrations run per-test
//! against an isolated temporary database (mirrors `tests/find_similar.rs`).
//!
//! To run locally:
//!     docker compose -f tests/docker-compose.rust-test.yml up -d
//!     DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
//!         cargo test --test seeded_users

use argon2::{Argon2, PasswordHash, PasswordVerifier};
use sqlx::MySqlPool;

#[sqlx::test(migrations = "./migrations")]
async fn admin_and_librarian_seeds_present(pool: MySqlPool) {
    // AC #1: fresh DB must contain EXACTLY two HUMAN-login-eligible
    // users, both active. The SYSTEM user (issue #68, migration
    // 20260513000001) is also present but excluded from this
    // assertion — see the dedicated `system_user_present` test below.
    let (total,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM users \
         WHERE deleted_at IS NULL AND role IN ('admin', 'librarian')",
    )
    .fetch_one(&pool)
    .await
    .expect("count users");
    assert_eq!(
        total, 2,
        "fresh DB must seed exactly two human users (admin + librarian), got {total}"
    );

    let rows: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT username, role, active FROM users \
         WHERE username IN ('admin', 'librarian') AND deleted_at IS NULL \
         ORDER BY username",
    )
    .fetch_all(&pool)
    .await
    .expect("query users");

    assert_eq!(
        rows,
        vec![
            ("admin".to_string(), "admin".to_string(), true),
            ("librarian".to_string(), "librarian".to_string(), true),
        ],
        "fresh DB must seed admin and librarian users with matching roles and active=TRUE"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn system_user_present_and_not_login_eligible(pool: MySqlPool) {
    // Issue #68: dedicated SYSTEM user for audit-trail attribution.
    // Migration 20260513000001 plants exactly one row with role='system',
    // active=FALSE, and a password_hash that is not a valid argon2
    // string. The login predicate filters role IN ('admin', 'librarian'),
    // so SYSTEM is unreachable through `POST /login` regardless.
    let rows: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT username, role, active FROM users WHERE role = 'system'",
    )
    .fetch_all(&pool)
    .await
    .expect("query system users");

    assert_eq!(rows.len(), 1, "exactly one SYSTEM user must exist");
    assert_eq!(rows[0].0, "SYSTEM");
    assert_eq!(rows[0].1, "system");
    assert!(!rows[0].2, "SYSTEM user must have active=FALSE");
}

#[sqlx::test(migrations = "./migrations")]
async fn librarian_seed_hash_validates(pool: MySqlPool) {
    let (hash,): (String,) = sqlx::query_as(
        "SELECT password_hash FROM users WHERE username = 'librarian' AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("fetch librarian hash");

    let parsed = PasswordHash::new(&hash).expect("hash parses");
    Argon2::default()
        .verify_password(b"librarian", &parsed)
        .expect("librarian/librarian must verify against the seeded hash");
}
