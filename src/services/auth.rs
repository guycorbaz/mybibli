//! Authentication service helpers.
//!
//! Currently exposes one function — `authenticate_session` — which is
//! the session-rotation chain shared by the login handler
//! (`routes/auth.rs::login`) and the first-launch setup wizard's
//! Step 1 admin creation (`routes/setup.rs::step_1_submit`).
//!
//! Both call sites need to:
//!   1. Generate a fresh session token + CSRF token.
//!   2. Insert a new `sessions` row bound to the just-authenticated
//!      `user_id`, with `last_activity = UTC_TIMESTAMP()`.
//!   3. Soft-delete the previous session row (if any) so the
//!      anonymous-session purge does not have to wait 7 days.
//!
//! The cookie set is the caller's responsibility — the helper just
//! returns the `(session_token, csrf_token)` pair so the caller can
//! decide on `SameSite`, `Max-Age`, etc.

use crate::db::DbPool;
use crate::error::AppError;
use crate::utils::{generate_csrf_token, generate_session_token};

/// Mint a fresh authenticated session for `user_id`. Returns
/// `(session_token, csrf_token)` so the caller can set the cookie and
/// embed the token in the next page render.
///
/// `previous_session_token` (the cookie the browser is currently
/// presenting, if any) is soft-deleted in the same call so the
/// anonymous-session purge does not have to wait 7 days. Pass `None`
/// when there is no prior session.
///
/// CR #39: the INSERT and the soft-delete are wrapped in a single
/// `sqlx::Transaction`. Before the fix the two queries ran as
/// independent round-trips, and a partial failure between them — DB
/// timeout, connection drop, app panic — left an orphaned live
/// anonymous session row alongside the new authenticated one. Both
/// carried valid CSRF tokens for the same browser, which is a real
/// (if low-probability) anonymity-mixing risk. The transactional
/// rewrite makes the outcome atomic: either BOTH rows transition
/// (new active, old soft-deleted) or NEITHER.
pub async fn authenticate_session(
    pool: &DbPool,
    user_id: u64,
    previous_session_token: Option<&str>,
) -> Result<(String, String), AppError> {
    let session_token = generate_session_token();
    let csrf_token = generate_csrf_token();

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO sessions (token, user_id, csrf_token, data, last_activity) \
         VALUES (?, ?, ?, '{}', UTC_TIMESTAMP())",
    )
    .bind(&session_token)
    .bind(user_id)
    .bind(&csrf_token)
    .execute(&mut *tx)
    .await?;

    if let Some(old_token) = previous_session_token
        && old_token != session_token
    {
        // Inside the same transaction now — a failure here rolls back
        // the INSERT above, so we never end up with two live session
        // rows for the same browser. The 7-day purge task is still
        // the long-term safety net for orphaned rows that escape
        // (e.g., older rows from before this fix).
        sqlx::query(
            "UPDATE sessions SET deleted_at = UTC_TIMESTAMP() \
             WHERE token = ? AND deleted_at IS NULL",
        )
        .bind(old_token)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok((session_token, csrf_token))
}

// generate_session_token + generate_csrf_token now live in src/utils.rs
// (#365) and own their length/charset/uniqueness coverage there.
