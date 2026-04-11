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

# Database
cargo sqlx prepare                   # Regenerate .sqlx/ offline cache after query changes
cargo sqlx prepare --check --workspace -- --all-targets  # Verify cache matches source (pre-commit check)

# i18n — REQUIRED after adding/changing locale keys
touch src/lib.rs && cargo build      # Force i18n proc macro to re-read YAML files
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

## Architecture

**Stack:** Rust 2024 + Axum 0.8 + SQLx 0.8 (MariaDB) + Askama 0.15 + HTMX 2.0 + Tailwind CSS v4

### Source Layout

- `src/routes/` — HTTP handlers. Thin: extract params, call service, return response.
- `src/services/` — Business logic. All domain rules live here, never in handlers.
- `src/models/` — Database models. SQL queries, row mapping, `DbPool` parameter.
- `src/middleware/` — Axum middleware: `auth.rs` (Session extractor), `htmx.rs` (HxRequest + HtmxResponse), `pending_updates.rs` (OOB metadata delivery), `logging.rs`.
- `src/error/` — `AppError` enum (Internal, NotFound, BadRequest, Conflict, Unauthorized, Database). All errors must use this — no `anyhow` or raw strings.
- `src/metadata/` — External metadata providers. `MetadataProvider` async trait + BnF implementation.
- `src/tasks/` — Background tasks (tokio::spawn). `metadata_fetch.rs` for async BnF lookups.
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
- **Session:** Cookie named `"session"` (NOT "session_token"). HttpOnly, SameSite=Strict, no max-age. Session extractor in `middleware/auth.rs`. Roles: Anonymous < Librarian < Admin.
- **Templates:** Askama templates extend `layouts/base.html` via `{% block content %}`. Nav bar in `components/nav_bar.html`. All page templates must pass `lang`, `role`, `current_page`, `skip_label`, nav labels.
- **HTML escaping:** Use `crate::utils::html_escape()` — DO NOT duplicate.
- **Pool access:** `pool: &DbPool` from `AppState`. For spawned tasks: `pool.clone()` (Arc internally).
- **SQLx offline:** Run `cargo sqlx prepare` after any query change, commit `.sqlx/`.

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
> - The `loginAs()` helper reads `TEST_ADMIN_PASSWORD` env var with default `admin` (matches seed in `migrations/20260331000004_fix_dev_user_hash.sql`)

**HTMX wait strategies:** Never use arbitrary `waitForTimeout(N)`. Wait for DOM state explicitly:
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
- `tests/e2e/helpers/scanner.ts` — **⚠️ STUB, not functional**. Pre-existing tech debt from Epic 1. Do not rely on it until explicitly reimplemented.

**Session cookie format:** The `DEV_SESSION_COOKIE` value `"ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2"` is base64 of a development session token seeded by `migrations/20260329000002_seed_dev_user.sql`. Cookie name is `session` (NOT `session_token`).

**Known app quirks (non-blocking):** (1) duplicate `#session-counter` IDs in catalog page DOM (mitigated with `.first()` in tests), (2) Google Books provider upgrades cover URLs to HTTPS (mitigated by accepting placeholder SVG in cover-image tests).
