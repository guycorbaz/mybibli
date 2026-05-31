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
pub const KEY_SETUP_STEP_2_DONE: &str = "setup_step_2_done";
pub const KEY_SETUP_STEP_3_DONE: &str = "setup_step_3_done";
// v1.5.1 fix #283 — Library valuation section. Seeded by migration
// 20260520100000; `routes/admin_system.rs::save_library_valuation_settings`
// writes them through `save_setting`.
pub const KEY_DEFAULT_CURRENCY: &str = "default_currency";
pub const KEY_SHOW_VALUE_INDICATORS: &str = "show_value_indicators";

// v1.7.1 fix #308 — runtime log-level. CR #301 (v1.7.0) advertised
// "flip log level from /admin > System without a redeploy", but the
// admin form + runtime reload were never implemented. Seeded by
// migration 20260522000000; the `routes/admin_system.rs::save_log_level`
// handler writes it through `save_setting` AND triggers the
// `Arc<reload::Handle<EnvFilter>>` stored in `AppState` so the
// `tracing` subscriber actually swaps its filter.
pub const KEY_LOG_LEVEL: &str = "log_level";

// v1.7.9 fix #334 — runtime metadata-chain + provider-health timeouts.
// Seeded by migration 20260526075659; surfaced in the same /admin > System
// "Metadata Providers" block as the API keys. Bounded to 1..=60 s by
// `validate_provider_timeout_secs`.
pub const KEY_METADATA_CHAIN_TIMEOUT: &str = "metadata_chain_per_provider_timeout_secs";
pub const KEY_PROVIDER_HEALTH_TIMEOUT: &str = "provider_health_probe_timeout_secs";

/// Inclusive bounds for both timeout settings. Below 1 s would race typical
/// HTTPS handshakes; above 60 s a stalled provider would block the chain
/// longer than any plausible user-facing latency budget.
pub const PROVIDER_TIMEOUT_MIN_SECS: u64 = 1;
pub const PROVIDER_TIMEOUT_MAX_SECS: u64 = 60;

/// Validate one of the two #334 timeouts. Returns a `BadRequest` with the
/// same i18n message regardless of which setting failed (the form field
/// label disambiguates for the user); callers pass the localized error
/// for re-rendering.
pub fn validate_provider_timeout_secs(value: u64, loc: &'static str) -> Result<(), AppError> {
    if (PROVIDER_TIMEOUT_MIN_SECS..=PROVIDER_TIMEOUT_MAX_SECS).contains(&value) {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            rust_i18n::t!("error.system.provider_timeout_invalid", locale = loc).to_string(),
        ))
    }
}

/// Validate the log-level setting. Accepts:
///   - a plain level: `trace`, `debug`, `info`, `warn`, `error`
///   - a `tracing-subscriber` `EnvFilter` directive list, e.g.
///     `mybibli=debug,sqlx::query=warn`
///
/// Strategy: try `EnvFilter::try_new` (the same parser the runtime
/// reload uses) and translate parse errors into a localized
/// `BadRequest`. Defers to the upstream library's grammar so admins
/// can use the full directive syntax (per-target levels, span filters,
/// etc.) — anything `RUST_LOG=<value>` accepts on a fresh start, this
/// form accepts too.
pub fn validate_log_level(value: &str, loc: &'static str) -> Result<(), AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(
            rust_i18n::t!("error.system.log_level_invalid", locale = loc).to_string(),
        ));
    }
    tracing_subscriber::EnvFilter::try_new(trimmed).map_err(|_| {
        AppError::BadRequest(
            rust_i18n::t!("error.system.log_level_invalid", locale = loc).to_string(),
        )
    })?;
    Ok(())
}

/// Validate the default-currency setting. 3-letter ISO 4217 code,
/// alphabetic, accepted both cases (normalized to upper on write).
pub fn validate_default_currency(value: &str, loc: &'static str) -> Result<(), AppError> {
    let trimmed = value.trim();
    if trimmed.len() == 3 && trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            rust_i18n::t!("error.system.default_currency_invalid", locale = loc).to_string(),
        ))
    }
}

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

/// Validate the default-language setting. Accepts `"fr"`, `"en"`, `"de"`,
/// or `"it"` — anything else (including the empty string) is `BadRequest`.
/// CR #275 / #276 (v1.7.0) added DE + IT to the original FR/EN set.
pub fn validate_default_language(value: &str, loc: &'static str) -> Result<(), AppError> {
    if matches!(value, "fr" | "en" | "de" | "it") {
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
    check_update_result(result.rows_affected(), "setting")
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
    fn validate_default_language_accepts_all_supported() {
        // v1.7.0 (CR #275 / #276) added DE + IT to the original FR/EN set.
        assert!(validate_default_language("fr", "en").is_ok());
        assert!(validate_default_language("en", "en").is_ok());
        assert!(validate_default_language("de", "en").is_ok());
        assert!(validate_default_language("it", "en").is_ok());
    }

    // ─── Fix #308 (v1.7.1) — validate_log_level ───────────────────

    #[test]
    fn validate_log_level_accepts_plain_levels() {
        for lvl in &["trace", "debug", "info", "warn", "error"] {
            assert!(
                validate_log_level(lvl, "en").is_ok(),
                "plain level {lvl} must validate"
            );
        }
    }

    #[test]
    fn validate_log_level_accepts_envfilter_directives() {
        for dir in &[
            "mybibli=debug",
            "mybibli=trace",
            "mybibli=debug,sqlx::query=warn",
            "info,mybibli::routes=trace",
        ] {
            assert!(
                validate_log_level(dir, "en").is_ok(),
                "EnvFilter directive {dir:?} must validate"
            );
        }
    }

    #[test]
    fn validate_log_level_rejects_empty_or_garbage() {
        assert!(matches!(
            validate_log_level("", "en"),
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            validate_log_level("   ", "en"),
            Err(AppError::BadRequest(_))
        ));
        // Bogus per-target directive — keyword `quiet` isn't a level.
        assert!(matches!(
            validate_log_level("mybibli=quiet", "en"),
            Err(AppError::BadRequest(_))
        ));
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
        assert!(matches!(
            validate_default_language("pt", "en"),
            Err(AppError::BadRequest(_))
        ));
        // Case-sensitive: callers must lowercase before validating.
        assert!(matches!(
            validate_default_language("FR", "en"),
            Err(AppError::BadRequest(_))
        ));
        assert!(matches!(
            validate_default_language("DE", "en"),
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

    // ─── Fix #334 (v1.7.9) — validate_provider_timeout_secs ──────

    #[test]
    fn validate_provider_timeout_accepts_inclusive_bounds() {
        assert!(validate_provider_timeout_secs(PROVIDER_TIMEOUT_MIN_SECS, "en").is_ok());
        assert!(validate_provider_timeout_secs(PROVIDER_TIMEOUT_MAX_SECS, "en").is_ok());
        for v in [3, 5, 10, 20, 45] {
            assert!(
                validate_provider_timeout_secs(v, "en").is_ok(),
                "{v} should validate inside 1..=60"
            );
        }
    }

    #[test]
    fn validate_provider_timeout_rejects_out_of_range() {
        for v in [0_u64, 61, 120, 3600] {
            assert!(
                matches!(
                    validate_provider_timeout_secs(v, "en"),
                    Err(AppError::BadRequest(_))
                ),
                "{v} should fail validation"
            );
        }
    }

    #[test]
    fn provider_timeout_keys_match_migration_strings() {
        assert_eq!(
            KEY_METADATA_CHAIN_TIMEOUT,
            "metadata_chain_per_provider_timeout_secs"
        );
        assert_eq!(
            KEY_PROVIDER_HEALTH_TIMEOUT,
            "provider_health_probe_timeout_secs"
        );
    }
}
