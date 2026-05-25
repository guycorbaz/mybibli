//! CR #39 — `services::auth::authenticate_session` transactional contract.
//!
//! Before the fix the function ran two independent queries (INSERT new
//! session, then UPDATE prior anonymous session's `deleted_at`). A partial
//! failure between them — DB timeout, connection drop, app panic — left an
//! orphaned live anonymous session row alongside the new authenticated
//! one. Both carried valid CSRF tokens for the same browser, which is a
//! real (if low-probability) anonymity-mixing risk.
//!
//! The fix wraps both queries in a single `sqlx::Transaction`. These
//! tests pin the observable contract: after every successful call, the
//! DB reaches one of exactly two end-states (with-prior or no-prior),
//! never the mixed "both live" state the bug produced.
//!
//! Failure injection (the stronger half of the original acceptance) is
//! covered by code-review of the implementation: the inlined UPDATE
//! propagates errors via `?`, and `tx.commit()` happens AFTER both
//! statements succeeded. The transactional contract is what makes the
//! "both live" outcome unreachable.

use mybibli::db::DbPool;
use mybibli::services::auth::authenticate_session;
use sqlx::Row;

async fn seed_user(pool: &DbPool, username: &str) -> u64 {
    let result = sqlx::query(
        "INSERT INTO users (username, password_hash, role) \
         VALUES (?, '$argon2id$v=19$m=19456,t=2,p=1$saltsaltsalt$hashash', 'admin')",
    )
    .bind(username)
    .execute(pool)
    .await
    .expect("seed user");
    result.last_insert_id()
}

async fn seed_anonymous_session(pool: &DbPool, token: &str) -> () {
    sqlx::query(
        "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
         VALUES (?, NULL, 'anon-csrf', '{}', UTC_TIMESTAMP())",
    )
    .bind(token)
    .execute(pool)
    .await
    .expect("seed anon session");
}

async fn count_live_sessions_for_user(pool: &DbPool, user_id: u64) -> i64 {
    sqlx::query("SELECT COUNT(*) AS c FROM sessions WHERE user_id = ? AND deleted_at IS NULL")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("count live sessions")
        .try_get::<i64, _>("c")
        .expect("count column")
}

async fn count_live_sessions_for_token(pool: &DbPool, token: &str) -> i64 {
    sqlx::query("SELECT COUNT(*) AS c FROM sessions WHERE token = ? AND deleted_at IS NULL")
        .bind(token)
        .fetch_one(pool)
        .await
        .expect("count live sessions for token")
        .try_get::<i64, _>("c")
        .expect("count column")
}

/// Happy path with a prior anonymous session: the INSERT and the UPDATE
/// commit atomically. After the call, the new authenticated row is live
/// and the prior anonymous row is soft-deleted — exactly one session
/// for this browser, no "both live" mixed state.
#[sqlx::test(migrations = "./migrations")]
async fn authenticate_session_with_prior_anon_atomically_swaps(pool: DbPool) {
    // Soft-delete the seeded dev users so user-id assignments are
    // predictable across test runs.
    sqlx::query("UPDATE users SET deleted_at = NOW() WHERE deleted_at IS NULL")
        .execute(&pool)
        .await
        .unwrap();

    let user_id = seed_user(&pool, "auth_svc_user_with_prior").await;
    let old_token = "anon-session-token-AAAAAAAAAAAAAAAAAAAAAA";
    seed_anonymous_session(&pool, old_token).await;

    let (new_token, _csrf) = authenticate_session(&pool, user_id, Some(old_token))
        .await
        .expect("authenticate_session");

    // Exactly one live session for the user: the new one.
    assert_eq!(count_live_sessions_for_user(&pool, user_id).await, 1);
    // The new token is alive.
    assert_eq!(count_live_sessions_for_token(&pool, &new_token).await, 1);
    // The old anonymous token has been soft-deleted (no live rows).
    assert_eq!(count_live_sessions_for_token(&pool, old_token).await, 0);
}

/// No prior session: the UPDATE branch is skipped entirely, but the
/// INSERT still runs and commits.
#[sqlx::test(migrations = "./migrations")]
async fn authenticate_session_without_prior_creates_single_row(pool: DbPool) {
    sqlx::query("UPDATE users SET deleted_at = NOW() WHERE deleted_at IS NULL")
        .execute(&pool)
        .await
        .unwrap();

    let user_id = seed_user(&pool, "auth_svc_user_no_prior").await;

    let (new_token, _csrf) = authenticate_session(&pool, user_id, None)
        .await
        .expect("authenticate_session");

    assert_eq!(count_live_sessions_for_user(&pool, user_id).await, 1);
    assert_eq!(count_live_sessions_for_token(&pool, &new_token).await, 1);
}

/// Repeated login from the same browser: each successive call soft-
/// deletes the previous session's row. End state: only the latest
/// session is live — no fan-out of orphaned rows.
#[sqlx::test(migrations = "./migrations")]
async fn authenticate_session_consecutive_calls_keep_only_latest_live(pool: DbPool) {
    sqlx::query("UPDATE users SET deleted_at = NOW() WHERE deleted_at IS NULL")
        .execute(&pool)
        .await
        .unwrap();

    let user_id = seed_user(&pool, "auth_svc_user_chain").await;

    let (token_a, _) = authenticate_session(&pool, user_id, None).await.unwrap();
    let (token_b, _) = authenticate_session(&pool, user_id, Some(&token_a)).await.unwrap();
    let (token_c, _) = authenticate_session(&pool, user_id, Some(&token_b)).await.unwrap();

    assert_eq!(count_live_sessions_for_user(&pool, user_id).await, 1);
    assert_eq!(count_live_sessions_for_token(&pool, &token_a).await, 0);
    assert_eq!(count_live_sessions_for_token(&pool, &token_b).await, 0);
    assert_eq!(count_live_sessions_for_token(&pool, &token_c).await, 1);
}
