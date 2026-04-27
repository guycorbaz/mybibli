# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
# Build and check
cargo check                          # Fast type-check
cargo build                          # Full debug build
cargo clippy -- -D warnings          # Lint (zero warnings policy)

# Unit tests
cargo test                           # Run all unit tests
cargo test config::                  # Run tests in a specific module
cargo test test_name -- --nocapture  # Run single test with output

# DB-backed integration tests (tests/find_similar.rs uses #[sqlx::test])
docker compose -f tests/docker-compose.rust-test.yml up -d  # Starts dedicated MariaDB on port 3307
SQLX_OFFLINE=true DATABASE_URL='mysql://root:root_test@localhost:3307/mybibli_rust_test' \
    cargo test --test find_similar   # Each test gets a fresh DB via #[sqlx::test(migrations = "./migrations")]

# E2E tests (requires running app + MariaDB)
cd tests/e2e && npm test             # Run all Playwright E2E tests
cd tests/e2e && npx playwright test specs/journeys/catalog-title.spec.ts  # Single spec

# E2E with Docker (full stack)
cd tests/e2e && docker compose -f docker-compose.test.yml up -d
cd tests/e2e && npm test

# E2E stack reset — single-command teardown + rebuild + wait-for-ready.
# Use when local DB state is polluted and tests expect a fresh baseline
# (see Epic 7 retrospective Action 4 for the backstory).
./scripts/e2e-reset.sh

# Flake gate (run before committing E2E changes) — enforced by CI in the e2e job
grep -rE "waitForTimeout\(" tests/e2e/specs/ tests/e2e/helpers/ && exit 1 || true

# Database
cargo sqlx prepare                   # Regenerate .sqlx/ offline cache after query changes
cargo sqlx prepare --check --workspace -- --all-targets  # Verify cache matches source (pre-commit check)

# i18n — REQUIRED after adding/changing locale keys
touch src/lib.rs && cargo build      # Force i18n proc macro to re-read YAML files

# CSP report-only mode (story 7-4) — observe violations without blocking
CSP_REPORT_ONLY=true cargo run       # Switches header to Content-Security-Policy-Report-Only
```

## Foundation Rules

These apply to ALL sessions without exception.

1. **DRY** — No duplicated code. Shared utilities go in `src/utils.rs`.
2. **Unit Tests** — All code must have unit tests, written alongside implementation. No code ships without corresponding unit tests. Bug fixes must include a regression test.
3. **E2E Tests** — All features and bug fixes must have Playwright E2E tests covering the real user scenario. Must include a smoke test covering the real user journey (no cookie injection shortcuts). A feature without E2E coverage is not done.
4. **Code Language** — All code, comments, variables, and commit messages in English.
5. **Gate Rule** — No milestone transition until ALL tests (unit + E2E) are green.
6. **Code Review Loop** — After code review, if any Medium+ severity findings are discovered, re-run the review after fixes. Story is clean only when a full pass finds no Medium+ issues.
7. **E2E Smoke Test per Epic** — Each epic MUST have at least one E2E test that starts from a blank browser (no injected cookies), performs the epic's core user journey end-to-end (e.g., login → navigate → perform action → verify result). If this test fails, the epic is NOT done.
8. **Retrospectives** — Mandatory at the end of each epic, never postponed. Run the complete test suite before each retrospective.
9. **Error Message Quality** — Error messages are iteratively improved via retrospectives from real usage.
10. **Commit & Push Cadence** — Commit after every workflow step (after `create-story`, after `validate`, after `dev-story`, after `code-review`). Push only on demand or at epic close (after retrospective) — this saves CI minutes and keeps the remote history aligned with epic milestones rather than intra-story churn.
11. **Issue Tracking on GitHub** — Change requests, bugs, known-failures, code-review findings, and any other deferred work are tracked as GitHub Issues (labels `type:change-request`, `type:bug`, `type:known-failure`, `type:code-review-finding`). Never re-introduce a local markdown tracking doc (`deferred-work.md`, `known-issues.md`, etc.) — GitHub Issues is the single source of truth.
12. **Source File Size Limit** — No single source file should exceed 2000 lines. When a file approaches this limit, split it into focused modules or separate concerns into `src/services/`, `src/models/`, or new submodules. Maintainability and testability degrade when files grow beyond this threshold.
13. **Local Testing Before Push** — ALWAYS test locally before pushing to GitHub. Minimum: (1) `cargo check` + `cargo clippy`, (2) `cargo test` for affected modules, (3) if story includes E2E or Docker, run `./scripts/e2e-reset.sh` and `docker compose -f tests/e2e/docker-compose.test.yml up` to verify the full stack builds and starts without errors. Do NOT push if local tests fail. This prevents CI failures and wasted build minutes.

## Architecture

**Stack:** Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + HTMX 2.0 + Tailwind CSS v4

### Source Layout

- `src/routes/` — HTTP handlers. Thin: extract params, call service, return response. `admin.rs` ships the `/admin` page (tabs: health, users, reference_data, trash, system) — admin-only.
- `src/services/` — Business logic. All domain rules live here, never in handlers. `admin_health.rs` owns Health-tab data builders (entity counts, trash count, MariaDB version cache, disk usage).
- `src/models/` — Database models. SQL queries, row mapping, `DbPool` parameter.
- `src/middleware/` — Axum middleware: `auth.rs` (Session extractor), `htmx.rs` (HxRequest + HtmxResponse), `pending_updates.rs` (OOB metadata delivery), `logging.rs`, `csp.rs` (Content-Security-Policy + hardening headers, story 7-4).
- `src/error/` — `AppError` enum (Internal, NotFound, BadRequest, Conflict, Unauthorized, Database). All errors must use this — no `anyhow` or raw strings.
- `src/metadata/` — External metadata providers. `MetadataProvider` async trait + BnF implementation.
- `src/tasks/` — Background tasks (tokio::spawn). `metadata_fetch.rs` for async BnF lookups; `provider_health.rs` for 5-min provider-reachability pings (story 8-1).
- `src/config.rs` — `Config` (env vars) + `AppSettings` (DB settings table, `Arc<RwLock>`).
- `src/lib.rs` — `AppState { pool: DbPool, settings: Arc<RwLock<AppSettings>> }`.

### Key Patterns

- **Error handling:** `AppError` enum with `IntoResponse`. Conflict = 409, Unauthorized = 303 redirect with HX-Redirect.
- **Logging:** `tracing` macros only — no `println!`.
- **i18n:** `rust_i18n::t!("key")` for ALL user-facing text. Keys in `locales/en.yml` + `locales/fr.yml`. JS strings: read `<html lang>` and use embedded string map. **CRITICAL YAML FORMAT: locale files must NOT have `en:` or `fr:` as top-level wrapper — the filename determines the locale. Keys start at root level (e.g., `nav:` not `en: nav:`). After adding/changing keys, run `touch src/lib.rs` before `cargo build` to force proc macro recompilation.**
- **DB queries:** MUST include `deleted_at IS NULL` in every SELECT/JOIN on entity tables. Every entity table has `deleted_at`, `version`, `created_at`, `updated_at` columns. **MariaDB type gotchas:** (1) `JSON` columns are stored as `BLOB` — use `CAST(col AS CHAR)` to read as String. (2) `BIGINT UNSIGNED NULL` columns — use `CAST(col AS SIGNED)` and read as `Option<i64>`, then convert to `u64`. (3) Never use `CAST(... AS UNSIGNED)` in SELECT — SQLx can't decode `BIGINT UNSIGNED` into Rust integers reliably. (4) `TIMESTAMP` columns in dynamic queries (`sqlx::query()`) — use `CAST(col AS DATETIME) AS col` to read as `NaiveDateTime`. Without CAST, SQLx returns a type mismatch error. This does NOT affect typed macros (`sqlx::query!`) which handle conversion automatically.
- **Optimistic locking:** UPDATE with `WHERE id = ? AND version = ?`, then `check_update_result()` from `services/locking.rs`.
- **Soft delete:** `services/soft_delete.rs` with table whitelist. Never hard-delete.
- **HTMX responses:** `HtmxResponse { main, oob: Vec<OobUpdate> }`. Check `HxRequest(is_htmx)` — return fragment for HTMX, full page for direct navigation.
- **Session:** Cookie named `"session"` (NOT "session_token"). HttpOnly, `SameSite=Lax` (downgraded from Strict in story 7-3 so the language-toggle top-level POST from a same-site link still carries the session; a drift-compatible risk accepted in the story). Authenticated cookies: no max-age (session cookie). Anonymous-session cookies (story 8-2): `Max-Age=7d` aligned with the daily purge window. Session extractor in `middleware/auth.rs`. Roles: Anonymous < Librarian < Admin.
- **Templates:** Askama templates extend `layouts/base.html` via `{% block content %}`. Nav bar in `components/nav_bar.html`. All page templates must pass `lang`, `role`, `current_page`, `skip_label`, nav labels.
- **HTML escaping:** Use `crate::utils::html_escape()` — DO NOT duplicate.
- **Pool access:** `pool: &DbPool` from `AppState`. For spawned tasks: `pool.clone()` (Arc internally).
- **SQLx offline:** Run `cargo sqlx prepare` after any query change, commit `.sqlx/`.
- **CSP & hardening headers (story 7-4):** `src/middleware/csp.rs` is wrapped outermost in `routes::build_router` (per AR16: `Logging → Auth → [Handler] → PendingUpdates → CSP`). Strict directive — `script-src 'self'`, `style-src 'self'`, no `unsafe-inline` / no `unsafe-eval`. **Zero inline `<script>`, `<style>`, `style="..."`, `onclick=` etc.** in templates AND in HTML produced from Rust (`feedback_html`, `pending_updates`, `locations` tree, …). All dismiss buttons use `data-action="dismiss-feedback"` (delegated handler in `static/js/mybibli.js`). HTMX trigger filters that need JS evaluation (e.g. `hx-trigger="keydown[key=='Enter']"`) are forbidden — emit a `CustomEvent` from a JS module instead. The `src/templates_audit.rs` `#[test]` walks `templates/` and panics on regressions; pair it with manual greps over `src/` for HTML strings when adding new server-rendered fragments. Toggle observe-only mode with `CSP_REPORT_ONLY=true`.
- **Modal scanner-guard invariant (story 7-5):** `static/js/scanner-guard.js` watches `dialog[open]` and `[aria-modal="true"]` surfaces via MutationObserver. While any modal is open it captures `keydown` at the document-capture phase and either forwards printable chars / Enter to the modal's focused text input or blocks them — preventing a USB scanner burst from leaking into `#scan-field` (duplicate scan) or activating a modal's default-focused Cancel/Confirm button. New destructive action UX MUST use the UX-DR8 Modal component (Epic 9) so it automatically inherits this protection. New `hx-confirm=` attributes are BLOCKED by `src/templates_audit.rs::hx_confirm_matches_allowlist`; the allowlist is frozen at 5 grandfathered sites (5th added in story 8-3 for admin user deactivation) and only changes through explicit review.
- **CSRF synchronizer token (story 8-2):** Every session row carries a `sessions.csrf_token` (VARCHAR(64)) minted on login or on an anonymous visitor's first hit (via `src/middleware/auth.rs::session_resolve_middleware`). Pages emit `<meta name="csrf-token" content="{{ csrf_token|e }}">` in `layouts/base.html`, and every `<form method="POST">` includes a hidden `<input name="_csrf_token" value="{{ csrf_token|e }}">` as its first child. HTMX requests inherit the token via `static/js/csrf.js`'s `htmx:configRequest` listener (sets `X-CSRF-Token` from the meta tag). The `src/middleware/csrf.rs` layer runs on state-changing methods (POST/PUT/PATCH/DELETE), does constant-time compare via the `subtle` crate, and on mismatch emits 403 + `HX-Trigger: csrf-rejected` + `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` + `Cache-Control: no-store`. `csrf.js`'s `htmx:beforeSwap` listener flips `shouldSwap = true` on that rejection so the server-rendered "Session expired" FeedbackEntry actually lands in the DOM. Exempt-route allowlist is frozen at exactly `[("POST", "/login")]`, policed by `src/templates_audit.rs::csrf_exempt_routes_frozen`. Form coverage is policed by `forms_include_csrf_token` in the same file. **`HX-Trigger` → JS-listener idiom is a NEW project-wide pattern for server-driven UI coordination** — reuse it for future optimistic-locking conflicts, session expiry, etc. Anonymous visitors accumulate session rows; `src/tasks/anonymous_session_purge.rs` deletes them after 7 days of inactivity (authenticated rows are untouched — they're handled by story 7-2's inactivity timeout). `GET /logout` is now `405` — logout flows through a POST form in `nav_bar.html`.
- **Admin page tab pattern (story 8-1):** `/admin?tab=<name>` for deep-linking and history; `/admin/<name>` for HTMX panel swap via `hx-get` + `hx-push-url`. Tab resolution is server-side; invalid `?tab=` falls back to `health`. Every Epic-8 story fills in exactly one panel stub — extend `AdminTab` enum + replace the corresponding `admin_<name>_panel.html` fragment. All admin handlers start with `session.require_role_with_return(Role::Admin, "/admin...")?` so Anonymous bounces to `/login?next=%2Fadmin` and Librarian gets a 403 FeedbackEntry body.
- **User deactivation semantics (story 8-3):** User deactivation uses `users.deleted_at` (soft-delete) — set to `NOW()` on deactivate, `NULL` on reactivate. Deactivation is ATOMIC: a single DB transaction (a) locks the target user row, (b) checks guards (self-deactivate, last-active-admin), (c) updates `users.deleted_at`, (d) deletes all `sessions` rows for that user (`deleted_at = NOW()`), (e) commits. This ensures the deactivated user is immediately logged out and cannot log back in. The `active` boolean is vestigial (schema default `TRUE`) and is NOT toggled by this story. Login predicate requires both `active = TRUE AND deleted_at IS NULL`.
- **Auto-purge pattern (story 8-7):** Soft-deleted items are hard-purged after 30 days via two mechanisms: (1) **startup purge** (blocking, bounded by count) runs in `main()` after migrations and before binding the HTTP listener — controllable via `MYBIBLI_SKIP_STARTUP_PURGE=1` for fast-iteration dev/test loops, (2) **scheduled purge** via `src/tasks/auto_purge_scheduler.rs` runs on a configurable cadence (`AppSettings::auto_purge_interval_seconds`, default 86400 = 24h) with `MissedTickBehavior::Skip` so a clock jump doesn't trigger a burst, first run 1 minute after startup. Both use FK dependency ordering via a whitelist (`src/services/soft_delete.rs::ALLOWED_TABLES`) to delete children before parents (e.g., `title_contributors` before `titles`), scoped in transactions per entity family to prevent orphans, with a per-table drain loop (LIMIT 10000 per batch, hard-cap 100 iterations) so back-pressure on a single huge family doesn't leave stale rows. On error (FK violation, DB lock), the family transaction rolls back and logs the error; the daily task continues to the next family (will retry on next scheduled run). Both startup and scheduler runs write a single `admin_audit` row with per-table counts for forensics. Admins can also force permanent deletion from the Trash panel with explicit confirmation (modal friction: type item name to enable button); hard-deletes record an audit entry in `admin_audit` (who, what entity, when). **Self-deletion + last-active-admin guards apply ONLY to the admin manual delete handler (`POST /admin/trash/{table}/{id}/permanent-delete`); the auto-purge scheduler/startup task has no such guard and will hard-delete any row whose `deleted_at < NOW() - 30 DAY`, regardless of role.**

### HTMX OOB Swap Pattern

```rust
let resp = HtmxResponse {
    main: feedback_html("success", &message, &suggestion),
    oob: vec![OobUpdate { target: "context-banner".to_string(), content: banner_html }],
};
```

### Async Metadata Flow

Scan ISBN → create title → `tokio::spawn(fetch_metadata_chain)` → return skeleton feedback → background: BnF API → update title + cache → mark resolved → next HTMX request: PendingUpdates middleware delivers OOB swap replacing skeleton.

### E2E Test Patterns

Playwright E2E suite lives in `tests/e2e/`. Implementation details below are load-bearing — violating them causes the cascading flake failures documented in story 5-1 (2026-04-05).

**Execution mode:** `fullyParallel: true` (parallel, default workers). Each spec uses unique ISBNs via `specIsbn()` from `tests/e2e/helpers/isbn.ts` and unique V-codes/L-codes/borrower names to ensure no data collisions between specs. All non-smoke specs use `loginAs()` for per-test session isolation.

**Login strategy:**

> **HARD RULE — Foundation Rule #7 (Smoke tests):**
> - ✅ Smoke tests (one per epic) MUST use `loginAs(page)` from `tests/e2e/helpers/auth.ts` — real browser login starting from a blank context
> - ✅ All non-smoke tests also use `loginAs(page)` in `beforeEach` — each test gets its own server-side session for parallel safety
> - ❌ Do NOT inject `DEV_SESSION_COOKIE` — it causes session state pollution in parallel mode
> - Signature: `loginAs(page, role?)` with `role: "admin" | "librarian"` (default `"admin"`). Passwords resolve from `TEST_ADMIN_PASSWORD` / `TEST_LIBRARIAN_PASSWORD` (defaults `admin` / `librarian`), matching seeds in `migrations/20260331000004_fix_dev_user_hash.sql` and `migrations/20260414000001_seed_librarian_user.sql`. The role argument is a typed union so typos fail `tsc --noEmit` — the typecheck is wired into the `e2e` CI job.
> - Env-var overrides apply when Playwright runs on the host (the `_gates.yml` default: only the app runs in docker, `npm test` runs directly). If Playwright is ever moved into docker-compose, pass those vars through the Playwright service's `environment:` block.
> - **`TEST_*_PASSWORD` overrides are local-only.** CI does not set `TEST_ADMIN_PASSWORD` / `TEST_LIBRARIAN_PASSWORD`, so CI always uses the seed defaults (`admin` / `librarian`). If you rotate a seed password in a migration, update the seed itself — do NOT rely on env overrides as the source of truth in CI.

**HTMX wait strategies:** Never use arbitrary `waitForTimeout(N)`. This is enforced by a CI grep gate in the `e2e` job — new `waitForTimeout` calls fail the PR. Use the DOM-state assertions below instead:
```ts
// For V-code creation feedback — wait for the specific V-code text to avoid stale entries
await expect(page.locator(".feedback-entry").first()).toContainText(/V0060/i, { timeout: 10000 });
```
For OOB swaps (e.g., context-banner, pending-updates), wait for the specific swap target to update before asserting.

**Selector policy:** Prefer stable selectors in this priority order:
1. `page.getByRole(...)` — semantic, accessibility-aware
2. `page.locator("#id")` — stable id attributes from templates
3. `page.getByText(/Active loans|Prêts actifs/i)` — i18n-aware regex for user-visible text
4. CSS/XPath selectors — last resort; fragile to Tailwind class changes

**i18n-aware matchers:** All user-visible text in templates goes through `t!()`. Tests must match both EN and FR variants:
```ts
await expect(page.locator("h1")).toContainText(/Active loans|Prêts actifs/i);
```

**Data isolation:** Each spec file generates unique ISBNs via `specIsbn(specId, seq)` from `tests/e2e/helpers/isbn.ts`. The 2-character `specId` is unique per spec file (e.g., `"CT"` for catalog-title, `"LN"` for loans). The mock metadata server (`tests/e2e/mock-metadata-server/server.py`) has a catch-all that returns synthetic metadata for any unknown ISBN, so generated ISBNs always resolve. V-codes must also be unique across specs (no shared V0042, V0071, etc.). Only `provider-chain.spec.ts` uses the real known ISBNs (`9782070360246`, `9780134685991`) because it tests provider-specific metadata content.

**Helper files:**
- `tests/e2e/helpers/auth.ts` — `loginAs()` (real browser login), `logout()` (clears cookies)
- `tests/e2e/helpers/isbn.ts` — `specIsbn(specId, seq)` generates unique valid EAN-13 ISBNs per spec
- `tests/e2e/helpers/accessibility.ts` — axe-core a11y assertions
- `tests/e2e/helpers/loans.ts` — `scanTitleAndVolume`, `createBorrower`, `createLoan`, `returnLoanFromLoansPage`. Canonical loan-flow helpers. `createLoan` submits via direct `page.request.post('/loans', ...)` instead of the HTMX form — the HTMX path proved racy under parallel load (story 5-1c) because `waitForURL(/\/loans/)` was a no-op when the form lives on /loans.
- `tests/e2e/helpers/scanner.ts` — `simulateScan` (scanner burst, 20 ms inter-key) and `simulateTyping` (human pace, 100 ms inter-key). Uses Playwright's native `{ delay }` option — do NOT re-roll `waitForTimeout` sequences.

**Session cookie format:** The `DEV_SESSION_COOKIE` value `"ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2"` is base64 of a development session token seeded by `migrations/20260329000002_seed_dev_user.sql`. Cookie name is `session` (NOT `session_token`).

**Known app quirks & deferred work** are tracked as GitHub Issues — filter by label `type:known-failure` for accepted-risk quirks, `type:code-review-finding` for deferred review findings, `type:change-request` for product/architectural change proposals. The `.github/ISSUE_TEMPLATE/` forms enforce structured fields for each type. GitHub Issues is the single source of truth — do not re-introduce a markdown tracking doc.
