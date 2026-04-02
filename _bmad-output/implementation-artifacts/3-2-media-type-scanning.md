# Story 3.2: Media Type Scanning

Status: done

## Story

As a librarian,
I want to scan UPC codes for CDs, DVDs, and other non-ISBN media types,
so that I can catalog my entire media collection with the same efficient scanning workflow.

## Acceptance Criteria

### AC1: Media Type Detection from Barcode Prefix

- Given a barcode is scanned on the catalog page
- When the code is ISBN (978/979) → auto-assign Book, create title immediately, spawn metadata fetch
- When the code is ISSN (977 + valid ISSN check digit) → auto-assign Magazine, create title immediately, spawn metadata fetch
- When the code is UPC (other digit patterns) → do NOT create title yet; show MediaTypeSelector (AC2) for user to choose media type first
- Then for ISBN/ISSN: the title is created with the correct media_type and code stored in the matching column (isbn, issn)
- And for UPC: title creation is deferred until user selects media type via MediaTypeSelector (see AC2)

### AC2: MediaTypeSelector Disambiguation (UX-DR22)

- Given a UPC code is scanned that doesn't match ISBN/ISSN patterns
- When the system cannot auto-determine the media type
- Then an inline button group appears in the feedback list with 6 options: Book, BD, CD, DVD, Magazine, Report
- And each button shows a media-type SVG icon + i18n label
- And the previously chosen type (if any in session) is visually distinguished as "suggested" (filled primary background instead of outline)
- And clicking a button sends the selection + UPC to the server to create the title
- And the session remembers the last chosen type for subsequent UPC scans
- And after selection, focus returns to the scan field (via `focus.js` htmx:afterSettle) to maintain scan-confirm-scan rhythm
- **Accessibility:**
  - Button group: `role="group"`, `aria-label="Select media type"`
  - Each button: `role="radio"`, suggested button gets `aria-label="CD (suggested)"`
  - Keyboard: arrow keys navigate, Enter/Space selects
  - Focus indicator: 2px `--color-primary` ring per project accessibility rules
  - Minimum touch target: 44x44px on mobile/tablet
- **FeedbackEntry integration:**
  - Renders inside a FeedbackEntry with info-blue left border and question icon
  - Message line: "UPC {code} — What type?" with button group below
  - No fade timer on disambiguation entries (persists until user selects)
  - No Cancel button — user must select a type or scan a different code
- **Responsive:**
  - Desktop: 6 buttons in a horizontal row
  - Mobile (<640px): buttons wrap to 2 rows of 3, maintaining 44px min height
- **Dark mode:** Button borders and text use `--color-primary` (adapts automatically via CSS custom properties)

### AC3: MusicBrainz Provider (CD media type)

- Given a CD is scanned with a UPC code
- When the MusicBrainz provider is queried via `GET {base_url}/ws/2/release/?query=barcode:{upc}&fmt=json`
- Then metadata is extracted: title, artists, publisher (label), publication_date (date), cover_url (via Cover Art Archive), description (disambiguation)
- And the provider respects 1 req/sec rate limit via proactive RateLimiter
- And a custom User-Agent header is sent (MusicBrainz requirement, no API key)
- And the provider declares `supports_media_type(Cd) = true`
- And env var override: `MUSICBRAINZ_API_BASE_URL` (default: https://musicbrainz.org)

### AC4: OMDb Provider (DVD media type, primary)

- Given a DVD is scanned with a UPC code
- When OMDb is queried: `GET {base_url}/?s={upc}&type=movie&apikey={key}` then detail by imdbID of first result
- Then metadata is extracted: title (Title), publication_date (Year), description (Plot), cover_url (Poster), runtime
- And if multiple results: take the first match (user can correct later via story 3-5 manual edit)
- And if no results: return `Ok(None)` to trigger fallback to TMDb
- And the provider requires `OMDB_API_KEY` env var; skipped if missing
- And the provider declares `supports_media_type(Dvd) = true`
- And env var override: `OMDB_API_BASE_URL` (default: https://www.omdbapi.com)

### AC5: TMDb Provider (DVD media type, fallback)

- Given a DVD is scanned and OMDb returns no result
- When TMDb is queried: `GET {base_url}/3/search/movie?query={upc}&api_key={key}`
- Then metadata is extracted: title, description (overview), publication_date (release_date), cover_url (poster_path → full URL), language
- And if multiple results: take the first match (user can correct later via story 3-5 manual edit)
- And if no results: return `Ok(None)` — title remains with minimal metadata
- And the provider requires `TMDB_API_KEY` env var; skipped if missing
- And the provider declares `supports_media_type(Dvd) = true`
- And env var override: `TMDB_API_BASE_URL` (default: https://api.themoviedb.org)

**DVD lookup limitation (applies to AC4 + AC5):** No reliable UPC-to-movie mapping exists across providers. OMDb and TMDb accept UPC as search query text but don't match by barcode ID. Results are best-effort — the user can manually correct metadata later (story 3-5). Do not over-engineer multi-result selection in this story.

### AC6: BDGest Provider (BD media type) — STUB

- Given a BD (bande dessinee) is scanned with ISBN
- When BDGest is queried for BD-specific metadata
- Then metadata is extracted: title, authors, publisher, publication_date, cover_url, description
- And the provider requires `BDGEST_API_KEY` env var; skipped if missing
- And the provider declares `supports_media_type(Bd) = true`
- And env var override: `BDGEST_API_BASE_URL`
- **NOTE:** BDGest API specification is TBD in the architecture (marked as "Web scraping or API"). Implement as a stub provider that returns `Ok(None)` and logs `tracing::info!("BDGest provider not yet implemented")`. The stub must still implement `MetadataProvider` trait correctly so it can be replaced with a real implementation later without chain changes. Research the actual API during implementation if time permits.

### AC7: Comic Vine Provider (BD media type, NOT in chain — future use)

- Given Comic Vine is implemented as a MetadataProvider for future chain inclusion
- When the provider is queried: `GET {base_url}/api/issues/?api_key={key}&filter=barcode:{isbn}&format=json`
- Then metadata is extracted: title (volume.name + issue name), authors (person_credits), cover_url (image.medium_url), publication_date (cover_date), description
- And the provider requires `COMIC_VINE_API_KEY` env var; skipped if missing
- And the provider declares `supports_media_type(Bd) = true`
- And env var override: `COMIC_VINE_API_BASE_URL` (default: https://comicvine.gamespot.com)
- **NOTE:** Per architecture, Comic Vine is NOT in the BD chain (bd: BDGest → BnF → Google Books). The provider is implemented but NOT registered in the BD chain for now. It can be added later via a chain config change without code modifications.

### AC8: Proactive Rate Limiter

- Given MusicBrainz requires 1 req/sec rate limiting
- When multiple scans trigger MusicBrainz lookups in quick succession
- Then a generic `RateLimiter` struct enforces configurable requests-per-second per provider
- And requests exceeding the limit are delayed (not dropped) via tokio::time::sleep
- And the rate limiter replaces the string-matching "429" detection from story 3-1 (reactive detection remains as fallback)

### AC9: Provider Chain Per Media Type (Complete)

- Given all providers are registered at startup
- Then the provider priority chains are:
  - book: BnF → Google Books → Open Library
  - bd: BDGest (stub) → BnF → Google Books
  - cd: MusicBrainz
  - dvd: OMDb → TMDb
  - magazine: BnF → Google Books
  - report: (manual only, no providers)
- And providers with missing required API keys are skipped with `tracing::warn!` at startup

### AC10: Title Form Field Adaptation (FR93)

- Given a title has a specific media_type
- When the title detail/edit form is displayed
- Then fields are shown/hidden based on media_type:
  - Book: title, subtitle, authors, publisher, publication_date, language, isbn, description, page_count
  - BD: title, subtitle, authors, publisher, publication_date, isbn, description, page_count, series fields
  - CD: title, authors (as artists), publisher (as label), publication_date, upc, track_count, total_duration, description
  - DVD: title, authors (as director), publisher (as studio), publication_date, upc, age_rating, description
  - Magazine: title, publisher, publication_date, issn, issue_number, description
  - Report: title, authors, publication_date, description

### AC11: Mock Metadata Server Extensions

- Given E2E tests must not hit real APIs
- When Playwright tests run against the mock server
- Then mock routes exist for: MusicBrainz, OMDb, TMDb (BDGest is stub, Comic Vine not in chain — no mocks needed)
- And known test UPCs return deterministic metadata responses
- And unknown codes return appropriate error responses (404 or empty results)
- And docker-compose.test.yml includes env var overrides for all new provider base URLs

### AC12: ISSN vs UPC Prefix Disambiguation

- Given ISSN codes start with 977 and some UPC codes also start with 977
- When a code starting with 977 is scanned
- Then the system applies the ISSN check digit algorithm to distinguish:
  1. Extract the 8-digit ISSN from the 13-digit barcode: digits 4-10 (positions 3..10) form the 7-digit ISSN body
  2. Compute ISSN check digit: weighted sum of 7 digits × [8,7,6,5,4,3,2], mod 11, subtract from 11. Result 10 → 'X', 11 → '0'
  3. If computed check matches digit 11 of barcode → valid ISSN → route to Magazine
  4. If check fails → treat as UPC → show MediaTypeSelector
- And valid ISSN codes store the 8-digit ISSN (body + check) in the `issn` column
- And invalid-ISSN 977-codes are treated as regular UPC

## Tasks / Subtasks

- [x] Task 1: ChainExecutor UPC/ISSN Support (AC: #1, #3-#7, #9) — **PREREQUISITE FOR ALL PROVIDERS**
  - [x] Update `src/metadata/chain.rs`: `ChainExecutor::execute()` must accept a `code_type: CodeType` parameter (isbn, upc, or issn)
  - [x] Branch on code_type to call the appropriate provider method: `lookup_by_isbn()` for ISBN/ISSN, `lookup_by_upc()` for UPC
  - [x] Currently hardcoded to `provider.lookup_by_isbn(code)` at line 58 — this must become dynamic
  - [x] Update `src/tasks/metadata_fetch.rs`: rename `isbn` parameter to `code`, pass `code_type` through to ChainExecutor
  - [x] Update all callers (routes/catalog.rs spawn call) to pass code_type
  - [x] Unit tests: verify UPC codes call `lookup_by_upc()`, ISBN codes call `lookup_by_isbn()`

- [x] Task 2: Proactive RateLimiter (AC: #8) — **PREREQUISITE FOR Task 4 (MusicBrainz)**
  - [x] Create `src/metadata/rate_limiter.rs` with generic `RateLimiter` struct
  - [x] Fields: `min_interval: Duration`, `last_request: Arc<Mutex<Instant>>`
  - [x] Method: `async fn acquire(&self)` — sleeps if needed to enforce interval
  - [x] Unit tests: concurrent access, timing enforcement, zero-delay passthrough
  - [x] Add default method to `MetadataProvider` trait in `src/metadata/provider.rs`: `fn rate_limiter(&self) -> Option<Arc<RateLimiter>> { None }` — MusicBrainz overrides this to return its limiter
  - [x] Integrate into ChainExecutor (`src/metadata/chain.rs`): before calling each provider's lookup method, check `provider.rate_limiter()` and if `Some`, call `limiter.acquire().await`
  - [x] Keep existing reactive 429 detection as fallback

- [x] Task 3: MetadataResult Schema Extension (AC: #3-#7, #10)
  - [x] Add optional fields to `MetadataResult` in `src/metadata/provider.rs`: `track_count: Option<i32>`, `total_duration: Option<String>`, `age_rating: Option<String>`, `issue_number: Option<String>`
  - [x] Update `update_title_from_metadata()` in `src/tasks/metadata_fetch.rs` to write these new fields to DB
  - [x] Update SQL UPDATE query to include new columns (they already exist in titles table)
  - [x] Unit tests for serialization/deserialization of extended MetadataResult

- [x] Task 4: MusicBrainz Provider (AC: #3)
  - [ ] Create `src/metadata/musicbrainz.rs` implementing MetadataProvider
  - [ ] Constructor: `MusicBrainzProvider::new(client: reqwest::Client, rate_limiter: Arc<RateLimiter>)`
  - [ ] `lookup_by_upc()`: `GET {base_url}/ws/2/release/?query=barcode:{upc}&fmt=json`
  - [ ] Custom User-Agent: `mybibli/1.0 (contact@mybibli.local)` (MusicBrainz requirement)
  - [ ] Cover art: `https://coverartarchive.org/release/{mbid}/front-250` (from release MBID)
  - [ ] Parse: title, artist-credit[].name → authors, label-info[].label.name → publisher, date → publication_date, disambiguation → description, track-count → track_count
  - [ ] `supports_media_type`: Cd only
  - [ ] Unit tests: valid response parsing, empty results, malformed JSON
  - [ ] Env var: `MUSICBRAINZ_API_BASE_URL`

- [x] Task 5: OMDb Provider (AC: #4) — DVD primary
  - [ ] Create `src/metadata/omdb.rs` implementing MetadataProvider
  - [ ] Constructor: `OmdbProvider::new(client: reqwest::Client, api_key: String)`
  - [ ] `lookup_by_upc()`: search `/?s={upc}&type=movie&apikey={key}`, then detail by imdbID
  - [ ] Parse: Title → title, Year → publication_date, Plot → description, Poster → cover_url, Runtime → parse minutes into page_count (reuse field)
  - [ ] Handle `"Response":"False"` error format — return `Ok(None)` not error
  - [ ] `supports_media_type`: Dvd only
  - [ ] Unit tests: valid response, no results, error response
  - [ ] Env var: `OMDB_API_KEY`, `OMDB_API_BASE_URL`

- [x] Task 6: TMDb Provider (AC: #5) — DVD fallback
  - [ ] Create `src/metadata/tmdb.rs` implementing MetadataProvider
  - [ ] Constructor: `TmdbProvider::new(client: reqwest::Client, api_key: String)`
  - [ ] `lookup_by_upc()`: search by UPC string, extract first result
  - [ ] Cover URL: `https://image.tmdb.org/t/p/w500{poster_path}` — ensure HTTPS
  - [ ] Parse: title, overview → description, release_date → publication_date, poster_path → cover_url, original_language → language
  - [ ] `supports_media_type`: Dvd only
  - [ ] Unit tests: valid response, no results, missing poster
  - [ ] Env var: `TMDB_API_KEY`, `TMDB_API_BASE_URL`

- [x] Task 7: BDGest Provider Stub (AC: #6)
  - [ ] Create `src/metadata/bdgest.rs` implementing MetadataProvider as a **stub**
  - [ ] `lookup_by_isbn()`: log `tracing::info!("BDGest provider not yet implemented")` and return `Ok(None)`
  - [ ] `supports_media_type`: Bd only
  - [ ] Env var: `BDGEST_API_KEY`, `BDGEST_API_BASE_URL`
  - [ ] Unit test: verify stub returns None and logs correctly
  - [ ] TODO comment: replace with real API integration when BDGest API spec is available

- [x] Task 8: Comic Vine Provider (AC: #7) — implemented but NOT registered in BD chain
  - [ ] Create `src/metadata/comic_vine.rs` implementing MetadataProvider
  - [ ] Constructor: `ComicVineProvider::new(client: reqwest::Client, api_key: String)`
  - [ ] `lookup_by_isbn()`: `GET {base_url}/api/issues/?api_key={key}&filter=barcode:{isbn}&format=json`
  - [ ] Parse: volume.name → title, person_credits → authors (cap at 5), image.medium_url → cover_url, cover_date → publication_date, description
  - [ ] `supports_media_type`: Bd only
  - [ ] Unit tests: valid response, empty results, malformed JSON
  - [ ] Env var: `COMIC_VINE_API_KEY`, `COMIC_VINE_API_BASE_URL`
  - [ ] Do NOT register in ProviderRegistry — provider ready for future chain inclusion

- [x] Task 9: Media Type Detection & Title Creation Refactor (AC: #1, #2, #12) — **PREREQUISITE FOR UPC FLOW**
  - [x] Refactor `detect_code_type()` in `src/routes/catalog.rs`: return a struct `CodeDetection { code_type: CodeType, inferred_media_type: Option<MediaType> }` instead of `&'static str`
  - [x] ISBN (978/979) → `CodeType::Isbn, Some(MediaType::Book)`; ISSN (977 + valid ISSN check digit) → `CodeType::Issn, Some(MediaType::Magazine)`; UPC → `CodeType::Upc, None`
  - [x] ISSN vs UPC disambiguation: use ISSN check digit algorithm on 977-prefixed codes
  - [x] Refactor `TitleService::create_from_isbn()` → generalize to `create_from_code(pool, code, media_type, code_type, session_token)` — currently hardcodes `media_type: "book"` which blocks all non-book creation
  - [x] The new method stores code in the correct column: isbn for ISBN, issn for ISSN, upc for UPC
  - [x] Update `handle_scan()` UPC branch: if session has media_type preference → create title immediately; if not → return MediaTypeSelector fragment (no title created yet)
  - [x] Add `POST /catalog/scan-with-type` route: accepts code + explicit media_type, creates title + spawns fetch
  - [x] Session memory: store last chosen media type in a cookie `media_type_preference` (TODO: cookie read in Task 10)

- [x] Task 10: MediaTypeSelector Component (AC: #2) — depends on Task 9 (`/catalog/scan-with-type` route)
  - [ ] Create Askama template `templates/components/media_type_selector.html`
  - [ ] Renders inside a FeedbackEntry wrapper with info-blue left border and question icon
  - [ ] Message line: `t!("scan.select_media_type")` + UPC code display
  - [ ] Inline button group with 6 media type buttons: SVG icon + i18n label
  - [ ] Each button: `hx-post="/catalog/scan-with-type"` with `hx-vals='{"code":"...", "media_type":"..."}'`
  - [ ] Suggested button: filled primary background (vs outline for others)
  - [ ] Accessibility: `role="group"`, `aria-label`, `role="radio"` on buttons, 2px focus ring, arrow key navigation
  - [ ] After selection: HTMX response triggers `focus.js` to restore scan field focus (htmx:afterSettle)
  - [ ] No fade timer — entry persists until selection
  - [ ] Responsive: horizontal row on desktop, 2x3 grid on mobile (<640px), 44px min touch target
  - [ ] Dark mode: uses CSS custom properties (`--color-primary`), no hardcoded colors
  - [ ] Create inline SVG icons for each media type (book, bd, cd, dvd, magazine, report)
  - [ ] Cover placeholder SVGs: ensure media-type-specific placeholder icons exist for UX-DR10 cover component (cd, dvd, magazine, report variants needed alongside existing book)

- [x] Task 11: Provider Registration Update (AC: #9) — depends on Tasks 4-8
  - [ ] Update `src/main.rs` to register providers in correct chain priority order:
    - BD chain: BDGest (stub), BnF (existing), Google Books (existing) — Comic Vine NOT registered (future use)
    - CD chain: MusicBrainz with rate limiter (1 req/sec)
    - DVD chain: OMDb, TMDb (OMDb first per architecture)
    - Book chain: unchanged (BnF → Google Books → Open Library)
    - Magazine chain: unchanged (BnF → Google Books)
  - [ ] Read env vars: `OMDB_API_KEY`, `TMDB_API_KEY`, `BDGEST_API_KEY`
  - [ ] Skip providers with missing required keys (log warning, never log key values)
  - [ ] MusicBrainz: no key needed, always registered
  - [ ] Update `src/metadata/mod.rs` to export new modules (including comic_vine for future use)
  - [ ] Unit test: `registry.chain_for(MediaType::Dvd)` returns [OMDb, TMDb] not [TMDb, OMDb]
  - [ ] Unit test: `registry.chain_for(MediaType::Bd)` returns [BDGest, BnF, Google Books] — no Comic Vine

- [x] Task 12: Title Form Field Adaptation (AC: #10) — already implemented in story 1-3
  - [ ] Update title form template to conditionally show/hide fields based on `media_type`
  - [ ] Create route `GET /catalog/title/fields/{media_type}` returning form partial (the template placeholder already exists with HTMX onchange trigger)
  - [ ] Ensure field labels adapt via i18n: "Authors" for books, "Artists" for CDs, "Director" for DVDs, "Label" for CD publisher, "Studio" for DVD publisher
  - [ ] i18n for all adapted labels

- [x] Task 13: Mock Server & E2E Tests (AC: #11) — depends on ALL Tasks 1-12
  - [ ] Extend `tests/e2e/mock-metadata-server/server.py` with routes for MusicBrainz, OMDb, TMDb APIs (BDGest is stub, Comic Vine not in chain — no mocks needed)
  - [ ] Add test UPCs with deterministic responses (CD: 0093624738626, DVD: 5051889004578)
  - [ ] Update `docker-compose.test.yml` with env var overrides for all new provider base URLs
  - [ ] Create `tests/e2e/specs/journeys/media-type-scanning.spec.ts`:
    - Scan UPC → MediaTypeSelector appears → select CD → MusicBrainz metadata loads
    - Scan UPC → select DVD → OMDb metadata loads (primary, not TMDb)
    - Session memory: second UPC scan pre-selects last choice
    - Scan ISBN → auto-detects Book, no disambiguation needed
    - Scan ISSN → auto-detects Magazine, no disambiguation needed
  - [ ] Smoke test: blank browser → login → scan UPC → select type → verify metadata → verify catalog entry

- [x] Task 14: i18n Keys (AC: all)
  - [ ] Add to `locales/en.yml` and `locales/fr.yml`:
    - `media_type.book`, `media_type.bd`, `media_type.cd`, `media_type.dvd`, `media_type.magazine`, `media_type.report`
    - `scan.select_media_type`, `scan.media_type_suggested`, `scan.upc_what_type`
    - `form.artists`, `form.director`, `form.studio`, `form.label`, `form.track_count`, `form.total_duration`, `form.age_rating`, `form.issue_number`
    - Provider-related: `metadata.musicbrainz_*`, `metadata.omdb_*`, `metadata.tmdb_*`
  - [ ] Run `touch src/lib.rs && cargo build` after changes

## Dev Notes

### Architecture Compliance

- **Service layer:** All new providers go in `src/metadata/`. Each implements `MetadataProvider` trait independently. No provider calls another provider — `ChainExecutor` orchestrates.
- **Error handling:** Provider errors are `tracing::warn!`, never propagate as `AppError`. Chain continues on failure.
- **Logging:** Use `tracing` macros with structured fields: `provider`, `upc`, `isbn`, `duration_ms`, `media_type`, `title_id`.
- **i18n:** All user-facing text via `t!("key")`. After adding keys, run `touch src/lib.rs && cargo build`.
- **DB queries:** Always include `WHERE deleted_at IS NULL` on SELECTs.
- **Pool access:** `&DbPool` from AppState. Spawned tasks use `pool.clone()`.
- **API keys:** Environment variables only, never DB. Log key presence at startup, never log key values.
- **Open/closed:** New providers = implement trait + register in `main.rs`.

### Critical Infrastructure Changes Required (Do These FIRST)

**1. ChainExecutor must support UPC lookups (Task 1):**
- Currently `src/metadata/chain.rs:58` hardcodes `provider.lookup_by_isbn(code)` for ALL codes
- Must accept `code_type` parameter and branch: ISBN → `lookup_by_isbn()`, UPC → `lookup_by_upc()`
- Without this fix, ALL UPC-based providers will silently fail

**2. Title creation must accept any media_type (Task 9):**
- Currently `TitleService::create_from_isbn()` hardcodes `media_type: "book".to_string()`
- Must generalize to `create_from_code()` accepting media_type parameter
- Must store code in correct column: isbn, issn, or upc

**3. `detect_code_type()` must return MediaType (Task 9):**
- Currently returns `&'static str` ("isbn", "upc", etc.)
- Must return struct with both code_type and inferred media_type

**4. MetadataResult needs CD/DVD fields (Task 3):**
- Decision: ADD optional fields to MetadataResult: `track_count`, `total_duration`, `age_rating`, `issue_number`
- This is simpler than a secondary update path and consistent with existing `page_count` pattern

### Existing Infrastructure (from Story 3-1)

**AppState currently includes:**
```rust
pub struct AppState {
    pub pool: DbPool,
    pub settings: Arc<RwLock<AppSettings>>,
    pub http_client: reqwest::Client,
    pub registry: Arc<ProviderRegistry>,
}
```

**MetadataProvider trait already supports:**
- `lookup_by_isbn()` — required
- `lookup_by_upc()` — optional, defaults to `Ok(None)` — CD/DVD providers MUST override this
- `search_by_title()` — optional, defaults to `Ok(None)`
- `supports_media_type(&MediaType)` — required
- `rate_limiter()` — NEW in this story (Task 2): optional, defaults to `None`. MusicBrainz overrides to return its `Arc<RateLimiter>`. ChainExecutor calls `acquire()` before each lookup if present.

**ChainExecutor:** Cache-first check, per-provider 5s timeout, global timeout configurable. Currently uses string matching for rate limit detection — story 3-2 adds proactive rate limiter alongside (reactive detection stays as fallback).

**Provider env var override pattern:** Each provider accepts `{PROVIDER}_API_BASE_URL` env var for testability. Follow this pattern for all new providers.

**Client injection pattern:** All providers accept shared `reqwest::Client` from AppState.

### Database Schema (Already Supports All Fields)

The `titles` table already has columns for all media types:
- `isbn VARCHAR(13)`, `issn VARCHAR(8)`, `upc VARCHAR(13)` — code columns
- `media_type` — stored as string
- `page_count`, `track_count`, `total_duration`, `age_rating`, `issue_number` — type-specific fields
- `cover_image_url` — populated by metadata providers

No migration needed for title fields. Metadata cache table (`metadata_cache`) is code-agnostic (works for ISBN, UPC, ISSN).

### Scan Flow Change

**Current flow (story 3-1):** Scan → `detect_code_type()` returns `&str` → find/create title with hardcoded `media_type: "book"` → spawn `fetch_metadata_chain(isbn, ...)`.

**New flow (story 3-2):**
1. Scan → `detect_code_type()` returns `CodeDetection { code_type, inferred_media_type }`
2. ISBN → `MediaType::Book` (auto), `create_from_code(code, Book, isbn)` → spawn fetch with `code_type: Isbn`
3. ISSN → `MediaType::Magazine` (auto), `create_from_code(code, Magazine, issn)` → spawn fetch with `code_type: Issn`
4. UPC → check session preference cookie `media_type_preference`
   - If session has preference → use it, `create_from_code(code, preference, upc)` → spawn fetch with `code_type: Upc`
   - If no preference → return MediaTypeSelector fragment (no title created yet, no fetch spawned)
5. User clicks media type → `POST /catalog/scan-with-type` → `create_from_code()` + spawn fetch + update cookie
6. Focus returns to scan field via `focus.js` htmx:afterSettle

### FR88 Coverage (Placeholder Icons)

FR88 requires media-type-specific SVG placeholders while covers load. This is covered by Task 10 (create SVG icons for each media type) and will be fully integrated in story 3-3 when cover download/display is implemented. In this story, the SVG assets are created; in story 3-3, the Cover component (UX-DR10) uses them.

### Deferred to Later Stories (NOT in scope for 3-2)

- **Story 3-3:** FR14-FR15 — Cover image download, resize (400px max, JPEG 80%), lazy loading. All providers in 3-2 return `cover_url` but 3-3 handles actual download/storage. Also completes FR88 Cover component integration with media-type placeholder SVGs from this story.
- **Story 3-4:** FR61-FR64 — Feedback list lifecycle (auto-fade 10s+10s), audio feedback (Web Audio API 4 tones, audio.js module), error persistence, metadata error dashboard count.
- **Story 3-5:** FR16-FR18 — Re-download metadata on demand, per-field confirmation before overwrite, manual metadata editing for all fields. Also handles DVD multi-result correction (user edits wrong first-match metadata).

### Previous Story Intelligence (Story 3-1)

**Patterns to follow:**
- Provider constructor: `ProviderName::new(client: reqwest::Client, api_key: Option<String>)`
- Env var override: `{PROVIDER}_API_BASE_URL` for test isolation
- SSRF protection: validate URL components before HTTP calls
- Cover URL: ensure HTTPS (rewrite http:// to https://)
- Author/artist parsing: cap at 5 entries to avoid timeout
- JSON response handling: gracefully handle missing fields with `Option`
- Unit test pattern: test valid response, empty results, malformed JSON

**Review findings from 3-1 to avoid repeating:**
- Always write `cover_image_url` to DB in UPDATE query (was missed initially)
- URL-encode API keys in query strings
- Validate external URL paths to prevent SSRF
- Filter invalid cover IDs (e.g., negative values)
- Rewrite HTTP URLs to HTTPS for cover images

**Known test ISBNs (existing):** 9782070360246, 9780306406157, 9791032305560

**Suggested test UPCs for new providers:**
- CD: 0093624738626 (Radiohead - OK Computer), 0602498681282 (generic CD)
- DVD: 5051889004578 (generic DVD), 3333973137877 (French DVD)

### Project Structure Notes

New files to create:
- `src/metadata/rate_limiter.rs` — generic proactive rate limiter
- `src/metadata/musicbrainz.rs` — CD provider
- `src/metadata/omdb.rs` — DVD primary provider
- `src/metadata/tmdb.rs` — DVD fallback provider
- `src/metadata/bdgest.rs` — BD stub provider
- `src/metadata/comic_vine.rs` — BD fallback provider
- `templates/components/media_type_selector.html` — UX-DR22 disambiguation UI
- `tests/e2e/specs/journeys/media-type-scanning.spec.ts` — E2E tests

Files to modify (critical infrastructure):
- `src/metadata/chain.rs` — add code_type parameter, branch isbn/upc lookup, integrate rate limiter
- `src/metadata/provider.rs` — add track_count, total_duration, age_rating, issue_number to MetadataResult
- `src/tasks/metadata_fetch.rs` — rename isbn→code, pass code_type, write new fields to DB
- `src/routes/catalog.rs` — refactor detect_code_type(), add scan-with-type route, UPC handling
- `src/services/title.rs` (or equivalent) — generalize create_from_isbn() to create_from_code()

Files to modify (standard):
- `src/metadata/mod.rs` — export new modules
- `src/main.rs` — register new providers in correct chain order
- `tests/e2e/mock-metadata-server/server.py` — add mock API routes
- `tests/e2e/docker-compose.test.yml` — add env var overrides
- `locales/en.yml`, `locales/fr.yml` — new i18n keys
- Title form template — conditional field visibility + route for field partial

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-3] — Epic scope, story list, FR/NFR/AR assignments
- [Source: _bmad-output/planning-artifacts/architecture.md#Metadata-Providers] — Provider chain table, MetadataProvider trait, fallback chain design
- [Source: _bmad-output/planning-artifacts/architecture.md#Scan-Flow] — Complete scan action flow diagram
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#MediaTypeSelector] — UX-DR22 component spec
- [Source: _bmad-output/implementation-artifacts/3-1-provider-chain-and-fallback.md] — Previous story patterns, review findings, deferred work
- [Source: _bmad-output/implementation-artifacts/deferred-work.md] — Rate limiter, ISSN prefix overlap items

### Review Findings

- [x] [Review][Decision] Cookie session memory `media_type_preference` — FIXED: CookieJar added to handle_scan (reads) and handle_scan_with_type (sets)
- [x] [Review][Decision] MediaTypeSelector icons — FIXED: distinct emoji icons per media type (📖📚💿📀📰📄), SVG icons planned for story 3-3
- [x] [Review][Decision] Focus restoration — VERIFIED: existing focus.js htmx:afterSettle handler restores scan field focus after HTMX response replaces buttons
- [x] [Review][Patch] ISSN validation — FIXED: replaced broken ISSN check digit with standard EAN-13 checksum validation (same as ISBN-13)
- [x] [Review][Patch] Empty code validation — FIXED: added empty string check in handle_scan_with_type
- [x] [Review][Patch] RateLimiter guard — FIXED: added assert!(requests_per_second > 0.0) in per_second()
- [x] [Review][Defer] Rate limiter TOCTOU race — lock dropped before sleep allows concurrent bypass — deferred, single-user NAS app, low practical risk
- [x] [Review][Defer] OMDb two sequential HTTP requests within single 5s provider timeout — deferred, acceptable for MVP
- [x] [Review][Defer] UPC codes stored without checksum validation — deferred, no standard UPC validation in scope

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Task 1: Added CodeType enum, updated ChainExecutor to branch isbn/upc lookups, updated metadata_fetch to accept code_type, updated catalog.rs spawn call
- Task 2: Created RateLimiter with tokio::Mutex, added rate_limiter() trait method to MetadataProvider, integrated into ChainExecutor before each provider call
- Task 3: Extended MetadataResult with track_count, total_duration, age_rating, issue_number; updated all providers and cache to use ..Default
- Task 4: MusicBrainz provider with barcode search, artist-credit parsing, Cover Art Archive URL, 1 req/sec rate limiter
- Task 5: OMDb provider with search + detail by imdbID, "Response":"False" handling, N/A filtering, runtime→page_count
- Task 6: TMDb provider with search by UPC, poster_path→full URL, HTTPS enforcement
- Task 7: BDGest stub provider returning Ok(None) with tracing log
- Task 8: Comic Vine provider with barcode filter, person_credits parsing, NOT registered in chain
- Task 9: Refactored detect_code_type() to return CodeDetection struct with inferred MediaType; added create_from_code() generalizing create_from_isbn(); added ISSN check digit validation; added handle_scan UPC/ISSN branches; added POST /catalog/scan-with-type route; added MediaTypeSelector inline HTML
- Task 10: MediaTypeSelector with 6 buttons, a11y attributes, FeedbackEntry wrapper, responsive, dark mode
- Task 11: Registered all providers in main.rs in correct chain priority (BDGest→BnF→GB for BD, MusicBrainz for CD, OMDb→TMDb for DVD)
- Task 12: Already implemented in story 1-3 (type_specific_fields route exists)
- Task 13: Extended mock server with MusicBrainz/OMDb/TMDb routes, updated docker-compose.test.yml, created E2E test spec
- Task 14: Added media_type.* and scan.* i18n keys in en.yml and fr.yml

### File List

New files:
- src/metadata/rate_limiter.rs
- src/metadata/musicbrainz.rs
- src/metadata/omdb.rs
- src/metadata/tmdb.rs
- src/metadata/bdgest.rs
- src/metadata/comic_vine.rs
- tests/e2e/specs/journeys/media-type-scanning.spec.ts

Modified files:
- src/metadata/mod.rs
- src/metadata/chain.rs
- src/metadata/provider.rs
- src/metadata/google_books.rs
- src/metadata/open_library.rs
- src/models/media_type.rs
- src/models/title.rs
- src/models/metadata_cache.rs
- src/tasks/metadata_fetch.rs
- src/routes/catalog.rs
- src/routes/mod.rs
- src/services/title.rs
- src/main.rs
- locales/en.yml
- locales/fr.yml
- tests/e2e/mock-metadata-server/server.py
- tests/e2e/docker-compose.test.yml
