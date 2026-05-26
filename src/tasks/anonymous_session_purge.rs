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

use chrono::{DateTime, Utc};

use crate::db::DbPool;

const PURGE_INTERVAL_SECS: u64 = 86_400; // 24 h
/// Delay before the first purge on a fresh install (no prior `last_run` in DB).
/// Spec §Task 4.3: first run 24 h after boot. Crash-loop resilience (#46)
/// is achieved by persisting `last_anonymous_session_purge_at` after each
/// run and computing the next-run delay from it, NOT by shortening this.
const INITIAL_DELAY_SECS: u64 = 86_400; // 24 h
const RETENTION_DAYS: u64 = 7;

/// K/V settings row that stores the RFC3339 UTC timestamp of the last
/// successful purge. Empty on fresh install → falls back to the spec
/// default (24h initial delay). See migration 20260526075700.
const KEY_LAST_PURGE_AT: &str = "last_anonymous_session_purge_at";

/// Spawn the daily purge task. Swallows all errors — maintenance must
/// never crash the app. Call from `main.rs` once per process.
///
/// Fix #46 (v1.7.9): the initial delay is now computed from the persisted
/// `last_anonymous_session_purge_at` setting. If the app crash-loops
/// within a 24 h window, the next boot sees `NOW - last >= 24h`
/// (eventually) and triggers an immediate catch-up purge instead of
/// re-arming a fresh 24 h sleep that resets on every restart. If
/// the row is missing or empty (fresh install), the spec default
/// 24 h sleep is preserved.
pub fn spawn(pool: DbPool) {
    tokio::spawn(async move {
        let initial_delay = compute_initial_delay(&pool).await;
        tracing::info!(
            initial_delay_secs = initial_delay.as_secs(),
            "anonymous session purge: scheduled first run"
        );
        tokio::time::sleep(initial_delay).await;
        loop {
            purge_once(&pool).await;
            record_purge_at(&pool, Utc::now()).await;
            tokio::time::sleep(Duration::from_secs(PURGE_INTERVAL_SECS)).await;
        }
    });
}

/// Decide the sleep duration before the first purge of this process.
///   * row missing / empty / unparseable → spec default (`INITIAL_DELAY_SECS`).
///   * persisted `last_run` >= 24 h ago → `Duration::ZERO` (catch-up).
///   * otherwise → `24h - elapsed`.
async fn compute_initial_delay(pool: &DbPool) -> Duration {
    let last = read_last_purge_at(pool).await;
    let Some(last) = last else {
        return Duration::from_secs(INITIAL_DELAY_SECS);
    };
    let elapsed = (Utc::now() - last).num_seconds();
    if elapsed < 0 {
        // Clock skew / row written in the future — be conservative,
        // fall back to the spec default rather than purging immediately.
        return Duration::from_secs(INITIAL_DELAY_SECS);
    }
    let elapsed = elapsed as u64;
    if elapsed >= PURGE_INTERVAL_SECS {
        Duration::ZERO
    } else {
        Duration::from_secs(PURGE_INTERVAL_SECS - elapsed)
    }
}

async fn read_last_purge_at(pool: &DbPool) -> Option<DateTime<Utc>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT setting_value FROM settings \
         WHERE setting_key = ? AND deleted_at IS NULL",
    )
    .bind(KEY_LAST_PURGE_AT)
    .fetch_optional(pool)
    .await
    .ok()?;
    let value = row?.0;
    if value.trim().is_empty() {
        return None;
    }
    match DateTime::parse_from_rfc3339(value.trim()) {
        Ok(dt) => Some(dt.with_timezone(&Utc)),
        Err(e) => {
            tracing::warn!(
                value = %value,
                error = %e,
                "last_anonymous_session_purge_at: malformed RFC3339, treating as absent"
            );
            None
        }
    }
}

async fn record_purge_at(pool: &DbPool, when: DateTime<Utc>) {
    let value = when.to_rfc3339();
    if let Err(e) = sqlx::query(
        "UPDATE settings SET setting_value = ?, version = version + 1 \
         WHERE setting_key = ? AND deleted_at IS NULL",
    )
    .bind(&value)
    .bind(KEY_LAST_PURGE_AT)
    .execute(pool)
    .await
    {
        tracing::warn!(
            error = %e,
            "anonymous session purge: failed to persist last_run timestamp; crash-loop guard degraded"
        );
    }
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

/// Pure helper exposed for unit testing the crash-loop guard math. Given the
/// last-run timestamp (or `None` for a fresh install) and the current time,
/// returns the duration to sleep before the first purge.
#[cfg(test)]
fn initial_delay_from(last: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Duration {
    let Some(last) = last else {
        return Duration::from_secs(INITIAL_DELAY_SECS);
    };
    let elapsed = (now - last).num_seconds();
    if elapsed < 0 {
        return Duration::from_secs(INITIAL_DELAY_SECS);
    }
    let elapsed = elapsed as u64;
    if elapsed >= PURGE_INTERVAL_SECS {
        Duration::ZERO
    } else {
        Duration::from_secs(PURGE_INTERVAL_SECS - elapsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    // ─── Fix #46 (v1.7.9) — initial_delay_from semantics ─────────

    #[test]
    fn initial_delay_is_spec_default_when_no_prior_run() {
        let now = Utc::now();
        assert_eq!(
            initial_delay_from(None, now),
            Duration::from_secs(INITIAL_DELAY_SECS)
        );
    }

    #[test]
    fn initial_delay_is_zero_when_24h_already_elapsed() {
        let now = Utc::now();
        let last = now - ChronoDuration::hours(25);
        assert_eq!(initial_delay_from(Some(last), now), Duration::ZERO);
    }

    #[test]
    fn initial_delay_is_zero_at_exact_24h_boundary() {
        let now = Utc::now();
        let last = now - ChronoDuration::hours(24);
        assert_eq!(initial_delay_from(Some(last), now), Duration::ZERO);
    }

    #[test]
    fn initial_delay_is_remainder_when_within_24h() {
        let now = Utc::now();
        let last = now - ChronoDuration::hours(10);
        // 24h - 10h = 14h
        assert_eq!(
            initial_delay_from(Some(last), now),
            Duration::from_secs(14 * 3600)
        );
    }

    #[test]
    fn initial_delay_is_spec_default_on_clock_skew_future_timestamp() {
        // Row written in the "future" relative to NOW — clock skew between
        // hosts or NTP correction. Fall back to the spec default rather
        // than purging immediately.
        let now = Utc::now();
        let last = now + ChronoDuration::hours(2);
        assert_eq!(
            initial_delay_from(Some(last), now),
            Duration::from_secs(INITIAL_DELAY_SECS)
        );
    }

    // ─── Fix #46 (v1.7.9) — persisted last-run round-trip ───────

    #[sqlx::test(migrations = "./migrations")]
    async fn read_last_purge_at_returns_none_on_empty_seeded_row(pool: DbPool) {
        // Migration 20260526075700 seeds the row with an empty string.
        assert!(read_last_purge_at(&pool).await.is_none());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn record_purge_at_then_read_round_trips(pool: DbPool) {
        let when = Utc::now();
        record_purge_at(&pool, when).await;
        let read_back = read_last_purge_at(&pool).await.expect("row should parse");
        // RFC3339 round-trip is lossy at the sub-second level depending on
        // the serializer; assert second-precision equality.
        assert_eq!(read_back.timestamp(), when.timestamp());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn read_last_purge_at_returns_none_on_malformed_row(pool: DbPool) {
        sqlx::query(
            "UPDATE settings SET setting_value = 'not-a-timestamp' \
             WHERE setting_key = 'last_anonymous_session_purge_at'",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Malformed → treated as absent (safe — falls back to spec default
        // 24h delay rather than running immediately on bogus data).
        assert!(read_last_purge_at(&pool).await.is_none());
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn compute_initial_delay_uses_persisted_row(pool: DbPool) {
        // Seed last-run 25h ago — boot should schedule immediate catch-up.
        let stale = Utc::now() - ChronoDuration::hours(25);
        record_purge_at(&pool, stale).await;
        let delay = compute_initial_delay(&pool).await;
        assert_eq!(
            delay,
            Duration::ZERO,
            "25h-old persisted row triggers immediate catch-up"
        );
    }

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
