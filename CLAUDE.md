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

# E2E tests (requires running app + MariaDB)
cd tests/e2e && npm test             # Run all Playwright E2E tests
cd tests/e2e && npx playwright test specs/journeys/catalog-title.spec.ts  # Single spec

# E2E with Docker (full stack)
cd tests/e2e && docker compose -f docker-compose.test.yml up -d
cd tests/e2e && npm test

# Database
cargo sqlx prepare                   # Regenerate .sqlx/ offline cache after query changes

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
- **DB queries:** MUST include `deleted_at IS NULL` in every SELECT/JOIN on entity tables. Every entity table has `deleted_at`, `version`, `created_at`, `updated_at` columns. **MariaDB type gotchas:** (1) `JSON` columns are stored as `BLOB` — use `CAST(col AS CHAR)` to read as String. (2) `BIGINT UNSIGNED NULL` columns — use `CAST(col AS SIGNED)` and read as `Option<i64>`, then convert to `u64`. (3) Never use `CAST(... AS UNSIGNED)` in SELECT — SQLx can't decode `BIGINT UNSIGNED` into Rust integers reliably.
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
