//! CR #459 — `MYBIBLI_RESET_ADMIN` one-shot startup hatch.
//!
//! Resets a locked-out administrator's password to a freshly generated
//! random string, invalidates every live session for that account, and
//! lets `main()` log the credentials and refuse to start. The refusal is
//! what makes the hatch one-shot: a variable forgotten in
//! `docker-compose.yml` re-randomises the password on every boot instead
//! of silently pinning a known-password account.
//!
//! The generated password lands in the log, which is readable by anyone
//! with the volume. Accepted trade — the operator already has compose-file
//! and database access, so the hatch grants nothing they did not have
//! (documented in `docs/auth-threat-model.md`).

use crate::db::DbPool;
use crate::error::AppError;
use crate::services::password::hash_password;

/// Bytes of entropy behind the generated recovery password. 18 bytes →
/// 24 base64 chars, 144 bits — far beyond any online-guessing budget.
const RECOVERY_PASSWORD_BYTES: usize = 18;

/// Outcome of a successful reset, for `main()` to log before exiting.
/// Derived `Debug` prints the plain-text password — acceptable, that is
/// this type's entire purpose (it exists to be logged once).
#[derive(Debug)]
pub struct ResetOutcome {
    pub user_id: u64,
    pub username: String,
    /// The freshly generated plain-text password. Logged once at WARN;
    /// never stored anywhere else in plain text.
    pub password: String,
    pub sessions_killed: u64,
}

/// Strict accept-set for the env var, per R3-N6. The value is a username,
/// so the rule becomes: empty or whitespace-only is NOT a request to
/// reset. Surrounding whitespace is trimmed (compose files and shells
/// make trailing spaces easy to introduce and hard to see).
pub fn parse_reset_request(raw: Option<String>) -> Option<String> {
    let trimmed = raw?.trim().to_string();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

/// Generate the random recovery password: URL-safe base64 (no padding)
/// over `RECOVERY_PASSWORD_BYTES` random bytes. URL-safe so the operator
/// can paste it anywhere without shell-quoting surprises (`+` and `/`
/// never appear; the alphabet is `[A-Za-z0-9_-]`).
pub fn generate_recovery_password() -> String {
    use base64::Engine;
    use rand::RngCore;

    let mut bytes = [0u8; RECOVERY_PASSWORD_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Reset the named administrator's password and invalidate their
/// sessions, in one transaction.
///
/// Refuses (distinct operator-facing messages, no reset performed) when
/// the username is unknown, the account is deactivated, or the account is
/// not an admin — the hatch is for the "sole admin locked out" incident,
/// not a general password-reset tool; any other account is reset from
/// `/admin` once the admin is back in.
pub async fn reset_admin_password(
    pool: &DbPool,
    username: &str,
) -> Result<ResetOutcome, AppError> {
    let mut tx = pool.begin().await?;

    // `deleted_at IS NULL` projected as an integer flag — dynamic-query
    // SQLx cannot decode a raw TIMESTAMP column (CLAUDE.md MariaDB type
    // gotcha #4) and we only need liveness, not the timestamp itself.
    let row: Option<(u64, String, i64)> = sqlx::query_as(
        "SELECT id, role, (deleted_at IS NULL) AS active \
         FROM users WHERE username = ? FOR UPDATE",
    )
    .bind(username)
    .fetch_optional(&mut *tx)
    .await?;

    let Some((user_id, role, active)) = row else {
        return Err(AppError::BadRequest(format!(
            "MYBIBLI_RESET_ADMIN: no user named '{username}' exists"
        )));
    };
    if active == 0 {
        return Err(AppError::BadRequest(format!(
            "MYBIBLI_RESET_ADMIN: user '{username}' is deactivated — reactivate it first or name an active admin"
        )));
    }
    if role != "admin" {
        return Err(AppError::BadRequest(format!(
            "MYBIBLI_RESET_ADMIN: user '{username}' has role '{role}', not 'admin' — the hatch only resets administrators"
        )));
    }

    let password = generate_recovery_password();
    let new_hash = hash_password(&password)?;

    // No optimistic-lock predicate: the FOR UPDATE row lock above already
    // serialises this write against any concurrent update.
    sqlx::query(
        "UPDATE users SET password_hash = ?, version = version + 1, updated_at = NOW() \
         WHERE id = ?",
    )
    .bind(&new_hash)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    // Same invalidation as story 8-3 deactivation: soft-delete every live
    // session so nothing minted under the old password survives the reset.
    let sessions_killed = sqlx::query(
        "UPDATE sessions SET deleted_at = NOW() WHERE user_id = ? AND deleted_at IS NULL",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    tx.commit().await?;

    // Best-effort forensics row attributed to the SYSTEM user (issue #68
    // pattern) — the reset already committed, so an audit failure only
    // degrades to a log warning, never a rollback.
    match crate::models::user::UserModel::find_system_user_id(pool).await {
        Ok(system_id) => {
            if let Err(e) = crate::models::admin_audit::AdminAuditModel::create(
                pool,
                system_id,
                "admin_password_reset_hatch",
                Some("user"),
                Some(user_id),
                Some(serde_json::json!({
                    "username": username,
                    "sessions_killed": sessions_killed,
                })),
            )
            .await
            {
                tracing::warn!(error = %e, "Reset hatch: audit row insert failed (reset already applied)");
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Reset hatch: SYSTEM user not found, skipping audit row (reset already applied)");
        }
    }

    Ok(ResetOutcome {
        user_id,
        username: username.to_string(),
        password,
        sessions_killed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejects_unset_empty_and_whitespace() {
        assert_eq!(parse_reset_request(None), None);
        assert_eq!(parse_reset_request(Some(String::new())), None);
        assert_eq!(parse_reset_request(Some("   ".to_string())), None);
        assert_eq!(parse_reset_request(Some("\t\n".to_string())), None);
    }

    #[test]
    fn parse_trims_surrounding_whitespace() {
        assert_eq!(
            parse_reset_request(Some("  alice \n".to_string())),
            Some("alice".to_string())
        );
        assert_eq!(
            parse_reset_request(Some("bob".to_string())),
            Some("bob".to_string())
        );
    }

    #[test]
    fn recovery_password_is_24_urlsafe_chars() {
        let pw = generate_recovery_password();
        assert_eq!(pw.len(), 24);
        assert!(
            pw.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "unexpected character in {pw:?}"
        );
    }

    #[test]
    fn recovery_passwords_are_not_repeated() {
        assert_ne!(generate_recovery_password(), generate_recovery_password());
    }
}
