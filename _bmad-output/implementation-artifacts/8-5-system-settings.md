# Story 8.5: System settings

Status: ready-for-dev

Epic: 8 — Administration & Configuration
Requirements mapping: FR74 (overdue threshold configurable), FR75 (per-provider API keys configurable), AR9 (`Arc<RwLock<AppSettings>>` reload on save — load-bearing for "no restart needed"), NFR37 (single-host local — accepted plaintext API key storage), Story 7-3 locale resolution chain (default-language is fallback level 5 — last-resort default behind cookie / user pref / Accept-Language), Foundation Rules #1–#7

---

> **At a glance:**
> - **Three forms, three Save buttons** — Loans (overdue threshold), Metadata providers (3 API keys), Language (FR/EN). Each setting is a row in the existing K/V `settings` table with its own `version INT` so concurrent edits to different settings don't collide.
> - **API keys move from env vars to the DB**. The three keyed providers (`GoogleBooksProvider`, `OmdbProvider`, `TmdbProvider`) get `Arc<RwLock<AppSettings>>` and read their key per-fetch; OMDb and TMDb register unconditionally now (the env-var-presence gate at `main.rs:138-147` is removed). A save reloads the cache; the next request — including the next provider fetch — uses the new value with no restart.
> - **Default language** is added to `AppSettings` and replaces the hardcoded `"fr"` at `src/middleware/locale.rs:80` as the last-resort fallback in story 7-3's chain. Affects only new anonymous sessions with no `lang=` cookie and no Accept-Language match.
> - **API key UX is sentinel-free**: input renders blank, helper text shows `Set: ••••<last4>` or `Not set`, empty submit = no change, an explicit "Clear" checkbox sets `_clear_<key>=true` to wipe. **This deviates from epic AC #3** (which prescribes mask-string-equality detection) — see §Cross-cutting decisions for the rationale and the revert option.
> - **One-shot env-var migration** at boot: a separate `migrate_legacy_env_vars(pool)` function (called once from `main.rs`, NOT from `load_from_db`) copies legacy `GOOGLE_BOOKS_API_KEY` / `OMDB_API_KEY` / `TMDB_API_KEY` env vars into empty settings rows. Settings reloads within the same process don't re-run it — so an admin Clear of a key stays cleared for the process lifetime. A **process restart with the env var still set re-migrates** the value back into the row; see AC #8 for the full durability semantics.

## Story

As an **admin**,
I want to configure application-wide settings — **overdue loan threshold**, **metadata provider API keys**, and **default app language** — from a single form in `/admin?tab=system`,
so that I can tune behavior without redeploying the container, and changes take effect on the very next request without a process restart.

## Scope Reality & What This Story Ships

### What's already in place (do NOT redo)

**The `settings` table is K/V** — `migrations/20260329000000_initial_schema.sql:262-280`:

```sql
CREATE TABLE settings (
    setting_key VARCHAR(255) NOT NULL PRIMARY KEY,
    setting_value TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_settings_deleted_at (deleted_at)
);
```

One row per setting; each row has its own `version INT`. The epic AC's wording "concurrent admin edits on the same setting **row**" is literal — concurrency is per-setting, not per-form. Two admins editing the overdue threshold and the Google Books key at the same time do NOT collide.

**Initial seeded rows** (lines 273-279): `overdue_loan_threshold_days='30'`, `session_inactivity_timeout_hours='4'`, `session_warning_minutes_before_expiry='5'`, `scanner_burst_threshold_ms='50'`, `search_debounce_delay_ms='300'`, `metadata_fetch_timeout_seconds='30'`. **Story 8-7** added a code-side default for `auto_purge_interval_seconds` (no migration row; loaded with default 86400 if the row is absent).

**`AppSettings` struct** — `src/config.rs:154-166`:

```rust
pub struct AppSettings {
    pub overdue_threshold_days: i32,
    pub scanner_burst_threshold_ms: u64,
    pub search_debounce_delay_ms: u64,
    pub session_timeout_secs: u64,
    pub metadata_fetch_timeout_secs: u64,
    pub auto_purge_interval_seconds: u64,
}
```

`Default` impl at `src/config.rs:168-179`. Loader at `src/config.rs:181-280` — parses every row into the right field via a giant `match key.as_str()`, with per-key validation (`parse::<i32>`, `parse::<u64>`, plus clamping for `auto_purge_interval_seconds`). **Story 8-5 extends this struct** with four new fields and adds four new match arms to the loader.

**`AppState`** — `src/lib.rs:30-41`. Already carries `Arc<RwLock<AppSettings>>`, `Arc<ProviderRegistry>`, `pool`, `http_client`. The pattern of cloning a scalar out of the lock to avoid holding the guard across `.await` is encoded in the helper `AppState::session_timeout_secs()` at `src/lib.rs:47-52`. **Story 8-5 adds four sibling helpers** (`default_language()`, `google_books_api_key()`, `omdb_api_key()`, `tmdb_api_key()`) — each returns `Option<String>` for the keys (`None` if empty), `String` for the language. Use this pattern; do NOT inline `state.settings.read().unwrap().<field>.clone()` at call sites — the helpers are the documented API.

**Read pattern in production code** — `src/routes/loans.rs:92`:

```rust
let threshold = state.settings.read().unwrap().overdue_threshold_days;
```

The threshold IS already AppSettings-driven (story 8-1 wired it). The loans page renders the badge via template-side comparison (`loan.duration_days > overdue_threshold` — see `src/models/loan.rs:106-121` for the `DATEDIFF(NOW(), l.loaned_at) AS duration_days` SQL). **Story 8-5 changes nothing in the loans pipeline** — it just makes the threshold value editable from the admin UI.

**`services::locking::check_update_result`** — `src/services/locking.rs:5-12`:

```rust
pub fn check_update_result(rows_affected: u64, entity_type: &str) -> Result<(), AppError> {
    if rows_affected == 0 {
        Err(AppError::Conflict(rust_i18n::t!("error.conflict", entity = entity_type).to_string()))
    } else {
        Ok(())
    }
}
```

Returns `AppError::Conflict(String)` → HTTP 409 via `IntoResponse`. Use it as-is. The `entity_type` arg is the i18n token to interpolate; pass `"settings"` (or per-key e.g., `"setting:overdue_threshold"` if we want per-setting feedback granularity — see Cross-cutting decisions).

**Locale resolution chain (story 7-3)** — `src/i18n/resolve.rs:19-42` + `src/middleware/locale.rs`:

```rust
let locale = resolve_locale(
    query_lang.as_deref(),       // 1. ?lang= query (render-only)
    cookie_lang.as_deref(),      // 2. lang= cookie
    user_pref.as_deref(),        // 3. authenticated user's preferred_language
    accept_language.as_deref(),  // 4. Accept-Language header
    "fr",                        // 5. last-resort default — HARDCODED today
);
```

The hardcoded `"fr"` at `src/middleware/locale.rs:80` is the surface this story changes. Replace with a clone from `state.settings.read().unwrap().default_language` (or the new helper). The middleware already holds `state: AppState` so the change is local.

**Provider construction** — `src/main.rs:115-153`. The three keyed providers receive their key via env-var read at construction time:

```rust
let gb_key = std::env::var("GOOGLE_BOOKS_API_KEY").ok();
registry.register(Box::new(GoogleBooksProvider::new(http_client.clone(), gb_key)));
// ...
if let Ok(omdb_key) = std::env::var("OMDB_API_KEY") {
    registry.register(Box::new(OmdbProvider::new(http_client.clone(), omdb_key)));
} else {
    tracing::warn!("OMDB_API_KEY not set — OMDb provider disabled");
}
if let Ok(tmdb_key) = std::env::var("TMDB_API_KEY") {
    registry.register(Box::new(TmdbProvider::new(http_client.clone(), tmdb_key)));
} else {
    tracing::warn!("TMDB_API_KEY not set — TMDb provider disabled");
}
```

**This is the load-bearing refactor of the story.** See Cross-cutting decisions for the chosen approach (refactor the three providers to read their key from `AppSettings` at fetch time; register all three unconditionally; "no key set" becomes a runtime no-op rather than an unregistered provider).

**`/admin?tab=system` is wired but stubbed** — handler at `src/routes/admin.rs:932-941`, struct `AdminSystemPanel { stub_message }` at `src/routes/admin.rs:385-388`, template at `templates/fragments/admin_system_panel.html` (4-line stub). The 8-1 tab routing (`AdminTab::System`, `?tab=system` resolution, role gate) is correct as-is — story 8-5 replaces only the panel body. Note the stub template comment incorrectly says "Replaced by story 8-4" — fix to "Replaced by story 8-5" or remove the comment since the stub is deleted.

**`forms_include_csrf_token` audit** — `src/templates_audit.rs:302-394`. Walks `templates/` for `<form method="POST">` and asserts `_csrf_token` appears within the first 5 inputs. **Every form in this story is a POST form** and MUST include `<input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">` as one of its first inputs (first child is the strongest position).

**`ALLOWED_HX_CONFIRM_SITES`** — `src/templates_audit.rs:35-41`. Frozen at 5. **Story 8-5 does NOT extend it.** Settings saves are non-destructive — no confirmation prompt is required.

**`CSRF_EXEMPT_ROUTES`** — `src/middleware/csrf.rs`. Frozen at `[("POST", "/login")]`. **Story 8-5 does NOT add to it.** Every new POST route is automatically CSRF-protected.

### What this story ships

**1. Migration — `migrations/<DATE>_seed_system_settings_rows.sql`** (new, idempotent). Adds four rows to the `settings` table with empty / safe defaults so the optimistic-locking UPDATE has a row to target on first save:

```sql
INSERT IGNORE INTO settings (setting_key, setting_value) VALUES
  ('default_language', 'fr'),
  ('google_books_api_key', ''),
  ('omdb_api_key', ''),
  ('tmdb_api_key', '');
```

`INSERT IGNORE` is the canonical idempotent-seed pattern in this project (the existing seed migrations use it, e.g., `migrations/20260330000001_seed_default_genres.sql`). On a fresh install, all four rows land. On an upgrade from a DB that has been running pre-8-5, three of the four are missing; the fourth (`default_language`) might be present from local dev — IGNORE protects either case.

**Why empty strings for the keys (not NULL)**: the `setting_value` column is `TEXT NOT NULL`. The NULL-vs-empty-string question collapses on insert. Empty string is the project convention for "key not set" (zero-bytes is unambiguous; comparison is `value.is_empty()`). The handler logic distinguishes "submit empty → no change" from "explicit clear → write empty string" via the form contract (see point 5 below), so the database value `""` always means "no key configured".

**2. `AppSettings` struct extension** — `src/config.rs:154-166` becomes:

```rust
pub struct AppSettings {
    pub overdue_threshold_days: i32,
    pub scanner_burst_threshold_ms: u64,
    pub search_debounce_delay_ms: u64,
    pub session_timeout_secs: u64,
    pub metadata_fetch_timeout_secs: u64,
    pub auto_purge_interval_seconds: u64,
    // === NEW (story 8-5) ===
    pub default_language: String,            // "fr" | "en"
    pub google_books_api_key: String,        // empty = not configured
    pub omdb_api_key: String,                // empty = not configured
    pub tmdb_api_key: String,                // empty = not configured
}
```

`Default` impl gains four new defaults: `default_language: "fr".to_string()` (matches the current hardcoded behavior — fresh DB has no behavioral change), and three empty-string keys. Loader's `match key.as_str()` block adds four new arms; the `default_language` arm validates against the set `{"fr", "en"}` (silently falls back to default + log warn on invalid — matches the existing per-key warn-and-fallback pattern in the loader for invalid integers).

**3. One-shot env-var migration — `pub async fn migrate_legacy_env_vars(pool: &DbPool) -> Result<(), sqlx::Error>` in `src/config.rs`** (separate from `load_from_db`). Called exactly once from `main.rs` boot, AFTER migrations run and BEFORE the first `AppSettings::load_from_db` call:

```rust
pub async fn migrate_legacy_env_vars(pool: &DbPool) -> Result<(), sqlx::Error> {
    for (env_var, key) in &[
        ("GOOGLE_BOOKS_API_KEY", "google_books_api_key"),
        ("OMDB_API_KEY",         "omdb_api_key"),
        ("TMDB_API_KEY",         "tmdb_api_key"),
    ] {
        let env_value = match std::env::var(env_var) {
            Ok(v) if !v.is_empty() => v,
            _ => continue,
        };
        // Only migrate when the row's current value is empty.
        let current: Option<(String,)> = sqlx::query_as(
            "SELECT setting_value FROM settings WHERE setting_key = ? AND deleted_at IS NULL"
        ).bind(key).fetch_optional(pool).await?;
        if matches!(current, Some((ref v,)) if v.is_empty()) {
            sqlx::query("UPDATE settings SET setting_value = ?, version = version + 1 WHERE setting_key = ? AND deleted_at IS NULL")
                .bind(&env_value).bind(key).execute(pool).await?;
            tracing::info!(env_var = %env_var, "Migrated legacy env-var API key into settings table");
        }
    }
    Ok(())
}
```

**Why a separate function rather than inlining at the bottom of `load_from_db`**: every settings save calls `load_from_db` to refresh the cache. If the env-var copy lived in the loader, an admin who Clears a key whose env var is still set would see the value re-migrated on the next save — Clear would silently un-Clear itself. Keeping the migration as a one-shot boot step makes the user-facing Clear durable.

After this function runs once on boot, the settings rows hold the env-var values; the env vars are unused going forward. **`docker-compose.yml` env-var declarations stay in place** — they're ignored by mybibli on subsequent boots, but removing them is the admin's call (a follow-up cleanup story can document that as deprecated).

**4. Four `AppState` getter helpers** — `src/lib.rs`, after the existing `session_timeout_secs()`:

```rust
pub fn default_language(&self) -> String {
    self.settings.read().map(|s| s.default_language.clone()).unwrap_or_else(|_| "fr".into())
}
pub fn google_books_api_key(&self) -> Option<String> {
    self.settings.read().ok().and_then(|s| {
        if s.google_books_api_key.is_empty() { None } else { Some(s.google_books_api_key.clone()) }
    })
}
pub fn omdb_api_key(&self) -> Option<String> { /* same shape */ }
pub fn tmdb_api_key(&self) -> Option<String> { /* same shape */ }
```

Returning `Option<String>` for keys (None on empty string) is the contract "skip this provider" check sites depend on.

**5. Refactor the three keyed providers to read keys at fetch time, not construction time.**

Each of `GoogleBooksProvider`, `OmdbProvider`, `TmdbProvider`:

- Constructor signature changes from `pub fn new(http_client: reqwest::Client, api_key: Option<String>)` → `pub fn new(http_client: reqwest::Client, settings: Arc<RwLock<AppSettings>>)`. The provider stores the settings handle, not the key.
- The fetch implementation reads its key on each call: `let key = self.settings.read().ok().and_then(|s| if s.<field>.is_empty() { None } else { Some(s.<field>.clone()) });`. If `None`, the provider returns the existing "no API key configured" early-return (whatever the current code does — likely an `Ok(MetadataResult::NotFound)` or similar; verify in the existing impls).
- Fetch the value with `.read()` ONLY inside the synchronous prelude before any `.await` — clone into a local `String`, drop the guard, then proceed to the HTTP call. This is the same lock-discipline pattern as the existing `loans.rs:92` read.
- **`OmdbProvider` and `TmdbProvider` are now registered unconditionally** — the env-var-presence gate in `main.rs:138-147` is removed. The "skip if no key" decision moves entirely into the providers' fetch methods. Provider-health pings (story 8-1) for these providers don't use API keys, so unconditional registration doesn't break health-tab semantics.
- `GoogleBooksProvider`'s registration call site at `main.rs:124-127` simplifies — `Box::new(GoogleBooksProvider::new(http_client.clone(), state.settings.clone()))` — but `state` doesn't exist at that point in `main.rs`; pass the `Arc<RwLock<AppSettings>>` directly (it's constructed earlier in `main.rs`; the registry is built before `AppState` is finalized, so the order needs a small reshuffle — load settings first, build providers passing the settings handle, then construct the rest of `AppState`).

**Why this approach over rebuilding the registry on save** — rebuilding `Arc<ProviderRegistry>` would require recreating every provider (including the rate-limited `MusicBrainzProvider` whose internal `RateLimiter` state would be reset, breaking the 1-req/sec invariant). Read-on-fetch is the cleaner contract: providers see new keys on the next fetch automatically; no registry-rebuild dance; no shared-mutable-state surprises. The trade-off is a `RwLock::read()` per fetch — vanishingly cheap (single-digit nanoseconds for an uncontended read on x86_64).

**6. Locale-middleware default replaces hardcoded `"fr"`** — `src/middleware/locale.rs:75-81`:

```rust
let default_lang = state.default_language();   // new helper, scalar clone, no async
let locale = resolve_locale(
    query_lang.as_deref(),
    cookie_lang.as_deref(),
    user_pref.as_deref(),
    accept_language.as_deref(),
    &default_lang,                             // was: "fr"
);
```

The middleware is already async and already holds `state: AppState`. The change is one line + one helper call.

**Note on `'static str` vs owned `String`**: `resolve_locale`'s current signature takes `default: &'static str` per the agent's report. With the AppSettings value owned, we either (a) change the signature to `default: &str` (loses no type-safety, just a lifetime relaxation), or (b) match-compute a `&'static str` from the owned value (`match s.as_str() { "fr" => "fr", _ => "en" }`). Choose (a) — simpler, no string-shape gymnastics.

**7. New module `src/routes/admin_system.rs`** (NEW FILE). Houses all three groups' handlers + their template structs.

Why a separate module rather than adding to `admin.rs`: `admin.rs` is at 1571 lines pre-8-4 and growing. 8-4 already extracts to `admin_reference_data.rs` (per that story). 8-5 follows the same pattern. Foundation Rule #12 (2000-line ceiling) wants to keep us under.

**Routes** (mounted in `src/routes/mod.rs`):

```text
GET  /admin/system                       → admin_system_panel
POST /admin/system/loans                 → save_loans_settings        (overdue_threshold_days)
POST /admin/system/providers             → save_provider_keys         (3 keys, in one transaction)
POST /admin/system/language              → save_language_settings     (default_language)
```

4 routes total. All POST routes are CSRF-protected by 8-2 middleware automatically. Handler signatures all start with `session.require_role_with_return(Role::Admin, &return_path)?;`.

**8. Form structs** — in `admin_system.rs`:

```rust
#[derive(Deserialize)]
pub struct LoansSettingsForm {
    pub overdue_threshold_days: i32,
    pub overdue_threshold_version: i32,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct ProviderKeysForm {
    // Each key: empty = no change; non-empty = new value; _clear_<key>=true overrides empty to "explicit clear".
    pub google_books_api_key: String,
    pub google_books_version: i32,
    pub _clear_google_books: Option<String>,
    pub omdb_api_key: String,
    pub omdb_version: i32,
    pub _clear_omdb: Option<String>,
    pub tmdb_api_key: String,
    pub tmdb_version: i32,
    pub _clear_tmdb: Option<String>,
    pub _csrf_token: String,
}

#[derive(Deserialize)]
pub struct LanguageSettingsForm {
    pub default_language: String,    // "fr" | "en"
    pub default_language_version: i32,
    pub _csrf_token: String,
}
```

**9. Generic save-and-reload helper** in `admin_system.rs`:

```rust
async fn save_setting(
    pool: &DbPool,
    key: &str,
    new_value: &str,
    expected_version: i32,
) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE settings SET setting_value = ?, version = version + 1 \
         WHERE setting_key = ? AND version = ? AND deleted_at IS NULL"
    )
    .bind(new_value)
    .bind(key)
    .bind(expected_version)
    .execute(pool)
    .await?;
    services::locking::check_update_result(result.rows_affected(), &format!("setting:{key}"))
}

async fn reload_settings_cache(state: &AppState) -> Result<(), AppError> {
    let new_settings = AppSettings::load_from_db(&state.pool).await
        .map_err(|e| AppError::Internal(format!("settings reload failed: {e}")))?;
    *state.settings.write().unwrap() = new_settings;
    Ok(())
}
```

Each handler calls `save_setting(...)` for each setting it owns (three calls in the providers handler, one in loans, one in language) **inside a `pool.begin()` transaction**, then calls `reload_settings_cache(state)` AFTER the transaction commits. Important: do NOT hold the `RwLock` write guard across `.await` — `*state.settings.write().unwrap() = new_settings` is sync-bounded; the `.await` is on the preceding `load_from_db`.

**Concurrency-aware Providers handler** — three setting rows, each with its own version. The handler:

1. Begins a transaction.
2. For each provider key: determine the intended action — "no change" (input empty AND `_clear_*` not set), "explicit clear" (input empty AND `_clear_*=true`), or "set new value" (input non-empty).
3. For "no change": skip the UPDATE entirely. Version unchanged. Skip the version check too — saves a roundtrip and avoids spurious 409s when the admin only edited one of the three.
4. For "explicit clear" or "set new value": call `save_setting(...)` with the stored expected_version. On 409, rollback the transaction and return a feedback message identifying *which* setting was stale (use the per-key i18n token).
5. Commit. Reload cache.

**10. Templates** — see Tasks for the full list. Three new partials:
- `templates/fragments/admin_system_panel.html` (REPLACE existing 4-line stub) — outermost panel; contains three `<section>` blocks, one per group.
- `templates/fragments/admin_system_loans_form.html` (NEW) — single-input form for the overdue threshold. Hidden `_csrf_token` and `overdue_threshold_version` as first children. Submit button: "Save loans settings".
- `templates/fragments/admin_system_providers_form.html` (NEW) — three text inputs (one per provider), each with its own helper text (`Set: ••••<last4>` / `Not set`) + per-field "Clear" checkbox. Three hidden `_<provider>_version` fields. Single Submit button: "Save provider keys".
- `templates/fragments/admin_system_language_form.html` (NEW) — two-radio (FR / EN) form. Hidden `default_language_version`. Submit: "Save language settings".

**11. JS module — minimal** (no new file). The "Clear" checkbox per provider key is a normal checkbox; when checked, the provider's text input gains the `disabled` attribute (so the user understands the key field is being intentionally cleared) — this UX nicety wires through a small handler in `static/js/mybibli.js`. Single new `data-action="provider-key-clear-toggle"` delegated handler — ~10 lines. Pattern: same as the existing `dismiss-feedback` handler at `static/js/mybibli.js:195-202`. **CSP-compliant** — no inline event handlers.

**12. i18n keys — EN + FR** under `admin.system.*`, `success.system.*`, `error.system.*`. See §Tasks point 5 for the full list.

**13. Documentation:**
- `CLAUDE.md` Key Patterns: append a "System settings (story 8-5)" bullet covering the K/V table shape, the per-row version + per-form-group save model, the read-on-fetch contract for keyed providers, and the one-shot env-var → DB migration. Short bullet — Key Patterns is not a reference manual.
- `docs/route-role-matrix.md`: 4 route entries touched (1 GET handler reference updated + 3 POST routes added), all CSRF-protected, all Admin role — execution lives in Task 4.12.
- `_bmad-output/planning-artifacts/architecture.md` Authentication & Security or Configuration Architecture section: add a paragraph on the AppSettings + reload-on-save contract for AR9 — document that the cache reload is "load full struct from DB → swap under write lock", and that providers read keys per-fetch (no registry rebuild).

### What this story does NOT ship

- **No encryption at rest for API keys.** Per epic AC: "stored in plaintext (no encryption at rest — accepted trade-off because of NFR37)". Future story can add per-key encryption with a deployment-supplied master key if Guy ever exposes the box to the internet; not this story.
- **No setup-wizard integration** (8-8 owns that). The wizard creates the admin and seeds initial settings; this story manages the post-setup edits.
- **No edit UI for the OTHER `AppSettings` fields** (`scanner_burst_threshold_ms`, `search_debounce_delay_ms`, `session_timeout_secs`, `metadata_fetch_timeout_secs`, `auto_purge_interval_seconds`). Spec is explicit: only overdue threshold, API keys, and default language. The other fields stay env-var / migration-default driven for v1.
- **No removal of `docker-compose.yml` env-var declarations** for `GOOGLE_BOOKS_API_KEY` / `OMDB_API_KEY` / `TMDB_API_KEY`. The one-shot migration in point 3 makes them backward-compatible for the existing deploy; explicit removal is a follow-up cleanup story (open as a `type:change-request` GitHub issue at the end of this story, link from the commit).
- **No "test connection" button next to each provider key.** The Health tab (story 8-1) already pings provider URLs every 5 minutes and surfaces reachability; an explicit per-key health check would re-implement what 8-1 does. If Guy asks later, add it as a follow-up.
- **No Comic Vine API key field.** The provider is implemented but commented out at `src/main.rs:149-151`. Story 8-5 does NOT register it. Adding it would be a one-line settings row + form field + provider re-instantiation, but per CLAUDE.md "don't add features beyond what the task requires", we stay aligned with the AC's three-keyed-providers scope.
- **No length validation on API keys.** Each provider's key format is opaque; mybibli does not own that contract.
- **No HTML preview of the masked key as the user types** (e.g., live "•••" rendering). Plain text input with browser's normal echo. Provider keys are not passwords; defense in depth is moot in the home-NAS context (NFR37).
- **No history/audit log of who changed what setting when.** `admin_audit` (story 8-7) is reserved for permanent-delete actions; settings edits are deliberately not audited in v1. Easy follow-up if needed.

## Cross-cutting decisions

**Epic AC enumerates "BnF, Google Books" — code reality is Google Books, OMDb, TMDb.**

The epic AC #1 says "per-provider API key fields (one row per provider enumerated by `metadata/` — currently BnF, Google Books)". This is wrong about BnF: `BnfProvider::new()` takes only the http client, no key (`src/main.rs:122`). The actual key-bearing providers in the registry are Google Books (`src/main.rs:124`), OMDb (`src/main.rs:139`, conditional today), and TMDb (`src/main.rs:144`, conditional today). The epic AC's E2E line also says "enter a fake BnF key" — which can't work because BnF has no key field to enter into.

This story scopes to the three real keyed providers (Google Books, OMDb, TMDb). Adding a BnF "key" row would be a no-op field. The deviation is documented here so the dev agent does not chase the epic's literal list.

**Mask-display contract deviates from epic AC #3 — Guy's call to revert if desired.**

Epic AC #3 prescribes: render `••••••••<last4>` IN the input field, server detects "submitted value is byte-equal to the mask we just rendered" and skips re-write. This is the **mask-string-equality** contract.

This story ships a different contract: render the input BLANK; helper text shows `Set: ••••<last4>` or `Not set`; empty submit = no change; explicit `_clear_<key>=true` = wipe. This is the **empty-no-change-with-explicit-clear** contract.

Why the deviation:
- The mask-string-equality contract has a collision risk: if an admin's actual key happens to start with eight bullet chars (vanishingly unlikely but well-defined), the sentinel breaks.
- The mask-string-equality contract requires echoing the mask back to the browser as the input's `value=` — confusing UX (the field looks pre-filled with a value the admin didn't type and can't easily distinguish from a real edit).
- The empty-no-change contract is sentinel-free, has zero collision risk, and provides explicit-intent UX for both "leave alone" (default) and "clear this" (checkbox) without server-side string-comparison logic.

If Guy prefers the AC's literal contract for v1 — easier review, less novelty — revert by:
1. Render `••••••••<last4>` as the input's `value=` (not as helper text).
2. Drop the `_clear_<provider>` checkbox + JS handler.
3. On submit, compare `submitted_value` to the freshly-computed mask string for the stored key; if equal, skip the UPDATE; if non-empty and differs, store the new value; if empty, treat as Clear.
4. Update Task 7.2 to add `save_provider_keys_skips_unchanged_when_value_matches_mask`.
5. Drop AC#3's `_clear_<provider>` mention; replace with mask-equality detection.

The two contracts are interchangeable from an API surface perspective — same routes, same response shapes. The difference is only how the form-rendering and server-side detection work.

**Per-form save buttons partition the optimistic-locking surface — but the providers form has THREE versions.**

Each setting row has its own `version`. The Loans form holds 1 version (`overdue_threshold_version`). The Language form holds 1 version (`default_language_version`). The **Providers form holds 3 versions** because three different rows are being co-edited.

If admin A edits the overdue threshold and admin B edits Google Books at the same time, both succeed — no collision (different rows). If admin A and admin B both edit Google Books at the same time, the second one's POST hits version-stale → 409 → "Google Books was modified by another admin — reload and retry". The 409 message identifies which provider was stale, not which form (precision aids the recover-and-retry UX).

The handler's transaction-wrap-and-rollback semantics: if the admin edits ALL THREE provider keys in one form submission and the OMDb row is stale, all three rollback (no partial save). The admin reloads, re-enters the OMDb-relevant change, and resubmits. Submitting the unchanged Google Books and TMDb fields a second time is no-ops (per the empty-submit-no-change contract).

**Settings cache reload — re-SELECT then write-lock-swap.**

After the transaction commits, the handler:

1. Calls `AppSettings::load_from_db(&state.pool).await` — a SELECT fetching ALL setting rows. This is a synchronous-after-completion DB call; no locks held during the `.await`.
2. Acquires `state.settings.write()` — synchronous, no `.await` while held — and writes the new `AppSettings` into the `Arc<RwLock<AppSettings>>`.
3. Drops the write guard.
4. Returns the success FeedbackEntry.

**Why re-SELECT vs in-place struct update**: re-SELECT re-uses the existing loader's validation logic (clamping, fallback-on-invalid, log warns) — every save re-validates the entire settings table, surfacing any external corruption (e.g., a manual SQL edit gone wrong) on the next save. The DB roundtrip cost is negligible (~0.5ms locally) and is paid only on save, not on read.

**Reader-vs-writer race**: a request that started before the write-lock-swap will see the OLD `AppSettings` value (it cloned the scalar earlier at its own `read()` call). A request that arrives after the swap sees the NEW value. There is no "inconsistent half-and-half" state because individual scalars are owned by the `AppSettings` struct; the swap is a pointer-replace. The "next request" guarantee from AR9 is satisfied exactly: the very next request to read AppSettings sees the new value.

A specific case worth naming: an in-flight metadata fetch through (say) `OmdbProvider` has already cloned the OLD `omdb_api_key` out of the read lock at its first sync line and is waiting on an HTTP response. A concurrent admin save that changes the OMDb key swaps the cache. The in-flight fetch completes against the OLD key (its HTTP request was already in flight). The next fetch through OMDb reads the NEW key and uses it. This is correct semantics — there is no "abort and retry" path needed.

**Refactor providers to read keys per-fetch — register unconditionally.**

Today, OMDb and TMDb providers are NOT registered if their env var is absent (`main.rs:138-147`). The chain of `MetadataProvider` impls in `provider.rs` simply doesn't include them. Story 8-5 must accept "next request takes effect without restart" — meaning if the admin sets the OMDb key for the first time, the OMDb provider must serve fetches on the next request. Two ways:

1. **Re-register the provider on first save.** Requires mutable `Arc<ProviderRegistry>` — currently it's `Arc<ProviderRegistry>` with no Mutex (read-only). Adding a Mutex around the registry to support runtime mutation is a wider change with downstream concurrency surprises (every fetch path takes the lock).

2. **Register all three providers unconditionally; their fetch methods read the key from `AppSettings` per-call and short-circuit to "no result, skip me" if the key is empty.** No registry mutation. The fetch-time read is a single-digit-ns RwLock read. The "skip me" return must match the existing semantic (whatever `MetadataResult::NotFound` or equivalent the codebase uses for "this provider has nothing"); confirm by reading the existing OMDb / TMDb provider impls.

**Decision: option 2.** It's the smaller blast radius and matches the existing trait surface. The provider's `fetch()` (or whatever it's called — verify with the trait) gets one new line: `let key = match self.settings.read().ok().and_then(|s| if s.<field>.is_empty() { None } else { Some(s.<field>.clone()) }) { Some(k) => k, None => return Ok(<empty result>) };`. Done.

**Provider-health pings (story 8-1) are unaffected.** They use `health_check_url()` (no API key needed) — see `src/tasks/provider_health.rs`. So unconditional provider registration doesn't break the Health tab's per-provider color indicator.

**`hx-confirm=` allowlist stays at 5.**

Settings saves are non-destructive — they replace one editable value with another. No confirmation prompt is needed. `cargo test templates_audit::hx_confirm_matches_allowlist` passes with `len == 5` unchanged.

**`forms_include_csrf_token` audit — three new POST forms covered.**

Each of the Loans, Providers, Language forms is a `<form method="POST">`. Each MUST include `<input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">` as one of its first children. The audit at `src/templates_audit.rs:302-394` will fail `cargo test` if any one is missed.

**`base_context()` helper still deferred** (same as 8-3 / 8-4). Each new template struct flattens the common fields manually.

**No `CSRF_EXEMPT_ROUTES` additions.**

**Default-language ENUM scope.**

`default_language: String` accepts the literal set `{"fr", "en"}`. Validation at the loader (silent fallback on invalid + log warn) and at the form handler (400 BadRequest on invalid input). UI offers exactly two radios. If Guy ever wants Spanish, that's a new story (locale files, all the t! key translations, etc.) — beyond this story.

**Provider names are not translated.**

The strings `"Google Books"`, `"OMDb"`, `"TMDb"` interpolated into i18n keys via `%{provider}` are product names — they pass through `t!()` calls verbatim regardless of locale. Don't add `admin.system.provider_name_google_books` keys; don't `t!()` the names anywhere. Same convention as reference data values per NFR41.

**Env-var migration is one-shot per boot and durable across Clear actions.**

`migrate_legacy_env_vars(pool)` runs exactly once per process boot, called from `main.rs` after migrations and before the first `AppSettings::load_from_db`. It is NOT called from `load_from_db`, so it does NOT run on every settings save. After the first run, the rows hold whatever values the migration wrote.

If an admin later Clears a key whose env var is still set in `docker-compose.yml`, the row is empty + the env var is non-empty — but **the next boot WILL re-migrate the env-var value back into the row**. This is a design choice: the env vars represent the operator's deployment-time intent; if Guy actually wants the key permanently cleared, he should also remove the env var from `docker-compose.yml` (the follow-up `type:change-request` issue tracks this cleanup). For this story's lifetime, "Clear from the UI" stays cleared until the next process restart; if the env var is also gone by then, Clear is durable forever.

The migration's `tracing::info!` log makes its execution observable on every boot where it actually writes a row.

## Acceptance Criteria

1. **System tab renders three groups — Loans, Metadata Providers, Language — each with its own form and Save button.**
   - `/admin?tab=system` as admin renders the full admin page with the System tab pre-selected and the panel server-rendered (non-HTMX path); as librarian → 403; as anonymous → 303 → `/login?next=%2Fadmin%3Ftab%3Dsystem`.
   - HTMX click on the System tab from another admin tab GETs `/admin/system` and swaps only the panel fragment into `#admin-shell`, with `hx-push-url="/admin?tab=system"`.
   - The panel renders in this order: § Loans → § Metadata providers → § Language. Each section has its own heading (localized) and its own `<form method="POST">` with a stable `id` attribute: `id="admin-system-loans-form"`, `id="admin-system-providers-form"`, `id="admin-system-language-form"`.
   - Each form's submit button declares `hx-target="#<form-id>"` and `hx-swap="outerHTML"` so the server's response fragment swaps the form in place (preserving section order, OOB-targeting `#feedback-list` for the success FeedbackEntry).
   - Each form includes `<input type="hidden" name="_csrf_token" value="{{ csrf_token|e }}">` as its first input.

2. **Loans form — overdue threshold (integer days, ≥ 1, default 30).**
   - Single labeled input: "Overdue loan threshold (days)" / FR: "Seuil de retard de prêt (jours)". Pre-filled with the current `overdue_threshold_days` value.
   - Hidden `overdue_threshold_version` input from the SELECTed row's `version`.
   - Submit button: "Save loans settings" / FR: "Enregistrer les prêts".
   - POST `/admin/system/loans` with `LoansSettingsForm`.
   - Validation:
     - `overdue_threshold_days < 1` → 400 + `error.system.overdue_threshold_invalid` ("Threshold must be ≥ 1 day"); form re-renders with the invalid value preserved.
     - `overdue_threshold_days > 365` → 400 + `error.system.overdue_threshold_too_large` ("Threshold must be ≤ 365 days") — sanity cap.
     - Optimistic-lock conflict (rows_affected = 0) → 409 + `error.system.version_mismatch` ("Settings were modified by another admin — reload and retry").
   - On success: UPDATE the row (`SET setting_value = ?, version = version + 1`), reload `AppSettings` cache, return updated form fragment + success FeedbackEntry "Loans settings saved" / FR: "Préférences de prêt enregistrées".
   - **Live behavior**: navigating to `/loans` immediately after the save reads the new threshold via `AppState::settings.read()` — no restart needed. (Test via E2E.)

3. **Metadata Providers form — three keyed providers, mask-display + empty-no-change + explicit-clear contract.**
   - Three labeled rows, one per provider (in this order — matches the `metadata/` registration order for consistency):
     1. Google Books — input `google_books_api_key` (text, autocomplete=off, blank by default).
     2. OMDb — input `omdb_api_key`.
     3. TMDb — input `tmdb_api_key`.
   - Each row:
     - Input is BLANK on render (regardless of stored value).
     - Helper text below the input:
       - If the stored value is non-empty: `Set: ••••<last4>` / FR: `Définie : ••••<last4>` — `<last4>` is the last 4 chars of the stored key. Computed server-side; the unmasked key is never sent to the browser.
       - If empty: `Not set` / FR: `Non définie`.
     - "Clear" checkbox labeled `Clear this key on save` / FR: `Effacer cette clé à l'enregistrement`. Checking it disables the text input via JS (`data-action="provider-key-clear-toggle"`), giving visual confirmation that the field will be wiped.
     - Hidden `<provider>_version` input from the SELECTed row's `version`.
   - Submit button: "Save provider keys" / FR: "Enregistrer les clés".
   - POST `/admin/system/providers` with `ProviderKeysForm`.
   - Submission semantics — for each of the three providers:
     - Input empty AND `_clear_<provider>` not checked → **NO CHANGE** (handler skips this provider's UPDATE entirely; version is not bumped; no version-check is performed for this provider — the admin didn't touch it).
     - Input empty AND `_clear_<provider>` checked → **EXPLICIT CLEAR** (UPDATE settings SET setting_value = '', version = version + 1 WHERE setting_key = ? AND version = ?).
     - Input non-empty (whitespace-trimmed, then non-empty) → **SET NEW** (UPDATE settings SET setting_value = ?, version = version + 1 WHERE setting_key = ? AND version = ?).
     - Input non-empty AND `_clear_*` checked → **EXPLICIT CLEAR wins** (the box is the explicit signal; treat as clear). Optional: 400 with "Conflicting input — clear AND set new key" if Guy wants stricter UX. Default to "clear wins" to keep things forgiving.
   - All three updates run in a single transaction; if any one's optimistic-lock check fails, the entire transaction rolls back and the response is 409 + `error.system.provider_version_mismatch` with `provider=<google_books|omdb|tmdb>` interpolated so the admin knows which one is stale.
   - On success: reload `AppSettings` cache, return updated form fragment + success FeedbackEntry per affected provider ("Google Books key saved" / "OMDb key cleared" / etc.). When the form had no actual changes (all three were "no change"), still return success with feedback "No changes" / FR: "Aucune modification" rather than silently reloading.
   - **Live behavior**: the very next metadata fetch through one of the three providers reads the new key from `AppSettings`. No restart; no provider re-instantiation. (Test via unit test: directly invoke `provider.fetch(...)` after a save and assert the request used the new key — mock the HTTP client.)

4. **Language form — default language (FR / EN radio).**
   - Two-radio group labeled "Default language" / FR: "Langue par défaut". Radio values: `fr`, `en`. Pre-selected radio matches the current `default_language`.
   - Hidden `default_language_version` input.
   - Submit button: "Save language settings" / FR: "Enregistrer la langue".
   - POST `/admin/system/language` with `LanguageSettingsForm`.
   - Validation:
     - `default_language` not in `{"fr", "en"}` → 400 + `error.system.default_language_invalid`.
     - Optimistic-lock conflict → 409 + `error.system.version_mismatch`.
   - On success: UPDATE, reload cache, return updated form fragment + success FeedbackEntry "Default language saved" / FR: "Langue par défaut enregistrée".
   - **Live behavior**:
     - An anonymous user with NO `lang=` cookie AND no Accept-Language match hits any page → sees the new default language.
     - An anonymous user WITH a `lang=` cookie → cookie wins; default change has no effect for them.
     - An authenticated user with `users.preferred_language` set → user-pref wins; default change has no effect.
     - An anonymous user with Accept-Language header that matches `fr` or `en` → header wins (story 7-3 behavior); default change has no effect for them either.
     - The "default" only kicks in when ALL prior chain steps return None — i.e., a fresh visitor with a non-FR/EN Accept-Language (e.g., `de`), no cookie, no session.

5. **Cache reload on save — `Arc<RwLock<AppSettings>>` swap (AR9).**
   - After every successful save (any of the three forms), the handler calls `AppSettings::load_from_db(&state.pool).await` to fetch the fresh settings from the DB and writes them into `*state.settings.write().unwrap()`.
   - The write lock is acquired AFTER the load (no `.await` while the lock is held) and is held for a single pointer-write before being dropped.
   - On reload failure (DB unreachable mid-save — extremely unlikely since the UPDATE just succeeded): log error, return 500. The DB row is updated; the cache is stale until the next successful reload OR a process restart. This is the only "failure to reload" path and it's acceptable because (a) it's extremely rare, (b) the next save retry will re-reload, (c) auto-recovery on container restart is unaffected.
   - **No restart is needed for the new value to take effect on subsequent requests.** This is the load-bearing AR9 contract.

6. **Optimistic locking — per-row `version` partitions concurrency.**
   - Each setting row has its own `version` column. Concurrent edits to different settings (e.g., admin A edits the threshold, admin B edits the Google Books key) DO NOT collide.
   - Concurrent edits to the same setting (two admin tabs both load `version=5`, both submit `version=5`) → first wins, second gets 409 with the localized "Settings were modified by another admin — reload and retry" message.
   - 409 messages identify the specific setting that was stale (e.g., "Google Books was modified...") for precise recovery UX.

7. **API keys stored plaintext (NFR37).**
   - Stored verbatim in `settings.setting_value` (TEXT column). No encryption. No hashing. No truncation.
   - Documented in `architecture.md` Configuration Architecture as an accepted trade-off for the home-NAS deployment context (NFR37: no telemetry, no cloud sync — the threat model excludes a remote attacker, so encryption-at-rest provides marginal defense against an attacker who already has DB read access).

8. **One-shot env-var migration at boot — durable across admin Clear actions.**
   - `pub async fn migrate_legacy_env_vars(pool: &DbPool)` lives in `src/config.rs` (separate from `load_from_db`). It's called from `main.rs` exactly once per boot, after migrations and before the first `AppSettings::load_from_db`.
   - For each of `(GOOGLE_BOOKS_API_KEY, google_books_api_key)`, `(OMDB_API_KEY, omdb_api_key)`, `(TMDB_API_KEY, tmdb_api_key)`: if the env var is set non-empty AND the row's current value is empty, UPDATE the row to the env-var value and `tracing::info!` the migration.
   - The migration is NOT called from `load_from_db`. A settings save that reloads the cache does NOT re-run the migration. An admin who Clears a key from the UI keeps it cleared for the lifetime of the process even if the env var is still defined in `docker-compose.yml`.
   - On the next process boot, if the env var is still set AND the row is empty (because the admin Cleared it), the migration WILL re-populate the row — this is the documented design choice ("operator's deployment-time intent wins on boot"); making Clear durable across reboots requires also removing the env var from `docker-compose.yml` (tracked as a follow-up `type:change-request` GitHub issue).

9. **Admin role gating + CSRF.**
   - Every handler starts with `session.require_role_with_return(Role::Admin, &return_path)?` — Anonymous → 303, Librarian → 403.
   - Every POST route is automatically CSRF-protected by 8-2 middleware. No allowlist additions.
   - Tampering the CSRF token in DevTools → 403 + the 8-2 "Session expired" FeedbackEntry. (Coverage in E2E.)

10. **CSP compliance — zero inline event handlers.**
    - The "Clear" checkbox toggle behavior wires through a `data-action="provider-key-clear-toggle"` delegated handler in `static/js/mybibli.js`.
    - Zero inline `<style>` / `style=""` / `onclick=` in the new templates.
    - `cargo test templates_audit` passes with the new templates in place.

11. **`hx-confirm=` allowlist stays at 5 — no destructive prompts in this story.**

12. **i18n keys — EN + FR present for every new key.**
    - All new keys live under `admin.system.*`, `success.system.*`, `error.system.*`. See §Tasks point 5 for the full list.
    - Post-YAML-edit: `touch src/lib.rs && cargo build`.
    - No EN-only fallbacks.

13. **Provider refactor — three keyed providers read API keys from `AppSettings` per fetch.**
    - `GoogleBooksProvider`, `OmdbProvider`, `TmdbProvider` each accept `Arc<RwLock<AppSettings>>` at construction; they store the handle, not the key.
    - Each provider's `fetch()` (or whichever method initiates an HTTP request) reads its key on the first sync line via the `AppState::*_api_key()` helper (or, since providers don't have AppState, the equivalent `self.settings.read().ok().and_then(...)` pattern). Empty key → return the existing "no result" / "skip me" path WITHOUT making the HTTP call.
    - All three providers are now registered UNCONDITIONALLY in `main.rs` (the env-var-presence gate at lines 138-147 is removed). The "skip me when key empty" check happens at fetch time inside the provider, not at registration time.
    - Provider-health pings (story 8-1) continue to work — they don't use API keys.

14. **Unit tests** — see §Tasks point 6.

15. **E2E test passes** — see §Tasks point 7. Includes a smoke test for the new admin journey (Foundation Rule #7).

16. **Documentation updated** — `CLAUDE.md`, `docs/route-role-matrix.md`, `architecture.md`.

## Tasks / Subtasks

- [ ] **Task 1: Migration + AppSettings extension + helpers (AC: 1, 2, 3, 4, 5, 8)**
  - [ ] 1.1 Create `migrations/<DATE>_seed_system_settings_rows.sql` with `INSERT IGNORE INTO settings ...` for the four new rows (`default_language='fr'`, three empty API keys).
  - [ ] 1.2 Extend `AppSettings` struct in `src/config.rs:154-166` with `default_language: String`, `google_books_api_key: String`, `omdb_api_key: String`, `tmdb_api_key: String`. Update `Default` impl with sensible defaults (lang=`"fr"`, keys empty).
  - [ ] 1.3 Add four new arms to `AppSettings::load_from_db` match block (`src/config.rs:181-280`) for the four new keys. The `default_language` arm validates against `{"fr", "en"}` (warn-and-fallback on invalid, matching the existing per-key warn pattern). Do NOT add env-var migration logic to `load_from_db` — see 1.4.
  - [ ] 1.4 Add `pub async fn migrate_legacy_env_vars(pool: &DbPool) -> Result<(), sqlx::Error>` in `src/config.rs` (separate function, NOT inside `load_from_db`). Three iterations over (env_var, key) tuples; each one (a) checks the env var is set non-empty, (b) SELECTs the current row value, (c) UPDATEs only if the row is empty, (d) logs `tracing::info!` on a write.
  - [ ] 1.5 Wire the migration into `src/main.rs` boot sequence: AFTER `MIGRATOR.run(&pool).await?` (or wherever migrations execute) and BEFORE the first `AppSettings::load_from_db(&pool).await` call. Single line: `migrate_legacy_env_vars(&pool).await?;`. The order matters because `load_from_db` then picks up whatever values the migration wrote.
  - [ ] 1.6 Add four helper methods on `AppState` in `src/lib.rs` (after `session_timeout_secs()`): `default_language() -> String`, `google_books_api_key() -> Option<String>`, `omdb_api_key() -> Option<String>`, `tmdb_api_key() -> Option<String>`. Pattern: clone scalar out of `read()`, return Option-wrapping for the three keys.
  - [ ] 1.7 Run `cargo sqlx prepare` to regenerate `.sqlx/` after any new query landings, then `cargo sqlx prepare --check --workspace -- --all-targets` to verify the offline cache before commit.

- [ ] **Task 2: Provider refactor (AC: 13)**
  - [ ] 2.1 Read the existing `GoogleBooksProvider::new()` signature (likely `pub fn new(http_client: reqwest::Client, api_key: Option<String>)` per the codebase explorer's report, but verify in the impl). Replace the `api_key` parameter with `settings: Arc<RwLock<AppSettings>>`. Store the handle in the struct as `settings: Arc<RwLock<AppSettings>>`.
  - [ ] 2.2 Read the existing `GoogleBooksProvider` fetch method to identify its name (likely on the `MetadataProvider` trait) and its current "no key" early-return shape — match it exactly. Update the method to read its key on the first sync line (before any `.await`): `let key = self.settings.read().ok().and_then(|s| if s.google_books_api_key.is_empty() { None } else { Some(s.google_books_api_key.clone()) });`. On `None`, return the existing "no key" path verbatim.
  - [ ] 2.3 Same for `OmdbProvider` (`s.omdb_api_key`) and `TmdbProvider` (`s.tmdb_api_key`). Verify each provider's existing constructor signature and trait-method name in the impl before touching them; mirror the "no key" early-return path that those providers use today.
  - [ ] 2.4 Update `src/main.rs:115-153`:
    - Remove the env-var reads for the three keys (`std::env::var("GOOGLE_BOOKS_API_KEY")`, `OMDB_API_KEY`, `TMDB_API_KEY`).
    - Pass the `Arc<RwLock<AppSettings>>` (named `settings_arc` or whatever the variable is in `main.rs`'s `AppState` construction order) to each of the three providers.
    - Register OMDb and TMDb unconditionally — remove the `if let Ok(...)` branches at lines 138-147. Register them like the others (just the `registry.register(Box::new(OmdbProvider::new(http_client.clone(), settings_arc.clone())));` line).
    - **Order of operations**: ensure `AppSettings::load_from_db(&pool).await` runs BEFORE the registry is built, so the providers receive the populated settings. Re-check the existing order; reshuffle if needed (settings → providers → AppState).
  - [ ] 2.5 Provider unit tests: `#[tokio::test]` (or `#[sqlx::test]`) for each of the three keyed providers — assert that with empty key the fetch returns the no-key result without HTTP call (mock the http client to assert no request was made), and with non-empty key the fetch issues an HTTP call with the correct Authorization or query-param shape.

- [ ] **Task 3: Locale-middleware default-language wiring (AC: 4)**
  - [ ] 3.1 Update `src/middleware/locale.rs:75-81` — replace hardcoded `"fr"` with `&state.default_language()` (or whatever cleanest call-shape works).
  - [ ] 3.2 If `resolve_locale` in `src/i18n/resolve.rs:19-42` takes `default: &'static str`, change to `default: &str`. Update all callers (probably just the one in locale.rs). The function's matching logic doesn't need any other changes.
  - [ ] 3.3 Unit test in `src/middleware/locale.rs`: with no cookie, no session, no Accept-Language match, and AppSettings default = "en" → resolved locale is "en"; with default = "fr" → "fr".

- [ ] **Task 4: New module `src/routes/admin_system.rs` (AC: 1–6, 9)**
  - [ ] 4.1 Create the file. Add `pub mod admin_system;` to `src/routes/mod.rs`.
  - [ ] 4.2 Define `LoansSettingsForm`, `ProviderKeysForm`, `LanguageSettingsForm` `#[derive(Deserialize)]` structs at the top.
  - [ ] 4.3 Define template structs (Askama) for the panel (`AdminSystemPanelTemplate` — replaces 8-1's `AdminSystemPanel` stub) and three section fragments (`AdminSystemLoansForm`, `AdminSystemProvidersForm`, `AdminSystemLanguageForm`). Each carries the flattened common fields (`lang`, `role`, `current_page`, `skip_label`, nav labels, `csrf_token`, `admin_tabs_*`, `trash_count`) plus the section-specific fields (current values, version numbers, masked-helper-text strings).
  - [ ] 4.4 Implement `admin_system_panel(State, Session, HxRequest, OriginalUri) -> Result<Response, AppError>` — replaces the 8-1 stub at `src/routes/admin.rs:932-941`. Issue a **single** SELECT pulling the 5 relevant rows: `SELECT setting_key, setting_value, version FROM settings WHERE setting_key IN ('overdue_loan_threshold_days', 'default_language', 'google_books_api_key', 'omdb_api_key', 'tmdb_api_key') AND deleted_at IS NULL`. Collect into a `HashMap<String, (String, i32)>` keyed on `setting_key`. For values, you can also read `state.settings.read()` (the cache holds the same values) — but the **versions** must come from the SELECT (the cache doesn't carry per-row versions). Construct the panel template (with masked helper-text computed via `mask_key`); render full page or fragment depending on `HxRequest`.
  - [ ] 4.5 Implement `save_loans_settings` (POST `/admin/system/loans`) — validate, `save_setting`, reload cache, return updated section + success feedback.
  - [ ] 4.6 Implement `save_provider_keys` (POST `/admin/system/providers`) — for each of the three providers, determine action (no-change / clear / set), call `save_setting` for changes only, all in one transaction. Reload cache. Return updated section + per-provider feedback.
  - [ ] 4.7 Implement `save_language_settings` (POST `/admin/system/language`) — validate against `{"fr", "en"}`, save, reload, feedback.
  - [ ] 4.8 Add private helpers in the module: `save_setting(...)` (UPDATE + locking check), `reload_settings_cache(...)` (re-SELECT + write-lock-swap), `mask_key(value: &str) -> Option<String>` (returns `Some("••••<last4>")` if the value is non-empty — just the mask portion, no `Set:` prefix; the i18n key `admin.system.provider_key_set` interpolates `%{mask}` so the prefix lives in the localized string. Returns `None` if value is empty so the template can `t!("admin.system.provider_key_not_set")` instead), `validate_overdue_threshold(i32) -> Result<(), AppError>` (range check 1..=365).
  - [ ] 4.9 Each handler's first line: `let return_path = build_return_path(&uri); session.require_role_with_return(Role::Admin, &return_path)?;`. (Reuse the existing helper from `admin.rs` if it's `pub`; otherwise duplicate the 3-line build_return_path locally.)
  - [ ] 4.10 Delete the old `admin_system_panel` stub handler at `src/routes/admin.rs:932-941` and the `AdminSystemPanel` template struct at lines 385-388. Ensure `render_panel` in `admin.rs` routes `AdminTab::System` to the new module's renderer (small wiring change in the `match tab` block).
  - [ ] 4.11 **Update the route table in `src/routes/mod.rs:235`** — change `axum::routing::get(admin::admin_system_panel)` to `axum::routing::get(admin_system::admin_system_panel)`. Add three new POST routes immediately below: `/admin/system/loans` → `admin_system::save_loans_settings`, `/admin/system/providers` → `admin_system::save_provider_keys`, `/admin/system/language` → `admin_system::save_language_settings`. None of the four are added to `CSRF_EXEMPT_ROUTES` in `src/middleware/csrf.rs`.
  - [ ] 4.12 Update `docs/route-role-matrix.md` — 3 new rows (POST routes) under Admin role + 1 updated row (the existing GET keeps its row, just the handler reference changes). All CSRF-protected.

- [ ] **Task 5: Templates — panel replacement + three new forms (AC: 1, 2, 3, 4, 9, 10)**
  - [ ] 5.1 **REPLACE** `templates/fragments/admin_system_panel.html` (currently a 4-line stub from 8-1) with three `<section>` blocks, one per group, each `{% include %}`-ing the corresponding form fragment. Update the comment header from "Replaced by story 8-4" → "Replaced by story 8-5".
  - [ ] 5.2 Create `templates/fragments/admin_system_loans_form.html` — `<form id="admin-system-loans-form" method="POST" hx-post="/admin/system/loans" hx-target="#admin-system-loans-form" hx-swap="outerHTML">`. Single labeled input + hidden `overdue_threshold_version` + hidden `_csrf_token` (FIRST input) + Save button.
  - [ ] 5.3 Create `templates/fragments/admin_system_providers_form.html` — `<form id="admin-system-providers-form" method="POST" hx-post="/admin/system/providers" hx-target="#admin-system-providers-form" hx-swap="outerHTML">`. Three labeled rows (one per provider), each with: text input (autocomplete=off, blank), helper text below (`{% if mask %}{{ "admin.system.provider_key_set"|t(mask=mask) }}{% else %}{{ "admin.system.provider_key_not_set"|t }}{% endif %}` style — match Askama's actual i18n call shape), "Clear" checkbox (`data-action="provider-key-clear-toggle"`), hidden `<provider>_version` field. Single hidden `_csrf_token` (FIRST input). Single Save button.
  - [ ] 5.4 Create `templates/fragments/admin_system_language_form.html` — `<form id="admin-system-language-form" method="POST" hx-post="/admin/system/language" hx-target="#admin-system-language-form" hx-swap="outerHTML">`. Two radios (FR / EN) + hidden `default_language_version` + hidden `_csrf_token` (FIRST input) + Save button.
  - [ ] 5.5 Each handler's success response is the updated form fragment (with bumped version number embedded) PLUS an OOB FeedbackEntry targeting `#feedback-list` with `hx-swap-oob="beforeend"` (mirroring the 8-3 admin-users pattern).
  - [ ] 5.6 Zero inline `style=""` / `onclick=` / inline `<script>`/`<style>` — all interactivity via `data-action="..."`.
  - [ ] 5.7 Add the JS handler for `provider-key-clear-toggle` to `static/js/mybibli.js` after the existing `dismiss-feedback` handler. ~10 lines: on checkbox change, toggle the sibling text input's `disabled` attribute and clear its value if disabling.

- [ ] **Task 6: i18n keys — EN + FR (AC: 12)**
  - [ ] 6.1 Add `admin.system.*` and `success.system.*` and `error.system.*` keys to `locales/en.yml` and `locales/fr.yml`. Minimum set:
    ```yaml
    admin:
      system:
        panel_heading: "System settings"               # FR: "Paramètres système"
        section_loans: "Loans"                         # FR: "Prêts"
        section_providers: "Metadata providers"        # FR: "Fournisseurs de métadonnées"
        section_language: "Language"                   # FR: "Langue"
        overdue_threshold_label: "Overdue loan threshold (days)"
                                                        # FR: "Seuil de retard de prêt (jours)"
        overdue_threshold_help: "A loan is flagged overdue when its age exceeds this number of days."
                                                        # FR: "Un prêt est signalé en retard quand son âge dépasse ce nombre de jours."
        provider_key_label_google_books: "Google Books API key"
                                                        # FR: "Clé d'API Google Books"
        provider_key_label_omdb: "OMDb API key"        # FR: "Clé d'API OMDb"
        provider_key_label_tmdb: "TMDb API key"        # FR: "Clé d'API TMDb"
        provider_key_set: "Set: %{mask}"               # FR: "Définie : %{mask}"
        provider_key_not_set: "Not set"                # FR: "Non définie"
        provider_key_clear_label: "Clear this key on save"
                                                        # FR: "Effacer cette clé à l'enregistrement"
        default_language_label: "Default language"     # FR: "Langue par défaut"
        default_language_help: "Used for new anonymous visitors with no cookie and no Accept-Language match."
                                                        # FR: "Utilisée pour les nouveaux visiteurs anonymes sans cookie ni Accept-Language correspondant."
        btn_save_loans: "Save loans settings"          # FR: "Enregistrer les prêts"
        btn_save_providers: "Save provider keys"       # FR: "Enregistrer les clés"
        btn_save_language: "Save language settings"    # FR: "Enregistrer la langue"
    success:
      system:
        loans_saved: "Loans settings saved"            # FR: "Préférences de prêt enregistrées"
        providers_saved: "Provider keys saved"         # FR: "Clés de fournisseur enregistrées"
        language_saved: "Default language saved"       # FR: "Langue par défaut enregistrée"
        no_changes: "No changes"                       # FR: "Aucune modification"
        provider_set: "%{provider} key saved"          # FR: "Clé %{provider} enregistrée"
        provider_cleared: "%{provider} key cleared"    # FR: "Clé %{provider} effacée"
    error:
      system:
        overdue_threshold_invalid: "Threshold must be ≥ 1 day"
                                                        # FR: "Le seuil doit être ≥ 1 jour"
        overdue_threshold_too_large: "Threshold must be ≤ 365 days"
                                                        # FR: "Le seuil ne peut dépasser 365 jours"
        default_language_invalid: "Language must be FR or EN"
                                                        # FR: "La langue doit être FR ou EN"
        version_mismatch: "Settings were modified by another admin — reload and retry"
                                                        # FR: "Les paramètres ont été modifiés par un autre administrateur — rechargez et réessayez"
        provider_version_mismatch: "%{provider} was modified by another admin — reload and retry"
                                                        # FR: "%{provider} a été modifié par un autre administrateur — rechargez et réessayez"
    ```
  - [ ] 6.2 `touch src/lib.rs && cargo build` to force rust-i18n proc-macro re-read.
  - [ ] 6.3 Optionally remove the `admin.placeholder.coming_in_story` stub for the System tab — but the key may still be used by 8-8 (setup wizard). Check; leave the key if other tabs still reference it.

- [ ] **Task 7: Unit tests (AC: 14)**
  - [ ] 7.1 `#[sqlx::test]` block in `src/config.rs` (or `tests/config_settings.rs`):
    - `load_from_db_picks_up_default_language_row`: seed a row `('default_language', 'en')` → assert `settings.default_language == "en"`.
    - `load_from_db_picks_up_api_keys`: seed three key rows → assert correct field population.
    - `load_from_db_falls_back_to_default_on_invalid_language`: seed `('default_language', 'es')` → assert `settings.default_language == "fr"` and a warn was logged.
    - `env_var_migration_writes_row_when_empty`: set `GOOGLE_BOOKS_API_KEY=test123` env var, seed empty `google_books_api_key` row, call `migrate_legacy_env_vars(pool)` → assert row updated to `'test123'`, assert info log line. Then call `load_from_db` → assert `settings.google_books_api_key == "test123"`.
    - `env_var_migration_no_op_when_row_set`: seed `google_books_api_key='already_here'`, set env var to a different value, call `migrate_legacy_env_vars(pool)` → assert row unchanged.
    - `env_var_migration_no_op_when_env_var_unset`: seed empty row, no env var → assert row stays empty, no log line.
    - `load_from_db_does_not_re_migrate_after_admin_clear`: this is the regression test for the "Clear-then-reload-with-env-var-still-set" scenario. Seed `google_books_api_key='cleared_by_admin'`, call `load_from_db` → assert value is `"cleared_by_admin"`. Then UPDATE the row to empty (simulating the admin's Clear), set `GOOGLE_BOOKS_API_KEY=lingering` env var, call `load_from_db` again → assert `settings.google_books_api_key == ""` (loader does NOT re-migrate; that's the migration function's job and it only runs at boot). Assert no migration log line during this `load_from_db` call.
    - `migrate_legacy_env_vars_re_migrates_on_next_boot_after_clear`: companion to the above — explicit assertion that `migrate_legacy_env_vars` WILL re-migrate the env-var value on the next process boot (because that's the design choice documented in Cross-cutting decisions). Sequence: seed empty row, set env var, call `migrate_legacy_env_vars` → asserts row populated. UPDATE row to empty (simulate Clear), call `migrate_legacy_env_vars` again (simulate next boot) → asserts row populated AGAIN from env var. This codifies the "operator's deployment-time intent wins on boot" semantics.
  - [ ] 7.2 `#[sqlx::test]` block in `src/routes/admin_system.rs::tests` (or `tests/admin_system.rs`):
    - `save_loans_settings_updates_row_and_reloads_cache`: load AppState, save threshold=42, assert SELECT shows new value AND `state.settings.read().overdue_threshold_days == 42`.
    - `save_loans_settings_rejects_zero_and_negative`: → 400 with `error.system.overdue_threshold_invalid`. **Also assert the DB row is unchanged** after the rejection (`SELECT setting_value FROM settings WHERE setting_key = 'overdue_loan_threshold_days'` returns the pre-rejection value) — directly tests the AC's "stored value stays" contract.
    - `save_loans_settings_rejects_above_365`: → 400 with `error.system.overdue_threshold_too_large`. **Also assert DB row unchanged.**
    - `save_loans_settings_409_on_stale_version`: load version=5, simulate concurrent update bumping version to 6, save with version=5 → 409.
    - `save_provider_keys_no_change_path`: submit all three inputs empty, no `_clear_*` checked → no UPDATEs issued, version unchanged, response success "No changes".
    - `save_provider_keys_set_one_clear_one`: empty Google Books + clear=true; new OMDb value; empty TMDb + clear=false → assert Google Books row updated to `''`, OMDb row updated to new value, TMDb row unchanged.
    - `save_provider_keys_409_partial`: simulate stale version on TMDb; assert all three rollback (Google Books and OMDb DID NOT update either).
    - `save_provider_keys_clear_wins_over_set`: input non-empty + `_clear_*=true` → assert row stored as empty (clear wins).
    - `save_language_settings_updates_and_reloads`: save `default_language='en'`, assert cache reload picks it up.
    - `save_language_settings_rejects_invalid`: post `default_language='es'` → 400. **Also assert DB row unchanged.**
    - `librarian_gets_403_on_all_4_routes`, `anonymous_gets_303_on_all_4_routes`.
  - [ ] 7.3 Provider unit tests (per Task 2.5).
  - [ ] 7.4 Locale middleware test (per Task 3.3).
  - [ ] 7.5 Run `cargo test system:: admin_system config_settings` and `cargo test --lib` — all green.

- [ ] **Task 8: E2E spec — `tests/e2e/specs/admin/system-settings.spec.ts` (AC: 15)**
  - [ ] 8.1 Spec ID `"SY"` — confirm no collision via `grep -rn 'specIsbn("SY"' tests/e2e/`. Reserved spec IDs in use (per the codebase explorer): BL, BT, CC, CI, CM, CS, CT, CV, DC, ES, LN, LR, LS, ME, MT, SE, SG, SH, UA (8-3), RD (8-4), XC. `SY` is free.
  - [ ] 8.2 **Foundation Rule #7 smoke path:** blank browser → `loginAs(page, "admin")` → navigate `/admin?tab=system` → assert all three sections render → in the Loans section, change threshold from 30 to 14 → click Save → assert success FeedbackEntry "Loans settings saved" → navigate to `/loans` → assert that a fixture loan aged 20 days NOW shows the overdue badge (it didn't at threshold=30). Change threshold back to 30 → assert the badge disappears.
  - [ ] 8.3 **Provider key save + masked redisplay:** navigate to System tab → Providers section → enter "test_key_ABCD1234" in Google Books input → Save → assert success → reload → assert input is BLANK and helper text shows "Set: ••••1234". Click "Clear" checkbox on Google Books → leave input empty → Save → assert success "Google Books key cleared" → reload → assert helper text shows "Not set".
  - [ ] 8.4 **Per-provider concurrent edit:** open the System tab in browser context A and B (`browserContext.newPage()`). In A, change Google Books to "key_A". In B (still showing version=5), change Google Books to "key_B" and Save → assert 409 + "Google Books was modified by another admin" feedback. In B, reload the page → version is now 6 → resubmit → succeeds.
  - [ ] 8.5 **Cross-group concurrency:** open System tab in A and B. In A, change overdue threshold. In B, change Google Books key. Submit BOTH at the same time (or close enough). Assert both succeed (different rows, different version columns, no collision).
  - [ ] 8.6 **Default language change affects fresh anonymous visitor only:** as admin → change default to "en" → save → log out → in a fresh `browser.newContext()` with NO cookies, set `Accept-Language: de` (so the chain can't match), navigate `/` → assert UI is in English (not French). Open a different fresh context with NO cookies and `Accept-Language: fr` → navigate `/` → assert UI is in French (Accept-Language wins over the AppSettings default). As admin again → log in → navigate to a UI page → assert the UI matches the admin's `users.preferred_language` (whatever it was in the seed) — the default change did NOT override the admin's user-level pref.
  - [ ] 8.7 **Validation errors:** post overdue threshold = 0 → assert 400 + the localized validation message inline. Post default_language = "es" → 400 + invalid-language message.
  - [ ] 8.8 **Anonymous + Librarian gating:** anonymous → `/admin?tab=system` → 303 to `/login?next=...`; librarian → `/admin?tab=system` → 403 FeedbackEntry.
  - [ ] 8.9 **CSRF tampering:** as admin → tamper meta CSRF token via `page.evaluate` → submit Loans form → 403 + "Session expired" feedback.
  - [ ] 8.10 No `waitForTimeout(...)` calls. Use DOM-state waits per CLAUDE.md.
  - [ ] 8.11 i18n-aware regex for assertions.
  - [ ] 8.12 Run `./scripts/e2e-reset.sh` → `cd tests/e2e && npm test` — 3 clean cycles.

- [ ] **Task 9: Documentation (AC: 16)**
  - [ ] 9.1 `CLAUDE.md` Key Patterns: append a "System settings (story 8-5)" bullet covering: K/V settings table; per-row `version` partitions concurrency per setting; cache-reload contract (`load_from_db` + `*write().unwrap() = new_settings`); read-on-fetch contract for the three keyed providers; env-var migration via the one-shot `migrate_legacy_env_vars(pool)` function (NOT inside `load_from_db`) — runs once per boot, durable across admin Clear actions for that process lifetime, re-migrates on the next boot if the env var is still set.
  - [ ] 9.2 `docs/route-role-matrix.md` — handled by Task 4.12 (3 new POST rows + 1 updated GET row reference, all CSRF-protected).
  - [ ] 9.3 `_bmad-output/planning-artifacts/architecture.md` Configuration Architecture (or AR9 detail): document the cache-reload model and the per-fetch key-read model for keyed providers.
  - [ ] 9.4 Open a `type:change-request` GitHub issue: "Remove deprecated env-var entries for API keys from docker-compose.yml" — links to this story's commit.

- [ ] **Task 10: Regression gate (AC: 14, 15)**
  - [ ] 10.1 `cargo check` + `cargo clippy -- -D warnings` — zero warnings.
  - [ ] 10.2 `cargo sqlx prepare --check --workspace -- --all-targets` — offline cache up to date.
  - [ ] 10.3 `cargo test` — all unit + integration green. Includes `templates_audit::*` audits.
  - [ ] 10.4 `./scripts/e2e-reset.sh` then `cd tests/e2e && npm test` — 3 clean cycles.
  - [ ] 10.5 Manual smoke: `cargo run` → navigate `/admin?tab=system` → exercise the three forms; verify the Loans threshold takes effect on `/loans` immediately; verify Google Books fetches use the new key (manually scan an ISBN that the chain would route to Google Books).

## Dev Notes

### Why three forms not one

The K/V table has independent `version` columns per setting — partitioned concurrency. A single mega-form with all 5 fields and 5 hidden version inputs WOULD work but defeats the partitioning: any concurrent edit to ANY field by another admin would fail the second admin's mega-save with no obvious recovery. Three group-scoped forms localize the conflict window and produce smaller, more targeted error messages. The UI also reads better — admins make one change at a time mentally.

### Why no "Save all" button

You'd have to either (a) post all 5 settings to one endpoint (which means re-introducing the mega-form pattern with all the conflict drawbacks), or (b) submit the three forms in sequence client-side via JS, surfacing per-form successes/failures (UX is just three Save clicks plus a JS coordinator — no real win over three independent saves). Skip.

### Why re-SELECT after UPDATE instead of in-place struct update

Re-SELECT runs `AppSettings::load_from_db` which re-validates the entire settings table — clamping ranges, falling back on invalid values, logging warnings. This is consistent with the boot-time loader (every save is a "re-boot of the settings cache"). In-place struct update would require duplicating the per-key validation logic in the save handler (DRY violation). The DB roundtrip cost is negligible (<1ms locally) and is paid only on save (rare).

### Why all three providers register unconditionally now

Today, OMDb and TMDb register only if their env var is present (`main.rs:138-147`). With keys moving to `AppSettings`, the env-var check fires before the settings are loaded (at provider construction time) — so OMDb and TMDb would NEVER register on a fresh deploy where the env vars are absent but the admin plans to set keys via the UI. Unconditional registration + per-fetch key check is the only way to support the "set key via UI on a previously-keyless deploy" flow without restart.

### Why the env-var migration is a separate function called once from `main.rs`

An earlier draft inlined the migration at the bottom of `load_from_db`. That breaks the "Clear from UI" UX: every settings save reloads the cache via `load_from_db`, and inlining the migration there means the migration re-runs on every save, silently re-populating any row the admin just Cleared (as long as the env var is still set in `docker-compose.yml`).

Pulling the migration into a separate `migrate_legacy_env_vars(pool)` function called exactly once from `main.rs` boot fixes this — Clear is durable for the process lifetime. The trade-off is documented: removing the env vars from `docker-compose.yml` is the operator's call to make the Clear durable across boots too. A `type:change-request` GitHub issue tracks that cleanup.

### Lock discipline

Never hold `state.settings.read()` or `.write()` across `.await`. The patterns:
- Read: clone the scalar out, drop the guard, then `.await`. Helpers `state.session_timeout_secs()` etc. encapsulate this.
- Write: load the new struct via `.await` (no lock held), then take `.write()` for the duration of one move-assignment, then drop. Inline.

### Provider impl change — verify the trait method shape before refactoring

The agent's report says `MetadataProvider` is a trait at `src/metadata/provider.rs:31-64` but doesn't enumerate the method signatures. Read those before touching the providers; the "no key" early-return path needs to use whatever shape the trait expects (might be `Result<Vec<MetadataResult>, _>`, might be `Result<Option<...>, _>`). Match the existing OMDb/TMDb codepaths exactly when their env-var was absent — that's the canonical "no key" return.

### File-size watch (Foundation Rule #12)

- `src/config.rs` is at ~290 lines pre-story; gains ~80 lines (4 match arms × ~10 lines, struct fields, default impl, `migrate_legacy_env_vars` function ~15 lines). Post-story: ~370. Well under 2000.
- `src/routes/admin.rs` shrinks slightly (delete `AdminSystemPanel` struct + `admin_system_panel` stub handler — ~25 lines removed).
- `src/routes/admin_system.rs` (new): ~600-800 lines.
- `src/main.rs` simplifies slightly (env-var reads + conditional registration removed; provider construction lines simplified).

### Project Structure Notes

- New file `src/routes/admin_system.rs` — same Foundation Rule #12 reason as 8-4's `admin_reference_data.rs`.
- New migration file — date-based filename per project convention.
- New JS handler — single ~10-line addition to existing `static/js/mybibli.js`, no new file.
- Three new template fragments + one rewritten panel template.
- Provider refactor — touches three files (`google_books.rs`, `omdb.rs`, `tmdb.rs`) plus `main.rs`.
- AppSettings extension — four new fields, four new helper methods.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 8.5: System settings] — full epic AC for Story 8.5
- [Source: _bmad-output/planning-artifacts/epics.md#Functional Requirements] — FR74 (overdue threshold), FR75 (provider API keys)
- [Source: _bmad-output/planning-artifacts/epics.md#Additional Requirements] — AR9 (settings cache reload)
- [Source: _bmad-output/planning-artifacts/epics.md#NonFunctional Requirements] — NFR37 (no telemetry, plaintext-key tradeoff)
- [Source: _bmad-output/implementation-artifacts/8-1-admin-shell-and-health-tab.md] — admin shell pattern, tab routing, role gating
- [Source: _bmad-output/implementation-artifacts/8-2-csrf-middleware-and-form-token-injection.md] — CSRF token contract
- [Source: _bmad-output/implementation-artifacts/8-3-user-administration.md] — closest reference for the form + optimistic-locking + admin-handler module pattern
- [Source: _bmad-output/implementation-artifacts/8-4-reference-data-management.md] — module-extraction pattern (admin_reference_data.rs); reuse for `admin_system.rs`
- [Source: _bmad-output/implementation-artifacts/8-7-permanent-delete-and-auto-purge.md] — settings table k/v read pattern (auto_purge_interval_seconds added by 8-7)
- [Source: CLAUDE.md] — Key Patterns: settings table, `Arc<RwLock<AppSettings>>`, optimistic locking, HTMX OOB, CSP, scanner-guard invariant, Admin tab pattern, source-file-size limit (#12)
- [Source: src/config.rs:154-280] — `AppSettings` struct + `Default` impl + `load_from_db` (extension surface; new `migrate_legacy_env_vars` function lands alongside, NOT inside `load_from_db`)
- [Source: src/lib.rs:30-53] — `AppState` shape + `session_timeout_secs()` helper (pattern for new helpers)
- [Source: src/services/locking.rs:5-12] — `check_update_result` (use as-is)
- [Source: src/error/mod.rs] — `AppError::Conflict` variant
- [Source: src/routes/admin.rs:932-941] — current `admin_system_panel` stub
- [Source: src/routes/admin.rs:385-388] — current `AdminSystemPanel` template struct stub
- [Source: src/middleware/locale.rs:75-81] — locale resolution chain (hardcoded `"fr"` to be replaced)
- [Source: src/i18n/resolve.rs:19-42] — `resolve_locale` signature (lifetime relaxation may be needed)
- [Source: src/main.rs:115-153] — provider registration + env-var reads (refactor surface)
- [Source: src/metadata/provider.rs:31-64] — `MetadataProvider` trait (verify method shape before refactor)
- [Source: src/routes/loans.rs:92] — current overdue-threshold read pattern (no change needed)
- [Source: src/templates_audit.rs:35-41] — `ALLOWED_HX_CONFIRM_SITES` (frozen at 5 — no extension)
- [Source: src/templates_audit.rs:302-394] — `forms_include_csrf_token` audit
- [Source: src/middleware/csrf.rs] — `CSRF_EXEMPT_ROUTES` (frozen at `[("POST", "/login")]`)
- [Source: migrations/20260329000000_initial_schema.sql:262-280] — `settings` table DDL + initial seeds
- [Source: static/js/mybibli.js:195-202] — `data-action`-delegated handler example (`dismiss-feedback`)

## Dev Agent Record

### Agent Model Used

_TBD on dev-story start_

### Debug Log References

_(empty)_

### Completion Notes List

_(empty)_

### File List

_(empty — to be filled by dev-story)_
