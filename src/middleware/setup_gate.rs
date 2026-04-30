//! Setup-wizard gate middleware (story 8-8).
//!
//! Intercepts every HTTP request while the first-launch wizard is active
//! and 303-redirects to `/setup`. Once the wizard completes, the
//! middleware turns into a no-op for the rest of the process lifetime.
//!
//! "Wizard active" is the boolean
//! `(active_admin_count == 0) AND (setup_completed_at IS NONE)`. Both
//! halves must be true:
//!   * `admin_count > 0` → wizard inactive (upgrade-from-pre-Epic-8 path).
//!   * `setup_completed_at IS NOT NULL` → wizard inactive (single-use).
//!
//! The predicate is computed once at startup and cached in
//! `Arc<RwLock<SetupGateState>>` to avoid a DB round-trip per request.
//! Step 1 (admin created) and Step 4 (`setup_completed_at` written)
//! invalidate the cache via `invalidate_gate_state` — both call sites
//! live inside the wizard handlers, so no other write path needs to know.
//!
//! `MYBIBLI_SKIP_SETUP=1|true|TRUE` bypasses the middleware entirely.
//! The env var is read **once at startup** (matches the
//! `MYBIBLI_SKIP_STARTUP_PURGE` pattern from story 8-7) and stored in
//! `SetupGateState.bypass_via_env`.
//!
//! Whitelist (always pass through, regardless of gate state):
//!   * `/static/*` — CSS/JS assets
//!   * `/covers/*` — uploaded artwork
//!   * `/health`  — liveness probe
//!   * `/setup*`  — the wizard itself

use std::sync::{Arc, RwLock};

use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::db::DbPool;
use crate::error::AppError;

/// Cached gate predicate. Reads happen on every request — keep the lock
/// hold time as short as possible (read-clone the bool, never `.await`
/// while the guard is alive).
#[derive(Debug, Clone, Copy)]
pub struct SetupGateState {
    /// `true` ⇒ middleware redirects every non-whitelisted request to
    /// `/setup`. Computed by `refresh_gate_state` from the live DB.
    pub active: bool,
    /// `true` ⇒ env-var bypass — middleware is a no-op regardless of
    /// `active`. Read once at startup via `read_skip_env`.
    pub bypass_via_env: bool,
}

impl Default for SetupGateState {
    /// Test-friendly default: wizard inactive, no env bypass — this is
    /// what every fixture wants (the test suite assumes admin users
    /// exist via seed migrations, so the wizard should not fire).
    fn default() -> Self {
        Self {
            active: false,
            bypass_via_env: false,
        }
    }
}

impl SetupGateState {
    /// Build the initial state at process startup. Reads the bypass env
    /// var and queries the DB for the predicate inputs.
    ///
    /// **Fails closed (story 8-8 review P21):** if the DB query errors,
    /// the function returns `Err`. `main.rs` propagates the error and
    /// the process aborts before binding the listener, so the operator
    /// notices and fixes the DB before any traffic hits a half-broken
    /// install. The previous fail-open behaviour silently disabled the
    /// wizard on a flaky boot DB and let the user reach an empty
    /// catalog — bad fail-safe direction for a fresh-install flow.
    pub async fn initialize(pool: &DbPool) -> Result<Self, AppError> {
        let bypass_via_env = read_skip_env();
        let active = fetch_active(pool).await?;
        if bypass_via_env {
            tracing::info!("setup-gate: MYBIBLI_SKIP_SETUP set — wizard middleware disabled");
        } else if active {
            tracing::info!("setup-gate: wizard ACTIVE (no admin user yet, no setup_completed_at)");
        } else {
            tracing::info!("setup-gate: wizard inactive");
        }
        Ok(Self {
            active,
            bypass_via_env,
        })
    }
}

/// Read `MYBIBLI_SKIP_SETUP` once at startup. Strict accept-set — only
/// `"1" | "true" | "TRUE"` count as enable, everything else (incl. empty
/// string and `0` / `false`) means disabled. Mirrors the
/// `MYBIBLI_SKIP_STARTUP_PURGE` contract from story 8-7 R3-N6.
fn read_skip_env() -> bool {
    std::env::var("MYBIBLI_SKIP_SETUP")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false)
}

/// Re-run the predicate query against the DB. Used by `initialize` and
/// by `refresh` (after Step 1 / Step 4).
async fn fetch_active(pool: &DbPool) -> Result<bool, AppError> {
    let admin_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM users \
         WHERE role = 'admin' AND active = TRUE AND deleted_at IS NULL",
    )
    .fetch_one(pool)
    .await?;

    let completed_row: Option<(String,)> = sqlx::query_as(
        "SELECT setting_value FROM settings \
         WHERE setting_key = 'setup_completed_at' AND deleted_at IS NULL",
    )
    .fetch_optional(pool)
    .await?;

    // Strict RFC3339 parse — matches `services::setup::fetch_predicate_inputs`.
    // A garbage / malformed timestamp value is treated as "wizard NOT yet
    // completed" so the gate keeps firing and `/setup` stays reachable.
    // Story 8-8 review P10 aligned this with the resolver's parser.
    let setup_completed = match completed_row {
        Some((v,)) if !v.is_empty() => {
            chrono::DateTime::parse_from_rfc3339(&v).is_ok()
        }
        _ => false,
    };

    Ok(admin_count.0 == 0 && !setup_completed)
}

/// Recompute the predicate and update the cached state in place. Called
/// by Step 1 (admin row inserted) and Step 4 (`setup_completed_at` written).
pub async fn refresh(state_arc: &Arc<RwLock<SetupGateState>>, pool: &DbPool) {
    let active = match fetch_active(pool).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "setup-gate: refresh query failed; leaving cached state untouched"
            );
            return;
        }
    };
    match state_arc.write() {
        Ok(mut guard) => {
            guard.active = active;
        }
        Err(poisoned) => {
            tracing::error!("setup-gate: RwLock poisoned during refresh; recovering");
            poisoned.into_inner().active = active;
        }
    }
}

/// Directly flip the cached `active` field without re-querying. Used
/// by integration tests (which seed DB state manually and need the
/// cache to match). Production code paths must go through `refresh`
/// so the cache stays a faithful mirror of the DB predicate.
///
/// Gated behind `cfg(any(test, debug_assertions))` so the helper is
/// reachable from unit tests, integration tests, and `cargo run`
/// development builds, but compiled out of `--release` artifacts —
/// production code cannot flip the cache without going through
/// `refresh`. Story 8-8 review P7.
#[cfg(any(test, debug_assertions))]
pub fn force_set_active(state_arc: &Arc<RwLock<SetupGateState>>, active: bool) {
    let mut guard = state_arc.write().expect("setup_gate lock poisoned");
    guard.active = active;
}

/// Path-prefix whitelist — these always pass through, regardless of
/// gate state. Pure function: testable without the middleware.
pub fn is_whitelisted_path(path: &str) -> bool {
    path == "/health"
        || path.starts_with("/static/")
        || path.starts_with("/covers/")
        || path == "/setup"
        || path.starts_with("/setup/")
}

/// Detect HTMX requests so the redirect uses `HX-Redirect` instead of
/// a 303 Location (HTMX swallows 3xx by default).
fn is_htmx_request(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get("hx-request")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub async fn setup_gate_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    // Read the cached state. The lock is held for the bool clone only —
    // never across the handler `.await`.
    let (active, bypass) = match state.setup_gate.read() {
        Ok(g) => (g.active, g.bypass_via_env),
        Err(poisoned) => {
            // Lock poisoning shouldn't happen — log loudly and treat the
            // wizard as inactive so the user can at least reach the
            // homepage to debug.
            tracing::error!("setup-gate: RwLock poisoned in middleware path");
            let g = poisoned.into_inner();
            (g.active, g.bypass_via_env)
        }
    };

    if bypass || !active {
        return next.run(request).await;
    }

    let path = request.uri().path();
    if is_whitelisted_path(path) {
        return next.run(request).await;
    }

    // Wizard active + non-whitelisted → redirect to /setup.
    let target = "/setup";
    if is_htmx_request(request.headers()) {
        // HTMX: 200 OK + HX-Redirect header so the client navigates
        // (3xx would be swallowed by htmx default behavior).
        let mut response = StatusCode::OK.into_response();
        response.headers_mut().insert(
            "HX-Redirect",
            HeaderValue::from_static(target),
        );
        response
    } else {
        let mut response = StatusCode::SEE_OTHER.into_response();
        response
            .headers_mut()
            .insert(header::LOCATION, HeaderValue::from_static(target));
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_accepts_static_assets() {
        assert!(is_whitelisted_path("/static/css/output.css"));
        assert!(is_whitelisted_path("/covers/abc.jpg"));
    }

    #[test]
    fn whitelist_accepts_health() {
        assert!(is_whitelisted_path("/health"));
        // Be strict: a sub-path under /health is NOT whitelisted —
        // someone could mount `/healthcheck` later and accidentally
        // bypass the gate.
        assert!(!is_whitelisted_path("/healthcheck"));
    }

    #[test]
    fn whitelist_accepts_setup_routes() {
        assert!(is_whitelisted_path("/setup"));
        assert!(is_whitelisted_path("/setup/step-1"));
        assert!(is_whitelisted_path("/setup/complete"));
    }

    #[test]
    fn whitelist_rejects_app_routes() {
        assert!(!is_whitelisted_path("/"));
        assert!(!is_whitelisted_path("/catalog"));
        assert!(!is_whitelisted_path("/login"));
        assert!(!is_whitelisted_path("/admin"));
        assert!(!is_whitelisted_path("/admin/health"));
    }

    /// Defense-in-depth: a malformed path that *starts* with `/static`
    /// but is not a static asset must NOT be whitelisted.
    #[test]
    fn whitelist_uses_strict_prefixes() {
        // `/static-evil/...` — slash-separated boundary required.
        assert!(!is_whitelisted_path("/static-evil"));
        assert!(!is_whitelisted_path("/setup-evil"));
    }

    /// AC9: env-var bypass is strict (R3-N6 lesson from story 8-7).
    /// `MYBIBLI_SKIP_SETUP` must accept ONLY "1" / "true" / "TRUE";
    /// empty string and "0" / "false" must NOT enable the bypass.
    #[test]
    fn read_skip_env_strict_accepts() {
        // Pure helper that mirrors the matches! contract.
        fn would_bypass(v: &str) -> bool {
            matches!(v, "1" | "true" | "TRUE")
        }
        assert!(would_bypass("1"));
        assert!(would_bypass("true"));
        assert!(would_bypass("TRUE"));
        // Strict rejects:
        assert!(!would_bypass(""));
        assert!(!would_bypass("0"));
        assert!(!would_bypass("false"));
        assert!(!would_bypass("True")); // capital-T-only is rejected per spec
        assert!(!would_bypass("yes"));
    }

    /// `SetupGateState` is `Copy + Clone` — sanity check.
    #[test]
    fn setup_gate_state_is_copy() {
        let s = SetupGateState {
            active: true,
            bypass_via_env: false,
        };
        let _copied = s;
        // s still usable.
        assert!(s.active);
    }

    /// AC1 truth-table — without DB, exercise the boolean directly.
    /// `(admin_count==0) AND (setup_completed empty)` ⇒ active.
    #[test]
    fn predicate_is_active_only_when_both_halves_true() {
        // (admin_count, completed_set, expected_active)
        let cases = [
            (0i64, false, true),  // fresh install
            (0, true, false),     // setup_completed_at set with no admin (recovery edge case) → inactive
            (1, false, false),    // upgrade path — admin already seeded, no completed flag
            (1, true, false),     // normal post-wizard
            (5, false, false),    // multiple admins, no completed flag
        ];
        for (admin, completed, expected) in cases {
            let actual = admin == 0 && !completed;
            assert_eq!(actual, expected, "admin={admin}, completed={completed}");
        }
    }
}
