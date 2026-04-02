# Story 3.1: Provider Chain & Fallback Infrastructure

Status: done

## Story

As a librarian,
I want the system to query multiple metadata providers with automatic fallback,
so that I get the best available metadata for any media type I scan.

## Acceptance Criteria (BDD)

### AC1: Provider Registry

**Given** the application starts,
**When** providers are initialized,
**Then** a `ProviderRegistry` holds all configured providers, each declaring which media types it supports via `supports_media_type()`. Providers that strictly require an API key and have none configured are skipped with a `tracing::warn!` log (FR19). Providers that work without a key (e.g., Google Books, Open Library, BnF) are always registered (NFR29).

### AC2: Fallback Chain Execution

**Given** a title needs metadata fetched,
**When** the primary provider returns no result or errors,
**Then** the system tries the next provider in priority order until one succeeds or all are exhausted (FR12). Each provider has a 5-second timeout (NFR6). The global chain timeout is 30 seconds (configurable via `metadata_fetch_timeout_secs` setting, NFR40).

### AC3: Google Books Provider

**Given** a title with an ISBN,
**When** the metadata fetch chain runs for media_type "book",
**Then** Google Books API is queried (after BnF) using the ISBN. If a result is found, title, subtitle, description, authors, publisher, publication_date, cover_url, language, and page_count are extracted (FR11). Works without API key at lower rate limits; optional `GOOGLE_BOOKS_API_KEY` env var enables higher quota.

### AC4: Open Library Provider

**Given** a title with an ISBN,
**When** the metadata fetch chain runs for media_type "book" and BnF + Google Books return no result,
**Then** Open Library API is queried as final fallback. Same fields extracted (FR11). No API key needed.

### AC5: Provider Chain Per Media Type

**Given** the following media type -> provider chains are configured,
**When** metadata is fetched for a title,
**Then** only providers supporting that media type are tried, in priority order:

| Media Type | Chain (priority order) |
|-----------|----------------------|
| book      | BnF -> Google Books -> Open Library |
| bd        | BnF -> Google Books |
| magazine  | BnF |
| cd        | *(no providers yet -- story 3-2)* |
| dvd       | *(no providers yet -- story 3-2)* |
| report    | *(manual only)* |

### AC6: API Key Configuration

**Given** a provider optionally accepts an API key (e.g., Google Books),
**When** the key is configured via env var,
**Then** it is used for higher rate limits. When absent, the provider still registers and works at lower rate limits. API keys are always stored as environment variables, never in the database (NFR14). The system remains fully functional when all external APIs are unavailable (FR85, NFR19).

### AC7: Metadata Cache

**Given** a metadata lookup has been performed for a code,
**When** the same code is queried again within 24 hours,
**Then** the cached result is returned without calling external APIs (NFR36). Cache is per-code, not per-provider. Cache check happens inside the ChainExecutor, not in the route handler.

### AC8: Provider Independence

**Given** any single provider fails, times out, or returns an error,
**When** the chain continues to the next provider,
**Then** the failure is logged with `tracing::warn!` (provider name, error, duration_ms) but does NOT block the chain or the scan loop (NFR17, NFR20). Adding new providers requires no changes to existing provider code (NFR29).

### AC9: Rate Limiting

**Given** Google Books has a rate limit of 1,000 requests/day,
**When** the limit is approached,
**Then** the provider respects rate limits by checking HTTP 429 responses and backing off. MusicBrainz (story 3-2) requires 1 req/sec -- implement a generic rate limiter that providers can opt into (NFR18).

## Explicit Scope Boundaries

**In scope:**
- Provider registry with media type filtering
- Fallback chain execution with per-provider and global timeouts
- Google Books provider (ISBN lookup)
- Open Library provider (ISBN lookup)
- API key configuration via environment variables (NFR14)
- Metadata cache (24h TTL, existing `metadata_cache` table)
- Rate limit handling (HTTP 429 detection + backoff)
- Structured logging for all provider operations
- Refactor BnfProvider to accept shared reqwest::Client
- Unit tests for chain logic, provider parsing
- E2E test: scan ISBN -> verify metadata from fallback provider

**NOT in scope (later stories):**
- MusicBrainz, TMDb, OMDb, BDGest, Comic Vine providers (story 3-2)
- UPC/ISSN scan handling (story 3-2)
- Cover image download/resize (story 3-3)
- Audio feedback (story 3-4)
- Manual metadata editing / re-download (story 3-5)
- Media type disambiguation UI (story 3-2)

## Tasks / Subtasks

- [x] Task 1: Shared reqwest::Client + BnfProvider refactor (AC: 8)
  - [x] 1.1 Add `http_client: reqwest::Client` to `AppState` in `src/lib.rs`. Initialize once at startup in `main.rs` with: 10s connect timeout, 30s total timeout, `User-Agent: mybibli/1.0` header
  - [x] 1.2 Refactor `BnfProvider::new()` to `BnfProvider::new(client: reqwest::Client)` -- accept shared client instead of creating its own. Also refactor `with_base_url()` similarly
  - [x] 1.3 Update `src/tasks/metadata_fetch.rs` to pass shared client when creating BnfProvider
  - [x] 1.4 Update `src/main.rs` AppState initialization:
    ```rust
    // CURRENT:
    let state = AppState { pool, settings: Arc::new(RwLock::new(app_settings)) };
    // TARGET:
    let http_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent("mybibli/1.0")
        .build()
        .expect("Failed to create HTTP client");
    let state = AppState { pool, settings: Arc::new(RwLock::new(app_settings)), http_client, registry: Arc::new(registry) };
    ```
  - [x] 1.5 Unit tests: verify BnfProvider still works with injected client

- [x] Task 2: MediaType enum + ProviderRegistry (AC: 1, 5)
  - [x] 2.1 Create `src/models/media_type.rs` with `MediaType` enum: `Book, Bd, Cd, Dvd, Magazine, Report`. Implement `Display` (lowercase), `FromStr`, `sqlx::Type` (maps to DB enum strings). Add `pub mod media_type;` to `src/models/mod.rs`
  - [x] 2.2 Refactor `MetadataProvider` trait in `src/metadata/provider.rs`: change `supports_media_type(&self, media_type: &str) -> bool` to `supports_media_type(&self, media_type: &MediaType) -> bool`. **BREAKING CHANGE** -- update BnfProvider implementation to match
  - [x] 2.3 Create `src/metadata/registry.rs` with `ProviderRegistry`: holds `Vec<Box<dyn MetadataProvider>>`, method `chain_for(media_type: &MediaType) -> Vec<&dyn MetadataProvider>` filters and returns in registration order (= priority order)
  - [x] 2.4 Add `registry: Arc<ProviderRegistry>` to `AppState` in `src/lib.rs`
  - [x] 2.5 Add `pub mod registry;` to `src/metadata/mod.rs`
  - [x] 2.6 Unit tests: MediaType Display/FromStr round-trip, registry filtering by media type, empty chain for unsupported types, registration order preserved

- [x] Task 3: Fallback chain executor (AC: 2, 7, 8, 9)
  - [x] 3.1 Create `src/metadata/chain.rs` with `ChainExecutor`. Method: `async fn execute(registry: &ProviderRegistry, pool: &DbPool, code: &str, media_type: &MediaType, timeout_secs: u64) -> Option<MetadataResult>`
  - [x] 3.2 Cache check FIRST: call existing `MetadataCacheModel::find_by_isbn(pool, code)`. It already parses JSON and returns `Result<Option<MetadataResult>, AppError>` — use directly, no manual parsing needed. **Note:** `response` column is JSON/BLOB — the existing query already handles `CAST(response AS CHAR)`
  - [x] 3.3 Iterate `registry.chain_for(media_type)`, call `lookup_by_isbn()` on each with `tokio::time::timeout(Duration::from_secs(5))` per provider. Global timeout wraps entire chain
  - [x] 3.4 On first successful result: serialize `MetadataResult` to `serde_json::Value`, then cache via `MetadataCacheModel::upsert(pool: &DbPool, isbn: &str, response_json: &serde_json::Value)`. Return result
  - [x] 3.5 Rate limit: detect HTTP 429, log `tracing::warn!`, skip to next provider
  - [x] 3.6 Structured logging: `tracing::info!` on chain start (code, media_type) and end (provider_used or "none"), `tracing::warn!` on each provider failure/timeout/skip
  - [x] 3.7 Add `pub mod chain;` to `src/metadata/mod.rs`
  - [x] 3.8 Unit tests: chain fallback on failure, timeout handling, cache hit returns early, cache miss runs chain, rate limit skip

- [x] Task 4: Google Books provider (AC: 3, 6)
  - [x] 4.1 Create `src/metadata/google_books.rs` implementing `MetadataProvider`
  - [x] 4.2 Constructor: `GoogleBooksProvider::new(client: reqwest::Client, api_key: Option<String>)`. Struct fields: `client`, `api_key: Option<String>`, `base_url: String`. Read `GOOGLE_BOOKS_API_BASE_URL` env var in constructor (default: `https://www.googleapis.com`). Same testability pattern as BnfProvider
  - [x] 4.3 API endpoint: `GET {base_url}/books/v1/volumes?q=isbn:{isbn}` (append `&key={api_key}` if Some)
  - [x] 4.4 Parse JSON: `items[0].volumeInfo.{title, subtitle, description, authors: [], publisher, publishedDate, pageCount, imageLinks.thumbnail, language}`
  - [x] 4.5 `name()` -> `"google_books"`, `supports_media_type()` -> true for Book, Bd
  - [x] 4.6 Add `pub mod google_books;` to `src/metadata/mod.rs`
  - [x] 4.7 Unit tests: JSON parsing (valid response, empty items, missing fields, partial data), error handling

- [x] Task 5: Open Library provider (AC: 4)
  - [x] 5.1 Create `src/metadata/open_library.rs` implementing `MetadataProvider`
  - [x] 5.2 Constructor: `OpenLibraryProvider::new(client: reqwest::Client)`. Struct fields: `client`, `base_url: String`. Read `OPEN_LIBRARY_API_BASE_URL` env var in constructor (default: `https://openlibrary.org`). Same testability pattern as BnfProvider
  - [x] 5.3 Primary endpoint: `GET {base_url}/isbn/{isbn}.json`
  - [x] 5.4 Author resolution: for each `authors[].key`, call `GET {base_url}/authors/{key}.json` -> extract `name`. Handle failures gracefully (skip author on error). Uses same `base_url` as primary endpoint for testability with mock server
  - [x] 5.5 Cover URL: `https://covers.openlibrary.org/b/id/{covers[0]}-L.jpg` (if covers array non-empty). **Note:** cover URLs are on a separate domain — in E2E tests, cover download is not tested (story 3-3 scope). No base_url override needed here
  - [x] 5.6 Description: handle both `description: "string"` and `description: { value: "string" }` formats
  - [x] 5.7 `name()` -> `"open_library"`, `supports_media_type()` -> true for Book only
  - [x] 5.8 Add `pub mod open_library;` to `src/metadata/mod.rs`
  - [x] 5.9 Unit tests: JSON parsing (both description formats, author resolution, cover URL construction, missing fields)

- [x] Task 6: Add page_count + Serialize/Deserialize to MetadataResult (AC: 3, 4)
  - [x] 6.1 In `src/metadata/provider.rs`, update `MetadataResult` derives to `#[derive(Debug, Clone, Default, Serialize, Deserialize)]` and add `pub page_count: Option<i32>` field. The Serialize/Deserialize derives enable `serde_json::to_value(&result)` for cache upsert (Task 3.4). **Note:** existing manual serialization in `metadata_cache.rs` (lines ~103-113 and ~56-100) can be simplified to use `serde_json::to_value()` / `serde_json::from_value()` — refactor if time permits, or leave as-is (both approaches work)
  - [x] 6.2 Update BnfProvider to populate page_count if available from UNIMARC data
  - [x] 6.3 Extend `update_title_from_metadata()` SQL UPDATE in `src/tasks/metadata_fetch.rs` (lines 92-131): add `page_count = COALESCE(?, page_count)` to the SET clause and bind `metadata.page_count`. **Note:** function is called `update_title_from_metadata`, NOT `apply_metadata_to_title`. Also called by `apply_cached_metadata()` (line 205) — verify both code paths

- [x] Task 7: Refactor metadata_fetch.rs to use chain (AC: 2, 5)
  - [x] 7.1 Change `fetch_metadata_chain()` signature from `(pool: DbPool, title_id: u64, isbn: String, timeout_secs: u64)` to `(pool: DbPool, title_id: u64, isbn: String, media_type: MediaType, registry: Arc<ProviderRegistry>, timeout_secs: u64)`
  - [x] 7.2 Replace direct `BnfProvider::new()` call with `ChainExecutor::execute(&registry, &pool, &isbn, &media_type, timeout_secs)`
  - [x] 7.3 Simplify `src/routes/catalog.rs` scan handler: remove the cache check (line ~284) AND the two conditional branches that follow. **Current flow has two paths:** (a) cache HIT + new title (lines ~333-354): spawns `apply_cached_metadata()`, returns success feedback; (b) cache MISS + new title (lines ~357-368): spawns `fetch_metadata_chain()`, returns skeleton. **New flow:** always spawn `fetch_metadata_chain()`, always return skeleton feedback. ChainExecutor handles cache internally and returns fast on cache hit. The `apply_cached_metadata()` call from catalog.rs is no longer needed
  - [x] 7.3b Remove the internal cache check in `src/tasks/metadata_fetch.rs` `fetch_metadata_inner()` (lines ~58-69: `MetadataCacheModel::find_by_isbn`). ChainExecutor now handles cache — this check is redundant
  - [x] 7.4 Update spawn call site in `src/routes/catalog.rs` (around line 363). Get `media_type` from the created title: `let media_type = title.media_type.parse::<MediaType>().unwrap_or(MediaType::Book);` (currently always "book" — story 3-2 changes this):
    ```rust
    // CURRENT:
    tokio::spawn(fetch_metadata_chain(pool.clone(), title.id, code.clone(), timeout_secs));
    // TARGET:
    let media_type = title.media_type.parse::<MediaType>().unwrap_or(MediaType::Book);
    tokio::spawn(fetch_metadata_chain(pool.clone(), title.id, code.clone(), media_type, state.registry.clone(), timeout_secs));
    ```
  - [x] 7.5 Keep existing `update_title_from_metadata()` logic and `pending_metadata_updates` OOB flow unchanged

- [x] Task 8: Provider registration in main.rs (AC: 1, 6)
  - [x] 8.1 Build registry at startup in `src/main.rs`:
    ```rust
    let mut registry = ProviderRegistry::new();
    registry.register(Box::new(BnfProvider::new(http_client.clone())));
    // Google Books: always registered, optional key for higher rate limits
    let gb_key = std::env::var("GOOGLE_BOOKS_API_KEY").ok();
    registry.register(Box::new(GoogleBooksProvider::new(http_client.clone(), gb_key)));
    registry.register(Box::new(OpenLibraryProvider::new(http_client.clone())));
    ```
  - [x] 8.2 Log registered providers at startup: `tracing::info!("Registered {} metadata providers", registry.len())`

- [x] Task 9: i18n keys (AC: all)
  - [x] 9.1 Add keys to `locales/en.yml`: `metadata.provider_failed`, `metadata.no_result`, `metadata.cached_result`, `metadata.chain_timeout`
  - [x] 9.2 Add French translations to `locales/fr.yml`
  - [x] 9.3 Run `touch src/lib.rs` before build

- [x] Task 10: E2E tests (AC: 2, 3, 4)
  - [x] 10.1 Extend mock metadata server at `tests/e2e/mock-metadata-server/server.py` (Python). Currently only serves BnF SRU/UNIMARC XML. Add routes for Google Books JSON API: `GET /books/v1/volumes?q=isbn:{isbn}` returning `{ items: [{ volumeInfo: {...} }] }`. Use a test ISBN that BnF returns 404 for but Google Books returns valid data
  - [x] 10.2 Update `docker-compose.test.yml` to set `GOOGLE_BOOKS_API_BASE_URL=http://mock-metadata:9090` and `OPEN_LIBRARY_API_BASE_URL=http://mock-metadata:9090` (new env vars, same pattern as `BNF_API_BASE_URL`)
  - [x] 10.3 Test: scan ISBN -> metadata arrives from fallback provider (Google Books), verify title/author populated
  - [x] 10.4 Test: scan ISBN -> all providers fail -> title exists with no metadata, no blocking error

### Review Findings

- [x] [Review][Patch] cover_url never written to DB in update_title_from_metadata [src/tasks/metadata_fetch.rs:55] — MetadataResult.cover_url populated by Google Books/Open Library but SQL UPDATE does not include cover_image_url column
- [x] [Review][Patch] Open Library author resolution unbounded — no cap on HTTP calls, no per-call timeout [src/metadata/open_library.rs:30] — cap at 5 authors max to avoid eating 5s per-provider timeout
- [x] [Review][Patch] Google Books API key not URL-encoded in query string [src/metadata/google_books.rs:112] — use url-encoding; also risk of key appearing in logs
- [x] [Review][Patch] SSRF risk via Open Library author key path [src/metadata/open_library.rs:34] — validate key matches `/authors/OL` prefix before HTTP call
- [x] [Review][Patch] ChainExecutor async unit tests missing per Task 3.8 [src/metadata/chain.rs] — add async tests for fallback, timeout, cache hit/miss, rate limit skip
- [x] [Review][Patch] BnF page_count not extracted from UNIMARC per Task 6.2 [src/metadata/bnf.rs] — attempt to parse UNIMARC 215$a for page count
- [x] [Review][Patch] Open Library cover ID -1 produces invalid cover URL [src/metadata/open_library.rs:78] — filter out negative cover IDs
- [x] [Review][Patch] Google Books thumbnail URL uses HTTP not HTTPS [src/metadata/google_books.rs:77] — rewrite http:// to https://
- [x] [Review][Defer] timeout_secs=0 causes instant global timeout — no validation/minimum [src/metadata/chain.rs:50] — deferred, pre-existing settings validation gap
- [x] [Review][Defer] Per-provider timeout 5s hardcoded, not configurable or related to global timeout [src/metadata/chain.rs:58] — deferred, design simplification acceptable for single-user NAS
- [x] [Review][Defer] Rate limit detection via string matching, no generic rate limiter struct [src/metadata/chain.rs:83] — deferred, proactive rate limiter planned for story 3-2 (MusicBrainz)

## Dev Notes

### Architecture Compliance

- **Service layer:** Chain execution logic in `src/metadata/chain.rs`, NOT in route handlers
- **Error handling:** Provider errors are `tracing::warn!`, never `AppError` -- they don't propagate to user as HTTP errors. The chain returns `Option<MetadataResult>`, None = no metadata found
- **Logging:** `tracing` macros with structured fields: `provider`, `isbn`, `duration_ms`, `media_type`, `title_id`
- **i18n:** `t!("key")` for any user-facing messages. Run `touch src/lib.rs` after locale changes
- **DB queries:** `WHERE deleted_at IS NULL` on all SELECTs
- **Pool access:** `pool: &DbPool` from `AppState`. For spawned tasks: `pool.clone()`
- **API keys:** Environment variables only, never in DB (NFR14)
- **Open/closed:** New providers = implement trait + register. No changes to existing providers (NFR29)

### Existing Code (DO NOT recreate)

| File | What exists | Action |
|------|------------|--------|
| `src/metadata/provider.rs` | `MetadataProvider` trait (uses `&str` for media_type), `MetadataResult` struct (derives Debug/Clone/Default only, no page_count), `MetadataError` enum | Extend: change `&str` to `&MediaType`, add `Serialize, Deserialize` derives + `page_count` field |
| `src/metadata/bnf.rs` | `BnfProvider` with `new()` (creates own Client), `with_base_url()`, supports book/bd/magazine | Refactor: accept `reqwest::Client` param, update `supports_media_type` to use `&MediaType` |
| `src/metadata/mod.rs` | Declares `pub mod bnf; pub mod provider;` | Add: `pub mod google_books; pub mod open_library; pub mod registry; pub mod chain;` |
| `src/tasks/metadata_fetch.rs` | `fetch_metadata_chain(pool, title_id, isbn, timeout_secs)` calls `BnfProvider::new()` directly. `update_title_from_metadata()` (lines 92-131) updates title/subtitle/description/publisher/language (NO page_count yet). `apply_cached_metadata()` (line 205) calls `update_title_from_metadata()` | Refactor: new params (media_type, registry), use ChainExecutor. Extend SQL to include page_count |
| `src/models/metadata_cache.rs` | `find_by_isbn(pool, code) -> Result<Option<MetadataResult>>` (already parses JSON!). `upsert(pool, isbn, response_json: &serde_json::Value)` | Reuse as-is from ChainExecutor. Serialize MetadataResult to serde_json::Value for upsert |
| `src/lib.rs` | `AppState { pool: DbPool, settings: Arc<RwLock<AppSettings>> }` | Add: `http_client: reqwest::Client`, `registry: Arc<ProviderRegistry>` |
| `src/routes/catalog.rs` | Cache check at ~line 284, two conditional branches: cache HIT spawns `apply_cached_metadata()` (lines ~333-354), cache MISS spawns `fetch_metadata_chain()` (lines ~357-368) | Remove cache check + both branches. Single path: always spawn `fetch_metadata_chain()` with new params. ChainExecutor handles cache |
| `src/models/mod.rs` | Existing module declarations | Add: `pub mod media_type;` |

### Actual Database Schema

```sql
-- titles table (media_type is DB ENUM, stored as String in Rust)
CREATE TABLE titles (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    title VARCHAR(500) NOT NULL,
    subtitle VARCHAR(500) NULL,
    description TEXT NULL,
    language VARCHAR(10) NOT NULL DEFAULT 'fr',
    media_type ENUM('book','bd','cd','dvd','magazine','report') NOT NULL DEFAULT 'book',
    publication_date DATE NULL,
    publisher VARCHAR(255) NULL,
    isbn VARCHAR(13) NULL,
    issn VARCHAR(8) NULL,
    upc VARCHAR(13) NULL,
    cover_image_url VARCHAR(500) NULL,
    genre_id BIGINT UNSIGNED NOT NULL,
    dewey_code VARCHAR(20) NULL,
    page_count INT NULL,
    track_count INT NULL,
    total_duration INT NULL,
    age_rating VARCHAR(20) NULL,
    issue_number INT NULL,
    version INT NOT NULL DEFAULT 1,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL
);

-- metadata_cache table -- CRITICAL: response column is JSON stored as BLOB
-- Use CAST(response AS CHAR) to read it (MariaDB JSON/BLOB pattern)
CREATE TABLE metadata_cache (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    code VARCHAR(20) NOT NULL,
    response JSON,
    fetched_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL,
    UNIQUE KEY uq_metadata_cache_code (code)
);
```

### Current AppState (exact)

```rust
// CURRENT (src/lib.rs):
#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub settings: Arc<RwLock<AppSettings>>,
}

// TARGET after this story:
#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub settings: Arc<RwLock<AppSettings>>,
    pub http_client: reqwest::Client,
    pub registry: Arc<ProviderRegistry>,
}
```

### Current MetadataResult (exact)

```rust
// CURRENT (src/metadata/provider.rs):
#[derive(Debug, Clone, Default)]
pub struct MetadataResult {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
    pub cover_url: Option<String>,
    pub language: Option<String>,
}

// ADD in this story:
//   - Add derives: Serialize, Deserialize (for cache serialization)
//   - Add field: pub page_count: Option<i32>,
```

### Current MetadataProvider Trait (exact)

```rust
// CURRENT (src/metadata/provider.rs):
#[async_trait]
pub trait MetadataProvider: Send + Sync {
    fn name(&self) -> &str;
    fn supports_media_type(&self, media_type: &str) -> bool;  // CHANGE to &MediaType
    async fn lookup_by_isbn(&self, isbn: &str) -> Result<Option<MetadataResult>, MetadataError>;
    async fn lookup_by_upc(&self, _upc: &str) -> Result<Option<MetadataResult>, MetadataError> { Ok(None) }
    async fn search_by_title(&self, _title: &str) -> Result<Option<MetadataResult>, MetadataError> { Ok(None) }
}
```

### Google Books API

- Endpoint: `GET https://www.googleapis.com/books/v1/volumes?q=isbn:{isbn}`
- Optional: `&key={GOOGLE_BOOKS_API_KEY}` for higher rate limits
- Response: `{ items: [{ volumeInfo: { title, subtitle, description, authors: [], publisher, publishedDate, pageCount, imageLinks: { thumbnail }, language } }] }`
- Rate limits: 1,000/day without key, higher with key
- Works unauthenticated (no key required)
- **Testability:** Use `GOOGLE_BOOKS_API_BASE_URL` env var to override base URL (same pattern as BnF). Default: `https://www.googleapis.com`. In E2E tests: `http://mock-metadata:9090`

### Open Library API

- Endpoint: `GET https://openlibrary.org/isbn/{isbn}.json`
- No API key needed
- Response: `{ title, subtitle, description: {value} | string, authors: [{key: "/authors/OL..."}], publishers: [], publish_date, covers: [id1, id2] }`
- Author resolution: `GET https://openlibrary.org/authors/{key}.json` -> `{ name }`
- Cover images: `https://covers.openlibrary.org/b/id/{cover_id}-L.jpg`
- **Testability:** Use `OPEN_LIBRARY_API_BASE_URL` env var to override base URL. Default: `https://openlibrary.org`. In E2E tests: `http://mock-metadata:9090`

### Mock Metadata Server (E2E)

- **Location:** `tests/e2e/mock-metadata-server/server.py` (Python 3.12)
- **Currently serves:** BnF SRU/UNIMARC XML only (3 known test ISBNs)
- **This story adds:** Google Books JSON routes + Open Library JSON routes
- **Docker service:** `mock-metadata` on port 9090 in `docker-compose.test.yml`
- **Known test ISBNs:** `9782070360246` (L'Etranger), `9780306406157` (Art of Electronics), `9791032305560` (Les Miserables)

### Previous Story Intelligence

From Epic 2 retrospective:
- **MariaDB type mapping quirks** -- BIGINT UNSIGNED, JSON as BLOB. Use `CAST(col AS CHAR)` for JSON columns
- **OOB swap pattern** proven and stable -- reuse for metadata delivery
- **i18n rebuild rule** -- always `touch src/lib.rs` after locale changes
- **reqwest::Client** created per BnF request (deferred-work.md) -- **this story fixes it**
- **Fire-and-forget spawned tasks** with no backpressure -- acceptable for single-user NAS
- **scan-field.js** ISSN (977) vs UPC prefix overlap -- addressed in story 3-2

### References

- [Source: _bmad-output/planning-artifacts/prd.md#FR11, #FR12, #FR19, #FR85]
- [Source: _bmad-output/planning-artifacts/prd.md#NFR6, #NFR14, #NFR16-NFR20, #NFR29, #NFR36, #NFR40]
- [Source: _bmad-output/planning-artifacts/architecture.md#MetadataProvider-Trait, #Fallback-Chain]
- [Source: _bmad-output/planning-artifacts/architecture.md#Async-Metadata-Flow]
- [Source: _bmad-output/implementation-artifacts/deferred-work.md#reqwest-client-reuse]

## Dev Agent Record

### Agent Model Used
Claude Opus 4.6 (1M context)

### Debug Log References
- Pre-existing clippy warnings fixed in locations.rs, catalog.rs, services/locations.rs, middleware/pending_updates.rs, models/title.rs, services/contributor.rs, metadata_cache.rs
- Restored missing .sqlx offline cache files from git history (f695757)

### Completion Notes List
- Task 1: Added shared `reqwest::Client` to AppState, refactored BnfProvider to accept injected client
- Task 2: Created MediaType enum with Display/FromStr, ProviderRegistry with chain_for() filtering by media type
- Task 3: Created ChainExecutor with cache-first check, per-provider 5s timeout, global timeout, rate limit (429) detection, structured logging
- Task 4: Implemented GoogleBooksProvider — ISBN lookup, JSON parsing, optional API key support
- Task 5: Implemented OpenLibraryProvider — ISBN lookup, author key resolution, dual description format handling
- Task 6: Added page_count field + Serialize/Deserialize derives to MetadataResult, updated SQL UPDATE and cache serialization
- Task 7: Refactored metadata_fetch.rs to use ChainExecutor, simplified catalog.rs scan handler (removed cache check + two branches → single spawn path)
- Task 8: Built provider registry at startup in main.rs: BnF → Google Books → Open Library
- Task 9: Added i18n keys for metadata.provider_failed, metadata.no_result, metadata.cached_result, metadata.chain_timeout
- Task 10: Extended mock server with Google Books + Open Library endpoints, added docker-compose env vars, created provider-chain.spec.ts E2E tests

### Change Log
- 2026-04-02: Story 3-1 implemented — provider chain infrastructure with BnF, Google Books, Open Library providers

### File List
- src/lib.rs (modified) — Added http_client, registry to AppState
- src/main.rs (modified) — Added HTTP client creation, provider registry setup
- src/metadata/mod.rs (modified) — Added chain, google_books, open_library, registry modules
- src/metadata/provider.rs (modified) — Added Serialize/Deserialize + page_count to MetadataResult, changed supports_media_type to &MediaType
- src/metadata/bnf.rs (modified) — Refactored to accept shared reqwest::Client, use &MediaType
- src/metadata/chain.rs (new) — ChainExecutor with cache, fallback, timeout, rate limit handling
- src/metadata/google_books.rs (new) — Google Books API provider
- src/metadata/open_library.rs (new) — Open Library API provider with author resolution
- src/metadata/registry.rs (new) — ProviderRegistry with chain_for() filtering
- src/models/mod.rs (modified) — Added media_type module
- src/models/media_type.rs (new) — MediaType enum with Display, FromStr
- src/models/metadata_cache.rs (modified) — Added page_count to parse/serialize
- src/tasks/metadata_fetch.rs (modified) — Refactored to use ChainExecutor, removed apply_cached_metadata
- src/routes/catalog.rs (modified) — Simplified scan handler: removed cache check, single spawn path
- src/routes/locations.rs (modified) — Fixed unused imports (clippy)
- src/middleware/pending_updates.rs (modified) — Fixed clippy warnings
- src/models/title.rs (modified) — Added clippy allow for too_many_arguments
- src/services/locations.rs (modified) — Fixed collapsible_if (clippy)
- src/services/contributor.rs (modified) — Removed unused import (clippy)
- locales/en.yml (modified) — Added metadata.* i18n keys
- locales/fr.yml (modified) — Added metadata.* i18n keys (French)
- tests/e2e/mock-metadata-server/server.py (modified) — Added Google Books + Open Library mock endpoints
- tests/e2e/docker-compose.test.yml (modified) — Added GOOGLE_BOOKS_API_BASE_URL, OPEN_LIBRARY_API_BASE_URL env vars
- tests/e2e/specs/journeys/provider-chain.spec.ts (new) — E2E tests for provider fallback and all-fail scenarios
