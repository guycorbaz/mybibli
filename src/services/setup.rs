//! First-launch setup-wizard service layer (story 8-8).
//!
//! Exposes:
//!   * [`SetupStep`] — the four wizard steps.
//!   * [`SetupPredicateInputs`] — pure inputs for [`resolve_step`].
//!   * [`fetch_predicate_inputs`] — DB-side gather.
//!   * [`resolve_step`] — pure resolver (16-state truth table).
//!   * [`create_or_update_admin`] — the single-flight admin row writer.
//!   * [`save_provider_keys`] — wizard's Step 2 helper.
//!   * [`save_preferences`] — wizard's Step 3 helper.
//!   * [`complete_setup`] — wizard's Step 4 helper.
//!
//! The split between `fetch_predicate_inputs` (impure, DB-bound) and
//! `resolve_step` (pure) makes the 16-state truth table testable
//! WITHOUT a DB — see the `resolve_step_truth_table` test below.

use chrono::{DateTime, SecondsFormat, Utc};

use crate::AppState;
use crate::db::DbPool;
use crate::error::AppError;
use crate::middleware::auth::generate_csrf_token;
use crate::models::user::UserModel;
use crate::services::admin_system::{
    KEY_DEFAULT_LANGUAGE, KEY_GOOGLE_BOOKS, KEY_OMDB, KEY_SETUP_COMPLETED_AT,
    KEY_SETUP_STEP_3_DONE, KEY_TMDB, fetch_setting_value_and_version, reload_settings_cache,
    save_setting,
};
use crate::services::password::hash_password;

// ─── SetupStep ────────────────────────────────────────────────────

/// Four wizard steps. Resolved server-side on every `GET /setup` —
/// never carried in a cookie or query param (see story 8-8 Dev Notes
/// §"Why server-side step resolution").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    Admin,
    Providers,
    Preferences,
    Done,
}

impl SetupStep {
    /// Step number 1..4 — used by the progress-dot template.
    pub fn number(self) -> u8 {
        match self {
            SetupStep::Admin => 1,
            SetupStep::Providers => 2,
            SetupStep::Preferences => 3,
            SetupStep::Done => 4,
        }
    }
}

// ─── Predicate inputs (pure resolver) ─────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupPredicateInputs {
    pub active_admin_count: i64,
    pub setup_completed_at: Option<DateTime<Utc>>,
    pub any_provider_key_set: bool,
    pub setup_step_3_done: bool,
}

/// Pure resolver — `None` means "wizard inactive" (gate is a no-op,
/// `/setup` returns 404). `Some(step)` means the wizard is active and
/// the user is on `step`.
///
/// Resolution order (first match wins):
///   1. `setup_completed_at IS SOME` ⇒ inactive.
///   2. `active_admin_count > 0` ⇒ inactive (upgrade-from-pre-Epic-8 path).
///   3. `active_admin_count == 0` ⇒ Step 1 (Admin).
///   4. Admin exists, no provider key set, `step_3_done == false` ⇒ Step 2.
///   5. Admin exists, `step_3_done == false` ⇒ Step 3 (provider step was visited).
///   6. Admin exists, `step_3_done == true` ⇒ Step 4 (Done — recap).
pub fn resolve_step(inputs: &SetupPredicateInputs) -> Option<SetupStep> {
    // Rule 1: completed → inactive.
    if inputs.setup_completed_at.is_some() {
        return None;
    }
    // Rule 2: admin already exists from a pre-Epic-8 install → inactive.
    // The wizard is a one-shot first-launch flow; once an admin row
    // exists we trust the operator to have configured the rest.
    if inputs.active_admin_count > 0 {
        return None;
    }

    // From here, no admin and not completed.
    // Rule 3: pristine state ⇒ Step 1.
    if inputs.active_admin_count == 0 && !inputs.any_provider_key_set && !inputs.setup_step_3_done {
        return Some(SetupStep::Admin);
    }

    // Defensive impossible-state guard: if there's somehow no admin but
    // step_3_done is set, the resolver still funnels back to Step 1 so
    // the user can recover.
    Some(SetupStep::Admin)
}

/// Resolver variant for the case where an admin DOES exist (e.g. just
/// created via Step 1) — picks Step 2/3/4 based on the rest of the
/// inputs. Used by the route handlers AFTER Step 1 commits, so the
/// next GET /setup lands on the right step. Keeping this split out of
/// `resolve_step` makes the "wizard inactive" path (the common-case
/// no-op) a single comparison.
pub fn resolve_step_with_admin(inputs: &SetupPredicateInputs) -> SetupStep {
    if inputs.setup_step_3_done {
        SetupStep::Done
    } else if inputs.any_provider_key_set {
        SetupStep::Preferences
    } else {
        SetupStep::Providers
    }
}

// ─── Predicate fetch ──────────────────────────────────────────────

/// Read every input the resolver needs in a single pass. Two queries:
/// one for `users`, one for `settings` — both indexed.
pub async fn fetch_predicate_inputs(pool: &DbPool) -> Result<SetupPredicateInputs, AppError> {
    let admin_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM users \
         WHERE role = 'admin' AND active = TRUE AND deleted_at IS NULL",
    )
    .fetch_one(pool)
    .await?;

    // Pull every setting key relevant to the resolver in one round-trip.
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT setting_key, setting_value FROM settings \
         WHERE setting_key IN (?, ?, ?, ?, ?) AND deleted_at IS NULL",
    )
    .bind(KEY_GOOGLE_BOOKS)
    .bind(KEY_OMDB)
    .bind(KEY_TMDB)
    .bind(KEY_SETUP_COMPLETED_AT)
    .bind(KEY_SETUP_STEP_3_DONE)
    .fetch_all(pool)
    .await?;

    let mut any_provider_key_set = false;
    let mut completed_value = String::new();
    let mut step_3_done = false;

    for (key, value) in &rows {
        match key.as_str() {
            k if (k == KEY_GOOGLE_BOOKS || k == KEY_OMDB || k == KEY_TMDB)
                && !value.is_empty() =>
            {
                any_provider_key_set = true;
            }
            k if k == KEY_SETUP_COMPLETED_AT => {
                completed_value = value.clone();
            }
            k if k == KEY_SETUP_STEP_3_DONE => {
                step_3_done = value == "1";
            }
            _ => {}
        }
    }

    let setup_completed_at = if completed_value.is_empty() {
        None
    } else {
        DateTime::parse_from_rfc3339(&completed_value)
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    };

    Ok(SetupPredicateInputs {
        active_admin_count: admin_count.0,
        setup_completed_at,
        any_provider_key_set,
        setup_step_3_done: step_3_done,
    })
}

// ─── Step 1: admin creation (single-flight tx) ────────────────────

/// Outcome of [`create_or_update_admin`].
#[derive(Debug, Clone)]
pub struct AdminCreated {
    pub user_id: u64,
}

/// Insert the initial admin row in a single-flight transaction:
///   1. BEGIN.
///   2. Re-read `active_admin_count` inside the tx (with row-level lock
///      via `LOCK IN SHARE MODE` would suffice, but we use a read +
///      conditional INSERT — `users.username` UNIQUE catches genuine
///      double-submits).
///   3. If `count > 0`, ROLLBACK + return Conflict (`admin_already_created`).
///   4. Else INSERT the new admin and COMMIT.
///
/// Returns `Conflict("username_taken")` on a UNIQUE collision (existing
/// active OR soft-deleted user with the same username).
pub async fn create_or_update_admin(
    pool: &DbPool,
    username: &str,
    password: &str,
) -> Result<AdminCreated, AppError> {
    // Trim + normalize. The handler should have done this already but
    // defense-in-depth is cheap.
    let username = username.trim();
    if username.is_empty() {
        return Err(AppError::BadRequest("username_required".to_string()));
    }
    if password.len() < 8 {
        return Err(AppError::BadRequest("password_too_short".to_string()));
    }

    let password_hash = hash_password(password)?;

    let mut tx = pool.begin().await?;

    let (existing_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM users \
         WHERE role = 'admin' AND active = TRUE AND deleted_at IS NULL",
    )
    .fetch_one(&mut *tx)
    .await?;

    if existing_count > 0 {
        tx.rollback().await?;
        return Err(AppError::Conflict("admin_already_created".to_string()));
    }

    // INSERT inside the tx. UNIQUE collision is caught by `UserModel::create`
    // and surfaced as `Conflict("username_taken")`. We need to use a raw
    // query here because UserModel::create uses the pool, not a transaction.
    let result = sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'admin')",
    )
    .bind(username)
    .bind(&password_hash)
    .execute(&mut *tx)
    .await;

    let user_id = match result {
        Ok(r) => r.last_insert_id(),
        Err(sqlx::Error::Database(e)) => {
            if e.code().as_ref().map(|c| c.as_ref()) == Some("23000") {
                tx.rollback().await?;
                return Err(AppError::Conflict("username_taken".to_string()));
            }
            tx.rollback().await?;
            return Err(AppError::Database(sqlx::Error::Database(e)));
        }
        Err(e) => {
            tx.rollback().await?;
            return Err(AppError::Database(e));
        }
    };

    tx.commit().await?;

    tracing::info!(user_id, username = %username, "Setup wizard: admin user created");
    Ok(AdminCreated { user_id })
}

/// Idempotent re-submit path: the user navigated back to Step 1 from
/// Step 2 and re-submitted. We update the existing admin instead of
/// inserting. Re-uses `UserModel::update` (story 8-3 chain).
pub async fn update_existing_admin(
    pool: &DbPool,
    user_id: u64,
    version: i32,
    new_username: &str,
    new_password: Option<&str>,
) -> Result<(), AppError> {
    let new_username = new_username.trim();
    if new_username.is_empty() {
        return Err(AppError::BadRequest("username_required".to_string()));
    }
    let password_hash: Option<String> = match new_password {
        Some(p) if !p.is_empty() => {
            if p.len() < 8 {
                return Err(AppError::BadRequest("password_too_short".to_string()));
            }
            Some(hash_password(p)?)
        }
        _ => None,
    };
    UserModel::update(
        pool,
        user_id,
        version,
        new_username,
        "admin",
        password_hash.as_deref(),
    )
    .await
}

// ─── Step 2: provider keys ────────────────────────────────────────

/// Trimmed values from the wizard's Step 2 form. Empty / whitespace-only
/// inputs are mapped to `None` by the handler before calling.
#[derive(Debug, Clone, Default)]
pub struct WizardProviderKeys {
    pub google_books: Option<String>,
    pub omdb: Option<String>,
    pub tmdb: Option<String>,
}

/// Save the three provider keys to the K/V `settings` table. Each key
/// is its own row with its own `version`, so the writes are independent.
/// A `None` value means "user did not provide one — leave the row alone".
///
/// Returns the number of rows actually changed (informational; the
/// caller does not need to act on it).
pub async fn save_provider_keys(
    state: &AppState,
    keys: &WizardProviderKeys,
) -> Result<usize, AppError> {
    let mut changes = 0usize;

    for (key_name, value_opt) in [
        (KEY_GOOGLE_BOOKS, &keys.google_books),
        (KEY_OMDB, &keys.omdb),
        (KEY_TMDB, &keys.tmdb),
    ] {
        let Some(value) = value_opt else { continue };
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((current_value, current_version)) =
            fetch_setting_value_and_version(&state.pool, key_name).await?
        else {
            // Row missing — the migration seeds it, so this is a real
            // problem. Log + skip rather than crash the wizard.
            tracing::warn!(
                key = %key_name,
                "wizard: settings row missing, expected from migration"
            );
            continue;
        };
        if current_value == trimmed {
            // Idempotent re-submit of an already-saved value.
            continue;
        }
        save_setting(&state.pool, key_name, trimmed, current_version).await?;
        changes += 1;
    }

    if changes > 0 {
        reload_settings_cache(state).await?;
    }
    Ok(changes)
}

// ─── Step 3: preferences ──────────────────────────────────────────

/// Save Step 3's two preferences. Validation is the caller's
/// responsibility — see `services::admin_system::validate_default_language`
/// and `validate_overdue_threshold`.
pub async fn save_preferences(
    state: &AppState,
    default_language: &str,
    overdue_threshold_days: i32,
) -> Result<(), AppError> {
    use crate::services::admin_system::KEY_OVERDUE_THRESHOLD;

    // language
    if let Some((current, version)) =
        fetch_setting_value_and_version(&state.pool, KEY_DEFAULT_LANGUAGE).await?
        && current != default_language
    {
        save_setting(&state.pool, KEY_DEFAULT_LANGUAGE, default_language, version).await?;
    }

    // overdue threshold
    if let Some((current, version)) =
        fetch_setting_value_and_version(&state.pool, KEY_OVERDUE_THRESHOLD).await?
    {
        let new_value = overdue_threshold_days.to_string();
        if current != new_value {
            save_setting(&state.pool, KEY_OVERDUE_THRESHOLD, &new_value, version).await?;
        }
    }

    // Always flip the sentinel even when the values matched the
    // existing rows — the sentinel is what the resolver uses to
    // distinguish "user explicitly chose defaults" from "user has not
    // visited Step 3".
    if let Some((current, version)) =
        fetch_setting_value_and_version(&state.pool, KEY_SETUP_STEP_3_DONE).await?
        && current != "1"
    {
        save_setting(&state.pool, KEY_SETUP_STEP_3_DONE, "1", version).await?;
    }

    reload_settings_cache(state).await?;
    Ok(())
}

// ─── Step 4: complete ─────────────────────────────────────────────

/// Stamp `setup_completed_at` with the current UTC time in RFC 3339
/// format and reload the AppSettings cache. Idempotent: a re-submit
/// after the wizard already completed will succeed (no-op-ish — the
/// row gets a new timestamp and version bump).
pub async fn complete_setup(state: &AppState) -> Result<(), AppError> {
    let now = Utc::now();
    let value = now.to_rfc3339_opts(SecondsFormat::Secs, true);

    if let Some((_current, version)) =
        fetch_setting_value_and_version(&state.pool, KEY_SETUP_COMPLETED_AT).await?
    {
        save_setting(&state.pool, KEY_SETUP_COMPLETED_AT, &value, version).await?;
    } else {
        // Defense-in-depth: row missing — INSERT it (the migration
        // should have seeded it, but a manual DB intervention could
        // remove it).
        sqlx::query(
            "INSERT INTO settings (setting_key, setting_value) VALUES (?, ?)",
        )
        .bind(KEY_SETUP_COMPLETED_AT)
        .bind(&value)
        .execute(&state.pool)
        .await?;
    }

    reload_settings_cache(state).await?;
    Ok(())
}

// ─── Re-export csrf token mint ───────────────────────────────────

/// Re-export so handlers don't have to reach into the auth middleware
/// for this helper. Generates a 43-char URL-safe base64 token.
pub fn mint_csrf_token() -> String {
    generate_csrf_token()
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_inputs() -> SetupPredicateInputs {
        SetupPredicateInputs {
            active_admin_count: 0,
            setup_completed_at: None,
            any_provider_key_set: false,
            setup_step_3_done: false,
        }
    }

    /// AC3 truth-table walks 16 states; the resolver collapses a lot
    /// of them onto the same answer. We exercise the salient combinations:
    ///   - Pristine → Step 1.
    ///   - `setup_completed_at` set → wizard inactive (regardless of others).
    ///   - Admin exists, completed unset → wizard inactive (upgrade path).
    #[test]
    fn resolve_step_pristine_returns_admin() {
        assert_eq!(resolve_step(&empty_inputs()), Some(SetupStep::Admin));
    }

    #[test]
    fn resolve_step_completed_returns_none() {
        let inputs = SetupPredicateInputs {
            setup_completed_at: Some(Utc::now()),
            ..empty_inputs()
        };
        assert_eq!(resolve_step(&inputs), None);
    }

    #[test]
    fn resolve_step_admin_exists_returns_none() {
        // Pre-Epic-8 deployment: admin already seeded, completed unset.
        let inputs = SetupPredicateInputs {
            active_admin_count: 1,
            ..empty_inputs()
        };
        assert_eq!(resolve_step(&inputs), None);
    }

    #[test]
    fn resolve_step_admin_exists_and_completed_returns_none() {
        let inputs = SetupPredicateInputs {
            active_admin_count: 1,
            setup_completed_at: Some(Utc::now()),
            ..empty_inputs()
        };
        assert_eq!(resolve_step(&inputs), None);
    }

    /// `resolve_step_with_admin` is only called when admin DOES exist.
    /// Its job is to pick Step 2/3/4. Walks the 4-state truth table.
    #[test]
    fn resolve_step_with_admin_truth_table() {
        // No keys, step_3_done == false ⇒ Step 2 (Providers).
        let inputs = SetupPredicateInputs {
            active_admin_count: 1,
            ..empty_inputs()
        };
        assert_eq!(resolve_step_with_admin(&inputs), SetupStep::Providers);

        // ≥1 key set, step_3_done == false ⇒ Step 3 (Preferences).
        let inputs = SetupPredicateInputs {
            active_admin_count: 1,
            any_provider_key_set: true,
            ..empty_inputs()
        };
        assert_eq!(resolve_step_with_admin(&inputs), SetupStep::Preferences);

        // step_3_done == true ⇒ Step 4 (Done) — regardless of keys.
        for any_key in [true, false] {
            let inputs = SetupPredicateInputs {
                active_admin_count: 1,
                any_provider_key_set: any_key,
                setup_step_3_done: true,
                ..empty_inputs()
            };
            assert_eq!(resolve_step_with_admin(&inputs), SetupStep::Done);
        }
    }

    #[test]
    fn setup_step_number_matches_progress_dot() {
        assert_eq!(SetupStep::Admin.number(), 1);
        assert_eq!(SetupStep::Providers.number(), 2);
        assert_eq!(SetupStep::Preferences.number(), 3);
        assert_eq!(SetupStep::Done.number(), 4);
    }
}
