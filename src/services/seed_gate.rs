//! Production gate against the dev seed migrations (issue #173).
//!
//! # Background
//!
//! Pre-1.1.0 mybibli shipped seed migrations that planted
//! `admin/admin` and `librarian/librarian` users on every fresh
//! install, including production. The first-launch wizard at
//! `/setup` was silently bypassed because the seed inserted an
//! active admin row before the wizard's
//! `active_admin_count == 0 AND setup_completed_at IS NULL`
//! predicate was first evaluated.
//!
//! # Why a Rust-side gate rather than splitting the migration tree
//!
//! Splitting the seed SQL files into a separate `migrations-dev-seed/`
//! directory is architecturally tidier, but it breaks every
//! `#[sqlx::test(migrations = "./migrations")]` integration test —
//! ~150 invocations across a dozen files rely on the seeded
//! `admin` / `librarian` rows being present after the test harness
//! runs migrations against a fresh database. Replicating those
//! fixtures via `sqlx::test(fixtures(...))` would mean mechanically
//! editing every affected attribute. The Rust-side gate avoids that
//! churn: the seed migrations still apply unchanged, and this
//! function neutralises their effect during the production-binary
//! boot sequence.
//!
//! # Behaviour
//!
//! Runs immediately after `sqlx::migrate!` in `main.rs`. When
//! [`MYBIBLI_SEED_DEV_USERS`](`crate::services::seed_gate`) is
//! **not** set (or set to anything other than `"1"` / `"true"` /
//! `"TRUE"`), soft-deletes any user whose `password_hash` still
//! matches the documented seed hash. The hash check protects
//! operators who have already rotated the seeded admin password —
//! their rotated row is left untouched.
//!
//! Net effect:
//!
//! * **Fresh production install** (default env): seed migrations
//!   run → this gate fires → seeded users disappear → setup wizard
//!   activates on the next request.
//! * **Dev / E2E** (`MYBIBLI_SEED_DEV_USERS=1`): gate is a no-op →
//!   `admin/admin` and `librarian/librarian` persist for the
//!   integration and Playwright test suites.
//! * **Operator who has rotated their password**: hash no longer
//!   matches → the rotated user row is left alone.

use crate::db::DbPool;

/// Exact `password_hash` value planted by
/// `migrations/20260331000004_fix_dev_user_hash.sql` for the
/// seeded `admin` row.
const ADMIN_SEED_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$4g83LVDxAaFJOYMH7jrQCA$rzWkSQWhV9koCi5hJu2BVQa9LhcZHCpvJnxNBrU1nBw";

/// Exact `password_hash` value planted by
/// `migrations/20260414000001_seed_librarian_user.sql` for the
/// seeded `librarian` row.
const LIBRARIAN_SEED_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$NfI9SYT0huhcqAanQWa9pw$mSEHLW8Wl8wlk504MRpzyS42JlcU9w2CXYVVFMFvbcU";

/// Pure parse for the `MYBIBLI_SEED_DEV_USERS` accept-set.
/// Strict: only the literal strings `1`, `true` and `TRUE` count
/// as "on". Anything else (including the empty string, `0`,
/// `false`, `True`, `yes`, whitespace-padded variants) is treated
/// as "off". Matches the convention used by `MYBIBLI_SKIP_SETUP`
/// and `MYBIBLI_SKIP_STARTUP_PURGE`.
pub fn parse_seed_dev_users(raw: Option<&str>) -> bool {
    matches!(raw, Some("1" | "true" | "TRUE"))
}

/// Apply the seed gate. See module documentation for semantics.
///
/// Reads `MYBIBLI_SEED_DEV_USERS` from the process env once, then
/// delegates to [`apply_with`]. The single env read keeps the call
/// site in `main.rs` short and lets integration tests target
/// [`apply_with`] without touching shared process state.
///
/// Returns the number of rows soft-deleted (0 when the env var
/// opts back in, or when the operator already rotated the
/// passwords).
pub async fn apply(pool: &DbPool) -> Result<u64, sqlx::Error> {
    let seed_enabled =
        parse_seed_dev_users(std::env::var("MYBIBLI_SEED_DEV_USERS").ok().as_deref());
    apply_with(pool, seed_enabled).await
}

/// Test-friendly entry point: same semantics as [`apply`] but the
/// `seed_enabled` boolean is passed in directly. Lets integration
/// tests under `#[sqlx::test]` exercise both branches without
/// mutating process env vars (which is unsafe and prone to leaking
/// between parallel test workers).
pub async fn apply_with(pool: &DbPool, seed_enabled: bool) -> Result<u64, sqlx::Error> {
    if seed_enabled {
        tracing::info!(
            "MYBIBLI_SEED_DEV_USERS=1 — dev seed gate skipped, seeded users retained"
        );
        return Ok(0);
    }

    let result = sqlx::query(
        "UPDATE users \
            SET deleted_at = NOW(), version = version + 1 \
          WHERE deleted_at IS NULL \
            AND ( (username = 'admin' AND password_hash = ?) \
               OR (username = 'librarian' AND password_hash = ?) )",
    )
    .bind(ADMIN_SEED_HASH)
    .bind(LIBRARIAN_SEED_HASH)
    .execute(pool)
    .await?;

    let removed = result.rows_affected();
    if removed > 0 {
        tracing::info!(
            removed_count = removed,
            "Issue #173 — dev seed gate removed seeded user(s). \
             The setup wizard at /setup is now reachable."
        );
    } else {
        tracing::debug!(
            "Issue #173 — dev seed gate found no seeded users to remove \
             (already rotated or never installed)."
        );
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::parse_seed_dev_users;

    #[test]
    fn accepts_strict_set() {
        assert!(parse_seed_dev_users(Some("1")));
        assert!(parse_seed_dev_users(Some("true")));
        assert!(parse_seed_dev_users(Some("TRUE")));
    }

    #[test]
    fn rejects_unset() {
        assert!(!parse_seed_dev_users(None));
    }

    #[test]
    fn rejects_empty_and_falsy() {
        assert!(!parse_seed_dev_users(Some("")));
        assert!(!parse_seed_dev_users(Some("0")));
        assert!(!parse_seed_dev_users(Some("false")));
        assert!(!parse_seed_dev_users(Some("FALSE")));
    }

    #[test]
    fn rejects_near_misses() {
        // Strictly outside the accept-set: protects against stale shell
        // values like `True` or `yes` silently flipping the gate.
        assert!(!parse_seed_dev_users(Some("True")));
        assert!(!parse_seed_dev_users(Some("TrUe")));
        assert!(!parse_seed_dev_users(Some("yes")));
        assert!(!parse_seed_dev_users(Some("YES")));
        assert!(!parse_seed_dev_users(Some("on")));
        assert!(!parse_seed_dev_users(Some("enabled")));
        assert!(!parse_seed_dev_users(Some(" 1")));
        assert!(!parse_seed_dev_users(Some("1 ")));
        assert!(!parse_seed_dev_users(Some("1\n")));
    }
}
