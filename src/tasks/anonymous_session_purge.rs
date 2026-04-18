//! Daily purge task for anonymous session rows (story 8-2).
//!
//! The session resolver middleware mints an anonymous session row on the
//! first hit from a browser with no `session` cookie. Over time these
//! rows accumulate — crawlers, drive-by scans, people who visit once
//! and never return. Left unbounded the `sessions` table grows forever
//! for no user benefit.
//!
//! This task runs once every 24h and deletes anonymous rows whose
//! `last_activity` is older than 7 days. Anonymous visitors who return
//! after a week simply get a fresh row on their next request — no
//! user-visible impact.
//!
//! Authenticated sessions are NOT affected: they carry `user_id IS NOT
//! NULL` and are already managed by the session-timeout soft-delete
//! path (story 7-2).
//!
//! GDPR posture: anonymous rows carry `user_id = NULL`, a random session
//! token, a random CSRF token, and timestamps. No PII. The 7-day
//! retention window is the narrowest span that keeps CSRF-token
//! continuity across anonymous POSTs (rare but possible — e.g. an
//! anonymous visitor submitting the language toggle).

use std::time::Duration;

use crate::db::DbPool;

const PURGE_INTERVAL_SECS: u64 = 86_400; // 24 h
/// Delay before the first purge (spec §Task 4.3: first run 24 h after boot).
const INITIAL_DELAY_SECS: u64 = 86_400; // 24 h
const RETENTION_DAYS: u64 = 7;

/// Spawn the daily purge task. Swallows all errors — maintenance must
/// never crash the app. Call from `main.rs` once per process.
pub fn spawn(pool: DbPool) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;
        let mut interval = tokio::time::interval(Duration::from_secs(PURGE_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            purge_once(&pool).await;
        }
    });
}

/// Run one purge round. Exposed for integration tests.
pub async fn purge_once(pool: &DbPool) -> u64 {
    let retention_days = RETENTION_DAYS as i64;
    match sqlx::query(
        "DELETE FROM sessions WHERE user_id IS NULL \
         AND last_activity < UTC_TIMESTAMP() - INTERVAL ? DAY",
    )
    .bind(retention_days)
    .execute(pool)
    .await
    {
        Ok(result) => {
            let rows = result.rows_affected();
            if rows > 0 {
                tracing::info!(
                    rows_deleted = rows,
                    "anonymous session purge completed"
                );
            }
            rows
        }
        Err(err) => {
            tracing::warn!(error = %err, "anonymous session purge failed");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    #[sqlx::test(migrations = "./migrations")]
    async fn purges_old_anonymous_rows_only(pool: DbPool) {
        // Seed: 3 old-anonymous + 2 recent-anonymous + 1 old-authenticated.
        // Only the 3 old-anonymous rows should be deleted.
        //
        // Clear the sessions table first — the dev-user seed migration
        // inserts a baseline authenticated session that would otherwise
        // skew the authenticated-row count at the end of the test.
        sqlx::query("DELETE FROM sessions")
            .execute(&pool)
            .await
            .unwrap();

        let old_activity = chrono::Utc::now() - ChronoDuration::days(8);
        let recent_activity = chrono::Utc::now() - ChronoDuration::days(3);

        // Tokens are 44-char base64 per generate_session_token — match width.
        for i in 0..3 {
            sqlx::query(
                "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
                 VALUES (?, NULL, ?, '{}', ?)",
            )
            .bind(format!("OLDANON{i:02}AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA0"))
            .bind(format!("csrf{i:02}"))
            .bind(old_activity)
            .execute(&pool)
            .await
            .unwrap();
        }
        for i in 0..2 {
            sqlx::query(
                "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
                 VALUES (?, NULL, ?, '{}', ?)",
            )
            .bind(format!("NEWANON{i:02}AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA0"))
            .bind(format!("csrf2{i:02}"))
            .bind(recent_activity)
            .execute(&pool)
            .await
            .unwrap();
        }
        // Seed an authenticated row with old activity — must NOT be purged.
        sqlx::query(
            "INSERT INTO users (username, password_hash, role) \
             VALUES ('purge_test_user', '$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU', 'librarian')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let user_id: u64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'purge_test_user'")
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
             VALUES (?, ?, ?, '{}', ?)",
        )
        .bind("AUTHSESSIONaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .bind(user_id)
        .bind("auth-csrf-token")
        .bind(old_activity)
        .execute(&pool)
        .await
        .unwrap();

        let deleted = purge_once(&pool).await;
        assert_eq!(deleted, 3, "should have deleted exactly the 3 old anonymous rows");

        let remaining_anon: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE user_id IS NULL AND deleted_at IS NULL")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining_anon, 2, "2 recent anonymous rows should remain");

        let remaining_auth: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sessions WHERE user_id IS NOT NULL AND deleted_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            remaining_auth, 1,
            "authenticated session must not be purged even if old"
        );
    }
}
