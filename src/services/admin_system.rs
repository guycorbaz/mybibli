//! Shared helpers for the K/V `settings` table — used by both the admin
//! "System settings" panel (`/admin?tab=system`, story 8-5) and the
//! first-launch setup wizard (`/setup`, story 8-8).
//!
//! Lifted out of `routes/admin_system.rs` to satisfy "rule of three":
//! the optimistic-locking save chain + AppSettings cache reload now has
//! two callers (admin/system + wizard) and these helpers are the
//! shortest common subset that both need. The complex per-form UI logic
//! (action_for, apply_provider_action, masking) stays in
//! `routes/admin_system.rs` because only the admin form needs it.

use crate::AppState;
use crate::config::AppSettings;
use crate::db::DbPool;
use crate::error::AppError;
use crate::services::locking::check_update_result;

/// Setting keys — single source of truth for the rows the admin / wizard
/// can write. Exported because both `routes/admin_system.rs` and
/// `routes/setup.rs` reference them.
pub const KEY_OVERDUE_THRESHOLD: &str = "overdue_loan_threshold_days";
pub const KEY_DEFAULT_LANGUAGE: &str = "default_language";
pub const KEY_GOOGLE_BOOKS: &str = "google_books_api_key";
pub const KEY_OMDB: &str = "omdb_api_key";
pub const KEY_TMDB: &str = "tmdb_api_key";
pub const KEY_SETUP_COMPLETED_AT: &str = "setup_completed_at";
pub const KEY_SETUP_STEP_3_DONE: &str = "setup_step_3_done";

/// Inclusive bounds for the overdue-loan threshold. Validated by both
/// `routes/admin_system.rs::save_loans_settings` and the wizard's
/// Step 3.
pub const OVERDUE_THRESHOLD_MIN: i32 = 1;
pub const OVERDUE_THRESHOLD_MAX: i32 = 365;

/// Validate the overdue-loan threshold (days). Returns `BadRequest` with
/// a localized message if out of range.
pub fn validate_overdue_threshold(days: i32, loc: &'static str) -> Result<(), AppError> {
    if days < OVERDUE_THRESHOLD_MIN {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.system.overdue_threshold_invalid", locale = loc).to_string(),
        ));
    }
    if days > OVERDUE_THRESHOLD_MAX {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.system.overdue_threshold_too_large", locale = loc).to_string(),
        ));
    }
    Ok(())
}

/// Validate the default-language setting. Accepts `"fr"` or `"en"`
/// only — anything else (including the empty string) is `BadRequest`.
pub fn validate_default_language(value: &str, loc: &'static str) -> Result<(), AppError> {
    if value == "fr" || value == "en" {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            rust_i18n::t!("error.system.default_language_invalid", locale = loc).to_string(),
        ))
    }
}

/// Optimistic-lock UPDATE for a single setting row. Returns
/// `AppError::Conflict` on `rows_affected = 0` (stale version or row
/// missing). The generic `executor` parameter accepts both `&DbPool`
/// and `&mut Transaction` so callers can include the save in a wider
/// transaction.
pub async fn save_setting<'e, E>(
    executor: E,
    key: &str,
    new_value: &str,
    expected_version: i32,
) -> Result<(), AppError>
where
    E: sqlx::Executor<'e, Database = sqlx::MySql>,
{
    let result = sqlx::query(
        "UPDATE settings SET setting_value = ?, version = version + 1 \
         WHERE setting_key = ? AND version = ? AND deleted_at IS NULL",
    )
    .bind(new_value)
    .bind(key)
    .bind(expected_version)
    .execute(executor)
    .await?;
    check_update_result(result.rows_affected(), &format!("setting:{key}"))
}

/// Re-SELECT every settings row and swap the `Arc<RwLock<AppSettings>>`
/// cache. The `.await` happens BEFORE the write lock is taken; the lock
/// is held for one move-assignment then dropped. On lock poisoning, log
/// at error level and recover via `into_inner` rather than silently
/// leaving the cache stale.
pub async fn reload_settings_cache(state: &AppState) -> Result<(), AppError> {
    let new_settings = AppSettings::load_from_db(&state.pool)
        .await
        .map_err(|e| AppError::Internal(format!("settings reload failed: {e}")))?;
    match state.settings.write() {
        Ok(mut guard) => *guard = new_settings,
        Err(poisoned) => {
            tracing::error!("settings RwLock poisoned during reload; recovering");
            *poisoned.into_inner() = new_settings;
        }
    }
    Ok(())
}

/// Fetch the current value + version of a single setting row. Returns
/// `Ok(None)` if the row does not exist (caller should treat as
/// "version 0, value empty"). Used by the wizard to read the version
/// before issuing an optimistic-lock UPDATE.
pub async fn fetch_setting_value_and_version(
    pool: &DbPool,
    key: &str,
) -> Result<Option<(String, i32)>, AppError> {
    let row: Option<(String, i32)> = sqlx::query_as(
        "SELECT setting_value, version FROM settings \
         WHERE setting_key = ? AND deleted_at IS NULL",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_overdue_threshold_rejects_zero() {
        assert!(matches!(
            validate_overdue_threshold(0, "en"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn validate_overdue_threshold_rejects_above_365() {
        assert!(matches!(
            validate_overdue_threshold(366, "en"),
            Err(AppError::BadRequest(_))
        ));
    }

    #[test]
    fn validate_overdue_threshold_accepts_in_range() {
        assert!(validate_overdue_threshold(1, "en").is_ok());
        assert!(validate_overdue_threshold(30, "en").is_ok());
        assert!(validate_overdue_threshold(365, "en").is_ok());
    }

    #[test]
    fn validate_default_language_accepts_fr_en() {
        assert!(validate_default_language("fr", "en").is_ok());
        assert!(validate_default_language("en", "en").is_ok());
    }

    #[test]
    fn validate_default_language_rejects_empty_or_other() {
        assert!(matches!(
            validate_default_language("", "en"),
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            validate_default_language("es", "en"),
            Err(AppError::BadRequest(_))
        ));
        // Case-sensitive: callers must lowercase before validating.
        assert!(matches!(
            validate_default_language("FR", "en"),
            Err(AppError::BadRequest(_))
        ));
    }

    /// Regression: Step 3 truth-table relies on `KEY_SETUP_STEP_3_DONE`
    /// being exactly the string the migration writes.
    #[test]
    fn setup_keys_match_migration_strings() {
        assert_eq!(KEY_SETUP_COMPLETED_AT, "setup_completed_at");
        assert_eq!(KEY_SETUP_STEP_3_DONE, "setup_step_3_done");
    }
}
