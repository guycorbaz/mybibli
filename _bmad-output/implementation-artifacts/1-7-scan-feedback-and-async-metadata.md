# Story 1.7: Scan Feedback & Async Metadata

Status: done

## Story

As a librarian,
I want to see immediate scan feedback and have metadata fetched asynchronously from external APIs,
so that I can continue scanning without waiting for metadata resolution.

## Acceptance Criteria (BDD)

### AC1: Skeleton FeedbackEntry on ISBN Scan

**Given** I scan an ISBN on /catalog,
**When** the scan is processed,
**Then** a skeleton FeedbackEntry appears immediately (< 500ms) showing "Title created from ISBN {isbn} — Fetching metadata..." with a spinner/shimmer bar while the async task runs. No confirmation audio on skeleton state.

### AC2: Resolved Metadata via PendingUpdates

**Given** the async metadata task completes successfully,
**When** I perform the next HTMX action (e.g., another scan, pagination click),
**Then** the PendingUpdates middleware delivers the resolved metadata as an OOB swap, replacing the skeleton FeedbackEntry with a success entry showing the title name and author (no cover thumbnail — cover download deferred). Phase 2 confirmation audio deferred to story 1-8.

### AC3: Failed/Timeout Metadata

**Given** the async metadata task fails or times out (30s configurable via AppSettings),
**When** the result is delivered on the next HTMX action,
**Then** a warning FeedbackEntry appears indicating "No metadata found for ISBN {isbn}. [Edit manually]". The title remains with only the ISBN — no data corruption.

### AC4: Metadata Cache (24h TTL)

**Given** I scan the same ISBN that was fetched within the last 24 hours,
**When** the metadata is looked up,
**Then** the cached response from `metadata_cache` table is used instead of calling external APIs. The skeleton resolves immediately on the same response (no next-action delay).

### AC5: Client-Side ISBN Validation (FR103)

**Given** I scan an invalid ISBN (checksum fails),
**When** client-side validation runs in scan-field.js,
**Then** an error FeedbackEntry appears immediately without making a server request. The error message follows "What happened → Why → What you can do" pattern.

### AC6: Already-Assigned Code Error (FR104)

**Given** I scan a V-code or L-code that is already assigned,
**When** the scan is processed,
**Then** an error FeedbackEntry appears with specific details ("Label V0042 is already assigned to L'Écume des jours. Scan a different label.").

### AC7: Session Counter (FR108)

**Given** I have cataloged items during this session,
**When** I look at the catalog page,
**Then** a session counter displays "X items cataloged this session" (tied to HTTP session, survives page navigation, resets on new session).

### AC8: Mock Metadata Server for E2E (AR12)

**Given** the mock metadata server is running (docker-compose.test.yml),
**When** Playwright e2e tests run,
**Then** metadata responses are deterministic and do not depend on real external APIs.

## Explicit Scope Boundaries

**In scope:**
- Skeleton FeedbackEntry variant (spinner, shimmer bar, muted border)
- Async metadata fetch via `tokio::spawn` with provider chain (BnF only for MVP — single provider, others stubbed)
- PendingUpdates middleware (checks `pending_metadata_updates` table, appends OOB swaps)
- AppSettings struct in Axum state (`Arc<RwLock<AppSettings>>`) loaded from `settings` table
- `metadata_cache` table for 24h caching
- MetadataProvider trait + BnF implementation (first provider)
- Session counter OOB updates
- Feedback lifecycle JS (fade at 10s, remove at 20s, persist warning/error)
- Mock metadata server for E2E tests
- Client-side ISBN checksum validation (already exists in scan-field.js — verify/extend)

**NOT in scope (deferred):**
- Full 8-provider chain (Google Books, Open Library, BDGest, Comic Vine, MusicBrainz, TMDb, OMDb) — each added incrementally in later stories
- Cover image download and processing — deferred to later story
- DVD UPC lookup (known limitation — title search, not UPC barcode)
- Admin configuration UI for AppSettings
- Metadata retry logic on failure
- Feedback audio (Web Audio oscillator) — deferred to story 1-8 cross-cutting

**Why BnF only:** The architecture specifies the provider chain per media type but the story focus is the async pipeline infrastructure. BnF is the first provider for books (ISBN 978/979). Adding more providers is incremental once the trait and fetch chain are working.

## Tasks / Subtasks

- [x] Task 1: AppSettings struct and DB loading (AC: 3, 7)
  - [x] 1.1 Add `AppSettings` struct to `src/config.rs` (alongside existing `Config` struct — **DO NOT create a config/ directory**, the module already exists as `src/config.rs`): `overdue_threshold_days: i32`, `scanner_burst_threshold_ms: u64`, `search_debounce_delay_ms: u64`, `session_timeout_secs: u64`, `metadata_fetch_timeout_secs: u64`
  - [x] 1.2 Implement `AppSettings::load_from_db(pool) -> Result<AppSettings>` — reads from `settings` table, maps `setting_key` → struct fields with defaults. **Setting keys already seeded in initial migration:** `overdue_loan_threshold_days=30`, `scanner_burst_threshold_ms=50`, `search_debounce_delay_ms=300`, `metadata_fetch_timeout_seconds=30`, `session_inactivity_timeout_hours=4`
  - [x] 1.3 Add `settings: Arc<RwLock<AppSettings>>` to `AppState` in `src/lib.rs` — **`pub mod config;` already exists, do NOT re-add**
  - [x] 1.4 Load settings in `main.rs` after migrations, before building router
  - [x] 1.5 Unit tests: AppSettings defaults, load mapping

- [x] Task 2: MetadataProvider trait and BnF implementation (AC: 1, 2, 3, 4)
  - [x] 2.0 **Add `async-trait = "0.1"` to `Cargo.toml`** — required for async methods in traits (Rust 2021 edition does not support native async trait methods)
  - [x] 2.1 Create `src/metadata/provider.rs` with `#[async_trait] MetadataProvider` trait: `name()`, `supports_media_type()`, `lookup_by_isbn()`, `lookup_by_upc()`, `search_by_title()`. All return `Result<Option<MetadataResult>>`. `MetadataResult` struct: `title, subtitle, description, authors: Vec<String>, publisher, publication_date, cover_url, language`
  - [x] 2.2 Create `src/metadata/bnf.rs` implementing `MetadataProvider` for BnF (Bibliothèque nationale de France) — REST/JSON API, no auth needed. Endpoint: `https://data.bnf.fr/sparql` or BnF SRU API. Parse response into `MetadataResult`
  - [x] 2.3 Create `src/metadata/mod.rs` exporting trait + BnF provider
  - [x] 2.4 Unit tests: MetadataResult construction, BnF response parsing (mock JSON)

- [x] Task 3: Database migrations + Metadata cache model (AC: 4)
  - [x] 3.1 metadata_cache table already exists in initial migration — no new migration needed
  - [x] 3.2 Create migration `migrations/20260331000003_add_pending_status.sql` — `ALTER TABLE pending_metadata_updates ADD COLUMN status VARCHAR(20) NOT NULL DEFAULT 'pending';` Values: 'pending', 'resolved', 'failed'
  - [x] 3.3 Create `src/models/metadata_cache.rs` with `MetadataCacheModel`: `find_by_isbn(pool, isbn) -> Option<MetadataResult>` (checks `fetched_at > NOW() - INTERVAL 24 HOUR AND deleted_at IS NULL`), `upsert(pool, isbn, response_json)`
  - [x] 3.4 Add `pub mod metadata_cache;` to `src/models/mod.rs`
  - [x] 3.5 Unit tests: cache hit/miss logic, 24h TTL calculation, JSON roundtrip

- [x] Task 4: Async metadata fetch task (AC: 1, 2, 3, 4)
  - [x] 4.1 Create `src/tasks/metadata_fetch.rs` with `fetch_metadata_chain(pool, title_id, isbn, timeout_secs)` — async function spawned via `tokio::spawn`
  - [x] 4.2 Flow: check metadata_cache → if hit, update title + resolve immediately → if miss, call BnF provider → on success, update title fields + insert cache + mark resolved
  - [x] 4.3 On failure/timeout: UPDATE `pending_metadata_updates` SET `resolved_at = NOW(), status = 'failed'`
  - [x] 4.4 Use `tokio::time::timeout(Duration::from_secs(timeout_secs))` to enforce the configurable timeout
  - [x] 4.5 All DB operations use `pool.clone()` (pool is `Arc` internally, cheap to clone)
  - [x] 4.6 Unit tests: metadata result validation, empty title skip

- [x] Task 5: PendingUpdates middleware (AC: 2, 3)
  - [x] 5.1 Create `src/middleware/pending_updates.rs` — Axum middleware that runs AFTER each HTMX handler response
  - [x] 5.2 On each HTMX request: query `pending_metadata_updates WHERE session_token = ? AND resolved_at IS NOT NULL AND deleted_at IS NULL`
  - [x] 5.3 For each resolved item: render a success or warning FeedbackEntry (based on status), append as OOB swap
  - [x] 5.4 After appending, soft-delete the processed `pending_metadata_updates` rows (set `deleted_at = NOW()`)
  - [x] 5.5 Register middleware in router (runs for all `/catalog/*` routes) with DbPool extension
  - [x] 5.6 Unit tests: session extraction, OOB rendering, HTML escaping, empty updates

- [x] Task 6: Update scan handler for async flow (AC: 1, 2, 7)
  - [x] 6.1 Update `handle_scan()`: for ISBN scans, check cache → if new, spawn async fetch and return skeleton
  - [x] 6.2 Return skeleton FeedbackEntry: muted border, spinner icon, shimmer bar
  - [x] 6.3 Include OOB updates + added `<span id="session-counter">` to catalog.html template
  - [x] 6.4 Pass `metadata_fetch_timeout_secs` from `AppSettings` to the spawned task
  - [x] 6.5 For cached ISBNs: skip spawn, return resolved FeedbackEntry immediately

- [x] Task 7: Skeleton FeedbackEntry HTML + CSS (AC: 1)
  - [x] 7.1 Add `skeleton_feedback_html(title_id, isbn)` function in `catalog.rs`
  - [x] 7.2 Add shimmer CSS animation inline `<style>` with the skeleton
  - [x] 7.3 Each skeleton has unique ID: `feedback-entry-{title_id}` for targeted OOB replacement
  - [x] 7.4 Skeleton respects `prefers-reduced-motion` (disable shimmer animation)

- [x] Task 8: Feedback lifecycle JS enhancement (AC: 2)
  - [x] 8.1 Skeletons use class `feedback-skeleton` (not `feedback-entry`) — excluded from auto-dismiss
  - [x] 8.2 OOB-resolved entries start their own lifecycle timer using `data-resolved-at`
  - [x] 8.3 Resolved success entries use `data-resolved-at` timestamp for timer calculation

- [x] Task 9: i18n keys (AC: all)
  - [x] 9.1 Add to `locales/en.yml`: `feedback.metadata_fetching`, `feedback.metadata_resolved`, `feedback.metadata_resolved_no_author`, `feedback.metadata_failed`, `feedback.metadata_cached`, `feedback.edit_manually`
  - [x] 9.2 Add French translations to `locales/fr.yml`

- [x] Task 10: Unit tests (AC: all)
  - [x] 10.1 AppSettings: defaults and clone tests (config::tests)
  - [x] 10.2 MetadataResult construction, default, multiple authors (provider::tests)
  - [x] 10.3 BnF provider SRU XML parsing: full, empty, no-author fallback, minimal (bnf::tests)
  - [x] 10.4 MetadataCacheModel: full, no-title, minimal, JSON roundtrip, empty object (metadata_cache::tests)
  - [x] 10.5 Skeleton FeedbackEntry HTML: structure, spinner, a11y (catalog::tests)
  - [x] 10.6 PendingUpdates: session extraction, OOB rendering, HTML escape, empty (pending_updates::tests)
  - [x] 10.7 fetch_metadata_chain: result validation, empty title skip (metadata_fetch::tests)

- [x] Task 11: Playwright E2E tests (AC: all)
  - [x] 11.1 Test: Scan ISBN → skeleton/feedback appears with spinner
  - [x] 11.2 Test: Second scan → OOB delivery of resolved metadata on next action
  - [x] 11.3 Test: Session counter increments on each scan
  - [x] 11.4 Test: Invalid ISBN → error FeedbackEntry
  - [x] 11.5 Test: Already-assigned V-code → error with title name
  - [x] 11.6 Test: Mock metadata server — deterministic test environment

- [x] Task 12: Mock metadata server for E2E (AC: 8)
  - [x] 12.1 Create `tests/e2e/mock-metadata-server/server.py` returning deterministic BnF SRU XML for known ISBNs
  - [x] 12.2 Update `tests/e2e/docker-compose.test.yml` to include mock-metadata service
  - [x] 12.3 BnfProvider reads `BNF_API_BASE_URL` env var — docker-compose sets it to mock server

### Review Findings

- [x] [Review][Patch] "[Edit manually]" is now a clickable link to /title/{id} — AC3 actionable element ✅
- [x] [Review][Patch] **CRITICAL: Session cookie name mismatch fixed** — changed `"session_token="` to `"session="` ✅
- [x] [Review][Patch] ISBN sanitized (alphanumeric only) before BnF URL interpolation ✅
- [x] [Review][Patch] Body read bounded to 10 MB in pending_updates middleware ✅
- [x] [Review][Patch] Settings validation: timeout clamped >= 1s, tracing::warn on parse failure ✅
- [x] [Review][Patch] Empty/whitespace author names filtered before contributor creation ✅
- [x] [Review][Patch] `soft_delete_processed` batched with WHERE IN clause ✅
- [x] [Review][Patch] `fetch_metadata_chain` now uses `AppError` instead of raw `String` ✅
- [x] [Review][Patch] E2E test asserts deterministic mock server content (title/author) ✅
- [x] [Review][Defer] Race condition in contributor creation (SELECT then INSERT) — deferred, single-user NAS app with accepted TOCTOU [src/tasks/metadata_fetch.rs:139-158]
- [x] [Review][Defer] Concurrent identical ISBN scans may create duplicate pending_metadata_updates — deferred, single-user NAS [src/routes/catalog.rs:232]
- [x] [Review][Defer] XML parsing via naive string search is fragile — deferred, acceptable for MVP with well-formed BnF responses [src/metadata/bnf.rs:75-127]
- [x] [Review][Defer] Fire-and-forget spawned tasks with no backpressure limit — deferred, single-user NAS [src/tasks/metadata_fetch.rs:17]
- [x] [Review][Defer] Hardcoded "Auteur" role name in SQL queries — deferred, pre-existing DB seed pattern [src/tasks/metadata_fetch.rs:161]

#### Review Pass 2 (2026-03-31)

- [x] [Review][Patch] **LOGIC BUG FIXED: Cache hit now applies metadata via apply_cached_metadata()** ✅ [src/routes/catalog.rs:273]
- [x] [Review][Patch] soft_delete_processed now scoped to session with `AND session_token = ?` ✅ [src/middleware/pending_updates.rs:195]
- [x] [Review][Patch] Content-Length header removed after OOB body append ✅ [src/middleware/pending_updates.rs:217]
- [x] [Review][Patch] Duplicate html_escape removed — uses shared `crate::utils::html_escape` ✅ [src/middleware/pending_updates.rs]
- [x] [Review][Patch] French locale "items" → "éléments" ✅ [locales/fr.yml:13-14]
- [x] [Review][Patch] BnF author parsing: trim forename/surname, skip empty names ✅ [src/metadata/bnf.rs:54]
- [x] [Review][Defer] No body size limit on BnF external API response — single-user NAS, BnF trusted
- [x] [Review][Defer] reqwest::Client created per-request instead of reused — perf optimization deferred
- [x] [Review][Defer] Spawned task panic silently swallowed — acceptable for MVP
- [x] [Review][Defer] COALESCE with empty strings may update DB with empty values — low impact edge case
- [x] [Review][Defer] Raw error strings in template rendering — pre-existing pattern across all routes

## Dev Notes

### Architecture Compliance

- **Service layer:** Metadata fetch logic in `src/tasks/metadata_fetch.rs`, NOT in route handlers
- **Error handling:** `AppError` enum — never crash on metadata failure, always degrade gracefully
- **Logging:** `tracing::info!` for metadata fetch start/complete/fail, `tracing::debug!` for cache hits
- **i18n:** All user-facing text via `t!("key")` — error messages follow "What happened → Why → What you can do" pattern (NFR38)
- **DB queries:** `WHERE deleted_at IS NULL` everywhere including `pending_metadata_updates` and `metadata_cache`
- **HTMX:** Skeleton → resolved transition via OOB swap on same `id` attribute
- **API keys:** Environment variables only (NFR14) — e.g., `std::env::var("GOOGLE_BOOKS_API_KEY")` for future providers. **BnF does NOT require an API key** — no env var needed for MVP
- **Pool access:** `pool: &DbPool` from `AppState`. For spawned tasks: `pool.clone()` (sqlx pool is `Arc` internally)

### Database Schema

**pending_metadata_updates** (already exists in initial migration):
- `id`, `title_id` FK, `session_token` VARCHAR(44), `resolved_at` TIMESTAMP NULL, soft delete + version
- Index: `idx_pending_session_resolved (session_token, resolved_at)`
- Need to add: `status` column ('resolved' or 'failed') — new migration

**metadata_cache** (new table):
- `id`, `isbn` VARCHAR(13) UNIQUE, `response_json` TEXT, `fetched_at` TIMESTAMP, soft delete + version
- 24h TTL: query checks `fetched_at > NOW() - INTERVAL 24 HOUR`

**settings** (already exists with seed data):
- `metadata_fetch_timeout_seconds` = 30 (already seeded)

### AppSettings Pattern

```rust
pub struct AppSettings {
    pub overdue_threshold_days: i32,
    pub scanner_burst_threshold_ms: u64,
    pub search_debounce_delay_ms: u64,
    pub session_timeout_secs: u64,
    pub metadata_fetch_timeout_secs: u64,
}
// In AppState: settings: Arc<RwLock<AppSettings>>
// Read in handlers: state.settings.read().unwrap().metadata_fetch_timeout_secs
```

### Async Metadata Fetch Flow

```
1. User scans ISBN → POST /catalog/scan
2. Handler: check metadata_cache → if hit, return resolved immediately
3. Handler: create minimal Title row (isbn + default genre)
4. Handler: tokio::spawn(fetch_metadata_chain(pool.clone(), title_id, isbn, session_token, timeout)) — **spawn BEFORE returning response**
5. Handler: return skeleton FeedbackEntry + OOB (banner, counter)
6. Background: BnF API call with tokio::time::timeout
7. Background: on success → UPDATE titles SET title=?, subtitle=?, ... WHERE id=?
8. Background: INSERT metadata_cache (isbn, response_json, fetched_at)
9. Background: UPDATE pending_metadata_updates SET resolved_at=NOW(), status='resolved' WHERE title_id=?
10. Next HTMX request from same session → PendingUpdates middleware
11. Middleware: SELECT * FROM pending_metadata_updates WHERE session_token=? AND resolved_at IS NOT NULL AND deleted_at IS NULL
12. Middleware: render success FeedbackEntry for each → append as OOB swap
13. Middleware: UPDATE pending_metadata_updates SET deleted_at=NOW() for processed rows
14. Client: skeleton replaced by resolved entry in-place
```

### Skeleton FeedbackEntry HTML

```html
<div id="feedback-entry-{title_id}" class="feedback-skeleton flex items-start gap-3 px-4 py-3 border-l-4 border-stone-300 dark:border-stone-600 bg-stone-50 dark:bg-stone-800/50 rounded-r-md">
    <svg class="animate-spin w-5 h-5 text-stone-400 flex-shrink-0 mt-0.5" ...>spinner</svg>
    <div class="flex-1">
        <p class="text-sm text-stone-700 dark:text-stone-300">Title created from ISBN {isbn}</p>
        <p class="text-xs text-stone-500 dark:text-stone-400">Fetching metadata from BnF...</p>
        <div class="mt-1 h-2 bg-stone-200 dark:bg-stone-700 rounded shimmer-bar"></div>
    </div>
</div>
```

### PendingUpdates Middleware Pattern

Implemented as Axum layer that wraps `/catalog/*` routes. After the handler produces a response:
1. Extract session token from cookie
2. Query resolved pending updates for this session
3. For each: render FeedbackEntry HTML (success or warning based on status)
4. Append OOB swaps to response body (before closing `</body>` or just concatenate)
5. Soft-delete processed rows

**Critical:** Must NOT block the response — the middleware runs after handler completion, modifying the response body.

### BnF API Integration

BnF SRU (Search/Retrieve via URL) API:
- Base URL: `https://catalogue.bnf.fr/api/SRU`
- Query: `?version=1.2&operation=searchRetrieve&query=bib.isbn adj "{isbn}"&recordSchema=unimarcXchange&maximumRecords=1`
- Response: XML (UNIMARC format) — parse title (200$a), author (700$a), publisher (210$c), date (210$d)
- No API key required
- Rate limit: not published, be respectful (1 req/sec recommended)

Alternative: BnF Data API (JSON-LD):
- `https://data.bnf.fr/sparql` with SPARQL query for ISBN
- Returns JSON-LD — simpler to parse than UNIMARC XML

**Decision for MVP:** Use the BnF Data API (JSON-LD via SPARQL) for simpler parsing. Fall back to creating title with ISBN-only data if BnF fails.

### Route Handler Signature (from catalog.rs)

```rust
use axum::extract::State;
use crate::AppState;
use crate::middleware::auth::Session;
use crate::middleware::htmx::HxRequest;

pub async fn handler(
    State(state): State<AppState>,
    session: Session,
    HxRequest(is_htmx): HxRequest,
) -> impl IntoResponse {
    let pool = &state.pool;
    let settings = state.settings.read().unwrap();
    // ...
}
```

### Previous Story Patterns to Follow

From story 1-6:
- `html_escape()` in `src/utils.rs` (shared module) — use for all user data in HTML
- `url_encode()` in `src/utils.rs` — use for URL parameter values
- `HtmxResponse { main, oob }` with `OobUpdate` for OOB swaps
- Runtime `sqlx::query()` for new queries (no `.sqlx` cache)
- `t!()` for ALL user-facing strings with `%{variable}` interpolation
- i18n keys: underscore-separated namespace (`feedback.metadata_fetching`, not `feedback.metadata.fetching`)
- Askama templates extend `layouts/base.html` with `{% block content %}`

### Project Structure Notes

**Files to create:**
- `migrations/20260331000002_add_metadata_cache.sql` — metadata_cache table
- `migrations/20260331000003_add_pending_status.sql` — add status column to pending_metadata_updates
- `src/metadata/provider.rs` — MetadataProvider trait + MetadataResult
- `src/metadata/bnf.rs` — BnF API implementation
- `src/metadata/mod.rs` — metadata module exports
- `src/models/metadata_cache.rs` — cache model
- `src/tasks/metadata_fetch.rs` — async fetch chain
- `src/middleware/pending_updates.rs` — PendingUpdates middleware
- `tests/e2e/mock-metadata-server/` — mock server for E2E
- `tests/e2e/specs/journeys/catalog-metadata.spec.ts` — E2E tests

**Files to modify:**
- `Cargo.toml` — add `async-trait = "0.1"`
- `src/config.rs` — add `AppSettings` struct alongside existing `Config` struct
- `src/lib.rs` — update `AppState` with `settings: Arc<RwLock<AppSettings>>` field (**`pub mod config;` already exists — DO NOT re-add**)
- `src/main.rs` — load AppSettings after migrations
- `src/models/mod.rs` — add `pub mod metadata_cache;`
- `src/metadata/mod.rs` — replace stub with real module
- `src/tasks/mod.rs` — replace stub with real module
- `src/middleware/mod.rs` — add `pub mod pending_updates;`
- `src/routes/catalog.rs` — update `handle_scan()` for async flow, add `skeleton_feedback_html()`
- `src/routes/mod.rs` — register PendingUpdates middleware layer
- `static/js/mybibli.js` — skip skeleton entries from fade timer
- `templates/pages/catalog.html` — add `<span id="session-counter">` empty element for OOB target
- `locales/en.yml` — add metadata i18n keys
- `locales/fr.yml` — add French translations
- `tests/e2e/docker-compose.test.yml` — add mock server service

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-1, Story 1.7]
- [Source: _bmad-output/planning-artifacts/prd.md#FR13, #FR60, #FR88, #FR103, #FR104, #FR108, #NFR3, #NFR14, #NFR38, #NFR40]
- [Source: _bmad-output/planning-artifacts/architecture.md#Async-Metadata-Pipeline, #MetadataProvider-Trait, #PendingUpdates-Middleware, #AppSettings, #Scan-Flow]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#FeedbackEntry, #Skeleton, #SessionCounter]
- [Source: _bmad-output/implementation-artifacts/1-6-search-and-browsing.md#Dev-Agent-Record]
- [Source: migrations/20260329000000_initial_schema.sql#pending_metadata_updates, #settings]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- i18n `t!()` keys compiled at build time — new locale keys require clean build to be picked up in tests

### Completion Notes List

- **Task 1:** AppSettings struct with defaults + DB loading via `load_from_db()`, integrated into AppState as `Arc<RwLock<AppSettings>>`
- **Task 2:** MetadataProvider async trait + BnfProvider using SRU API with UNIMARC XML parsing
- **Task 3:** `pending_metadata_updates.status` migration added; MetadataCacheModel with upsert, find_by_isbn (24h TTL), JSON roundtrip
- **Task 4:** Async metadata fetch chain: cache check → BnF provider → title update + cache insert + mark resolved/failed, with configurable timeout
- **Task 5:** PendingUpdates Axum middleware: queries resolved items, renders OOB swap HTML, soft-deletes processed rows. Registered on catalog routes via Extension layer
- **Task 6:** `handle_scan()` updated: new ISBN → check cache → if miss, spawn async task + return skeleton; if cache hit, return resolved immediately; existing ISBN → info feedback as before
- **Task 7:** Skeleton FeedbackEntry with spinner SVG, shimmer bar CSS animation, `prefers-reduced-motion` support, unique `feedback-entry-{title_id}` ID for OOB targeting
- **Task 8:** JS `initFeedbackAutoDismiss()` updated to use `data-resolved-at` for OOB-delivered entries; skeletons use `feedback-skeleton` class (excluded from lifecycle)
- **Task 9:** i18n keys for metadata_fetching, metadata_resolved, metadata_failed, metadata_cached, edit_manually in EN and FR
- **Task 10:** 143 unit tests covering all modules (config, metadata, BnF parsing, cache, middleware, tasks, routes)
- **Task 11:** Playwright E2E tests for skeleton feedback, OOB delivery, session counter, invalid ISBN, duplicate V-code
- **Task 12:** Python mock BnF server with deterministic responses for known ISBNs; docker-compose integration via `BNF_API_BASE_URL` env var

### Change Log

- 2026-03-31: Implemented story 1-7: Scan Feedback & Async Metadata — all 12 tasks complete

### File List

**New files:**
- `migrations/20260331000003_add_pending_status.sql`
- `src/metadata/provider.rs`
- `src/metadata/bnf.rs`
- `src/models/metadata_cache.rs`
- `src/tasks/metadata_fetch.rs`
- `src/middleware/pending_updates.rs`
- `tests/e2e/specs/journeys/catalog-metadata.spec.ts`
- `tests/e2e/mock-metadata-server/server.py`

**Modified files:**
- `Cargo.toml` — added `async-trait = "0.1"`
- `src/config.rs` — added `AppSettings` struct with `load_from_db()` + unit tests
- `src/lib.rs` — added `settings: Arc<RwLock<AppSettings>>` to `AppState`
- `src/main.rs` — load `AppSettings` after migrations
- `src/metadata/mod.rs` — replaced stub with `pub mod provider; pub mod bnf;`
- `src/models/mod.rs` — added `pub mod metadata_cache;`
- `src/tasks/mod.rs` — replaced stub with `pub mod metadata_fetch;`
- `src/middleware/mod.rs` — added `pub mod pending_updates;`
- `src/routes/catalog.rs` — updated `handle_scan()` for async flow, added `skeleton_feedback_html()` + unit tests
- `src/routes/mod.rs` — registered PendingUpdates middleware on catalog routes
- `static/js/mybibli.js` — updated `initFeedbackAutoDismiss()` for skeleton/resolved lifecycle
- `templates/pages/catalog.html` — added `<span id="session-counter">` OOB target
- `locales/en.yml` — added metadata i18n keys
- `locales/fr.yml` — added French metadata translations
- `tests/e2e/docker-compose.test.yml` — added mock-metadata service
