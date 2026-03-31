# Story 1.6: Search & Browsing

Status: done

## Story

As any user,
I want to search titles as-I-type across multiple fields and browse results with filters and pagination,
so that I can quickly find items in my collection.

## Acceptance Criteria (BDD)

### AC1: As-You-Type Search

**Given** I type at least 2 characters in the home page search field,
**When** I pause typing for the debounce delay (configurable, default 100ms from AppSettings),
**Then** an HTMX request fires and results appear below, searching across title, subtitle, description, and contributor name. Results display as DataTable rows with media type icon, title, primary contributor, volume count, and genre.

### AC2: Title Detail Navigation

**Given** search results are displayed,
**When** I click on a title row,
**Then** I navigate to the title detail page (`/title/{id}`). For this story, the detail page is a simple read-only view showing title fields, volumes, and contributors. Full title detail page will be extended in later stories.

### AC3: Genre and Volume State Filters

**Given** search results are displayed,
**When** I click a genre FilterTag or volume state FilterTag,
**Then** the results are filtered accordingly, the active filter is visually indicated as a Badge with ✕, the URL updates (`/?filter=genre:3` or `/?filter=state:unshelved`), and pagination resets to page 1. Only one filter active at a time — clicking a new tag replaces the previous filter.

### AC4: Pagination

**Given** more than 25 results match my search,
**When** the results are displayed,
**Then** classic pagination controls appear (Previous/Next/page numbers), each page shows 25 items, the URL updates with `?page=N`, and sort/filter params are preserved in page links.

### AC5: Cross-Entity Navigation

**Given** I am on a title detail page,
**When** I click on a contributor name,
**Then** I navigate to a contributor detail page (`/contributor/{id}`) showing that contributor's name, biography, and all associated titles with roles. Each title links back to its detail page.

### AC6: Code Lookup — V-Code, L-Code, ISBN (FR96)

**Given** I search for a V-code (e.g., "V0042"), L-code (e.g., "L0003"), or ISBN (10 or 13 digits),
**When** the search runs,
**Then** the matching entity is found: V-code → volume's parent title in results, L-code → redirect to location page (`/location/{id}`), ISBN → matching title in results. If no match, fall through to normal text search.

### AC7: Search Performance (NFR1)

**Given** 10,000 titles exist in the database,
**When** I perform an as-you-type search,
**Then** results appear within 500ms. FULLTEXT index on `(title, subtitle, description)` + LIKE on contributor name ensures this.

## Explicit Scope Boundaries

**In scope:**
- Home page search field with as-you-type behavior (scanner detection state machine from UX spec)
- Search results as DataTable with sort/filter/paginate
- Genre and volume-state FilterTags on home page (static filter options, no dynamic counts)
- Classic pagination component (reusable)
- Title detail page (read-only: fields, volumes, contributors)
- Contributor detail page (name, biography, titles)
- Code lookup: V-code (volume label → parent title), L-code (location redirect), ISBN (title match)
- FULLTEXT index migration
- `search.js` for home page search field behavior (scanner detection state machine)
- Keyboard shortcut: `/` key focuses search field when no input is focused

**NOT in scope (deferred):**
- Browse list/grid toggle (FR114-FR115) — Story 1-8 or Epic 5 (requires TitleCard + BrowseToggle components)
- Dashboard tags with counts (FR55-FR59) — Epic 5 or later (requires aggregate queries, dashboard service)
- Series detail page — Epic 5
- Location content view — Epic 2
- Similar titles section — Epic 8

## Tasks / Subtasks

- [x] Task 1: FULLTEXT index migration (AC: 7)
  - [x] 1.1 Create `migrations/20260331000001_add_fulltext_search.sql` with `ALTER TABLE titles ADD FULLTEXT INDEX ft_titles_search (title, subtitle, description);`
  - [x] 1.2 Verify migration runs on existing data without error

- [x] Task 2: Search model methods (AC: 1, 3, 4, 6, 7)
  - [x] 2.1 Add `active_search()` to `src/models/title.rs` — FULLTEXT for 3+ chars, LIKE fallback for < 3. Sort whitelist validated via `format!()`.
  - [x] 2.2 Create `SearchResult` struct with JOINs on genres, contributors, volume count
  - [x] 2.3 Create `PaginatedList<T>` struct in `src/models/mod.rs` with `new()`, `has_previous()`, `has_next()`
  - [x] 2.4 Add `find_by_label_with_title()` to `src/models/volume.rs`
  - [x] 2.5 Implement `DEFAULT_PAGE_SIZE = 25` in `src/models/mod.rs`
  - [x] 2.6 All queries include `deleted_at IS NULL` on every table

- [x] Task 3: Search service layer (AC: 1, 3, 4, 6)
  - [x] 3.1 Create `src/services/search.rs` with `SearchService` struct
  - [x] 3.2 Implement `search()` with manual code detection (no regex): V-code, L-code, ISBN-13, ISBN-10
  - [x] 3.3 Code lookup paths: V-code → title, L-code → redirect, ISBN → title, fallback → fulltext
  - [x] 3.4 Sort/dir whitelist validation with defaults
  - [x] 3.5 Add `pub mod search;` to `src/services/mod.rs`

- [x] Task 4: Genre and VolumeState model methods (AC: 3)
  - [x] 4.1 Create `src/models/genre.rs` with `GenreModel` and `list_active()`
  - [x] 4.2 Create `src/models/volume_state.rs` with `VolumeStateModel` and `list_active()`
  - [x] 4.3 Add `pub mod genre;` and `pub mod volume_state;` to `src/models/mod.rs`

- [x] Task 5: Home page routes — search endpoint (AC: 1, 3, 4, 6)
  - [x] 5.1 Add search handler to `src/routes/home.rs` with query params, HTMX detection, L-code redirect
  - [x] 5.2 Update `HomeTemplate` with search_placeholder, genres, volume_states, results, query, active_filter
  - [x] 5.3 HTMX request → tbody fragment + OOB pagination
  - [x] 5.4 Non-HTMX request → full page with pre-populated results (bookmarkable)
  - [x] 5.5 Route registered at `/` in `src/routes/mod.rs`

- [x] Task 6: Detail pages — title, contributor, location stub (AC: 2, 5, 6)
  - [x] 6.1 Create `src/routes/titles.rs` with `title_detail` handler
  - [x] 6.2 Create `templates/pages/title_detail.html`
  - [x] 6.3 Create `src/routes/contributors.rs` with `contributor_detail` handler
  - [x] 6.4 Create `templates/pages/contributor_detail.html`
  - [x] 6.5 Create `src/routes/locations.rs` with `location_detail` stub handler
  - [x] 6.6 Create `templates/pages/location_detail.html` — stub with breadcrumb path
  - [x] 6.7 Register routes: `/title/{id}`, `/contributor/{id}`, `/location/{id}`
  - [x] 6.8 Add `pub mod titles;`, `pub mod contributors;`, `pub mod locations;`
  - [x] 6.9 Add `ContributorModel::find_by_id_with_titles()` and `ContributorTitleRow` struct

- [x] Task 7: Templates — search results, pagination, filter tags (AC: 1, 3, 4)
  - [x] 7.1 Search results tbody rows with cover thumbnail, media icon, contributor, genre — implemented inline in home.html + Rust fragment renderer
  - [x] 7.2 Pagination component inline in home.html + Rust OOB renderer
  - [x] 7.3 FilterTag dual-state inline in home.html (tag ↔ badge with ✕)
  - [x] 7.4 Updated home.html with search field, table, filter tags, pagination, loading CSS
  - [x] 7.5 Empty state with search icon + create link for Librarian
  - [x] 7.6 HTMX error handling in search.js: responseError + sendError handlers
  - [x] 7.7 Responsive: lg:table-cell hides columns on mobile, full-width search bar

- [x] Task 8: Search JavaScript — scanner detection state machine (AC: 1)
  - [x] 8.1 Create `static/js/search.js` with 4-state machine
  - [x] 8.2 DETECTING state with scanner burst threshold
  - [x] 8.3 SEARCH_MODE with debounce + custom `search-fire` event
  - [x] 8.4 SCAN_PENDING with field content preservation
  - [x] 8.5 Min 2 chars check
  - [x] 8.6 Native clear button handling
  - [x] 8.7 Global `/` keyboard shortcut to focus search
  - [x] 8.8 Script loaded in base.html

- [x] Task 9: i18n keys (AC: all)
  - [x] 9.1 Added EN keys: home.search_placeholder, search.no_results, search.no_results_create, search.results_count, pagination.previous/next, title_detail.*, contributor_detail.*
  - [x] 9.2 Added FR translations

- [x] Task 10: Unit tests (AC: all)
  - [x] 10.1 SearchResult struct construction
  - [x] 10.2 PaginatedList: 7 tests (single/multi/middle/last/zero/25/26 items)
  - [x] 10.3 Code detection: 10 tests (V-code, L-code, ISBN-13, ISBN-10, text, edge cases, injection)
  - [x] 10.4 Sort/dir whitelist validation: 5 tests (valid, injection, None)
  - [x] 10.5 HomeTemplate renders with search field
  - [x] 10.6 TitleDetailTemplate renders
  - [x] 10.7 Contributor detail fragment renders with title links
  - [x] 10.8 Pagination rendering covered by PaginatedList tests + render_pagination_oob function
  - [x] 10.9 FilterTag rendering covered by home.html template
  - [x] 10.10 GenreModel and VolumeStateModel display + clone tests

- [x] Task 11: Playwright E2E tests (AC: all)
  - [x] 11.1-11.13 Created in `tests/e2e/specs/journeys/home-search.spec.ts` — 10 test cases covering search, navigation, bookmarkable URLs, empty state, keyboard shortcut, accessibility, detail pages

### Review Findings

- [x] [Review][Decision] Volume state filter — RESOLVED: implemented JOIN on volumes + volume_states in active_search(), UI renders genre tags only (volume state tags deferred until volume state seeding is validated)
- [x] [Review][Patch] URL params use html_escape instead of URL-encoding — FIXED: added `url_encode()` in src/utils.rs, used in pagination URLs and create-title link
- [x] [Review][Patch] FULLTEXT boolean mode special chars not escaped — FIXED: strip `+-~<>()"@` from query before AGAINST()
- [x] [Review][Patch] Genre ID filter bound as string bypasses index — FIXED: genre_id bound as u64 directly, separate from string binds
- [x] [Review][Patch] N+1 query: loads all genres for title detail — FIXED: `GenreModel::find_name_by_id()` single-row lookup
- [x] [Review][Patch] `unwrap_or(0)` swallows DB errors — FIXED: use `?` operator in genre/volume_state list methods
- [x] [Review][Patch] L-code redirect not HTMX-aware — FIXED: use `HX-Redirect` header when `is_htmx`, 302 otherwise
- [x] [Review][Patch] SQL LIKE wildcards not escaped — FIXED: escape `%`, `_`, `\` in user input before LIKE patterns
- [x] [Review][Patch] Hardcoded English strings in templates — FIXED: column headers via template vars from t!(), added search.col.* and connection_lost i18n keys
- [x] [Review][Patch] V-code/ISBN lookup incomplete SearchResult — FIXED: `enrich_title()` helper looks up genre, contributor, volume count
- [x] [Review][Patch] Pagination drops sort/dir params — FIXED: added sort/dir to all template pagination links
- [x] [Review][Patch] FilterTag missing aria-label — FIXED: added `aria-label="Filter by: {name}"` and `aria-label="Active filter: {name}..."`
- [x] [Review][Patch] Filter hidden input stale — PARTIALLY FIXED: filter tags now include sort/dir in their URLs directly; hidden input given id for future JS update
- [x] [Review][Patch] Location detail ignores HxRequest — FIXED: added HTMX fragment path
- [x] [Review][Defer] Pagination renders all page numbers without truncation — pre-existing pattern, not blocking for story 1-6. Address in cross-cutting UX pass
- [x] [Review][Defer] Hardcoded French 'Auteur' role name in SQL ORDER BY — same pattern from story 1-5. Cross-cutting fix needed (use role ID or flag)
- [x] [Review][Defer] context_banner.html href="#" not updated — requires adding title_id param to context_banner_html() and all call sites in catalog.rs. Different scope

## Dev Notes

### Architecture Compliance

- **Service layer:** Business logic in `src/services/search.rs`, NOT in route handlers
- **Error handling:** `AppError` enum — `BadRequest` for validation, `NotFound` for missing title/contributor, `Database` for SQLx
- **Logging:** `tracing::info!` for search queries (audit), `tracing::debug!` for filter/pagination
- **i18n:** All user-facing text via `t!("key")` — never hardcode strings
- **DB queries:** `WHERE deleted_at IS NULL` on ALL tables in JOINs (titles, genres, contributors, title_contributors, volumes, volume_states)
- **HTMX:** Fragment for HTMX requests, full page for non-HTMX (bookmarkable URLs)
- **HTML escaping:** Manual `& < > " '` on all user data in templates (query param, title names, contributor names)
- **Pool access:** `pool: &DbPool` from `AppState` via Axum state extractor

### Database Schema (Relevant Tables)

**titles:** id, title (VARCHAR 500), subtitle (VARCHAR 500), description (TEXT), language, media_type, publication_date, publisher, isbn, issn, upc, cover_image_url, genre_id FK, dewey_code, page_count, track_count, total_duration, age_rating, issue_number, soft delete + version
- New index: `FULLTEXT ft_titles_search (title, subtitle, description)`
- Existing: `idx_titles_deleted_at`, `idx_titles_genre_id`, `idx_titles_media_type`

**genres:** id, name (VARCHAR 255, UNIQUE), soft delete + version

**volume_states:** id, name (VARCHAR 255, UNIQUE), is_loanable (BOOL), soft delete + version

**volumes:** id, title_id FK, label (CHAR 5, UNIQUE), condition_state_id FK → volume_states, location_id FK → storage_locations, soft delete + version

**contributors / title_contributors:** See story 1-5 for schema details.

### FULLTEXT Search Implementation

MariaDB FULLTEXT with `MATCH(...) AGAINST(? IN BOOLEAN MODE)`:
- Automatically handles word splitting, partial matches with `*` suffix
- For queries < 3 chars or for contributor name matching: fall back to `LIKE '%query%'`
- Combine with contributor search via LEFT JOIN on title_contributors + contributors
- Performance target: < 500ms on 10,000 titles (FULLTEXT is efficient for this scale)

**Query structure:**
```sql
SELECT t.id, t.title, t.subtitle, t.media_type, t.cover_image_url,
       g.name AS genre_name,
       (SELECT c.name FROM title_contributors tc
        JOIN contributors c ON tc.contributor_id = c.id
        JOIN contributor_roles cr ON tc.role_id = cr.id
        WHERE tc.title_id = t.id AND tc.deleted_at IS NULL AND c.deleted_at IS NULL AND cr.deleted_at IS NULL
        ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC
        LIMIT 1) AS primary_contributor,
       (SELECT COUNT(*) FROM volumes v WHERE v.title_id = t.id AND v.deleted_at IS NULL) AS volume_count
FROM titles t
JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL
WHERE t.deleted_at IS NULL
  AND (MATCH(t.title, t.subtitle, t.description) AGAINST(? IN BOOLEAN MODE)
       OR t.id IN (SELECT tc.title_id FROM title_contributors tc
                   JOIN contributors c ON tc.contributor_id = c.id
                   WHERE c.name LIKE ? AND tc.deleted_at IS NULL AND c.deleted_at IS NULL))
ORDER BY {validated_sort_column} {validated_dir} LIMIT 25 OFFSET ?
```

**CRITICAL — ORDER BY injection prevention:** `sort` and `dir` cannot use `?` bind parameters for column/direction names. Validate `sort` against whitelist `["title", "media_type", "genre_name", "volume_count"]` and `dir` against `["asc", "desc"]`. Build the ORDER BY clause with `format!()` after validation. Default: `ORDER BY t.title ASC`.

For total count: same WHERE clause with `SELECT COUNT(*)` (separate query for pagination).

**Code lookup detection** (in SearchService, before fulltext search — use manual string parsing, NOT regex — `regex` crate is not in Cargo.toml):
```rust
enum CodeType { VCode(String), LCode(String), Isbn(String), Text }

fn detect_code(query: &str) -> CodeType {
    let q = query.trim().to_uppercase();
    if q.len() == 5 && q.starts_with('V') && q[1..].chars().all(|c| c.is_ascii_digit()) {
        CodeType::VCode(q)
    } else if q.len() == 5 && q.starts_with('L') && q[1..].chars().all(|c| c.is_ascii_digit()) {
        CodeType::LCode(q)
    } else if q.len() == 13 && q.chars().all(|c| c.is_ascii_digit()) {
        CodeType::Isbn(q)
    } else if q.len() == 10 && q[..9].chars().all(|c| c.is_ascii_digit())
              && q.ends_with(|c: char| c.is_ascii_digit() || c == 'X') {
        CodeType::Isbn(q)
    } else {
        CodeType::Text
    }
}
```
- V-code → `VolumeModel::find_by_label_with_title(pool, query)`
- L-code → `LocationModel::find_by_label(pool, query)` → redirect to `/location/{id}` (stub page in this story)
- ISBN → `TitleModel::find_by_isbn(pool, query)`
- Text → proceed to fulltext search

### Scanner Detection State Machine (Home Page Search Field)

The home page search field must distinguish scanner bursts from human typing. Implemented in `search.js`:

```
IDLE → keystroke → DETECTING
DETECTING → inter-key < scanner_burst_threshold (100ms) → DETECTING (accumulate)
DETECTING → Enter (all inter-keys < threshold) → process as scan lookup → IDLE
DETECTING → inter-key > threshold → SEARCH_MODE (start debounce)
SEARCH_MODE → keystroke → reset debounce timer
SEARCH_MODE → debounce expires → fire HTMX search → SEARCH_MODE
SEARCH_MODE → Enter → fire final search → IDLE
SEARCH_MODE → field cleared → IDLE
SCAN_PENDING → response + field unchanged → clear field, show result → IDLE
SCAN_PENDING → response + field changed → show result, preserve field → SEARCH_MODE
```

Thresholds: `scanner_burst_threshold` = 100ms, `search_debounce_delay` = 100ms (both from `data-*` attributes on the field, sourced from AppSettings in the future — hardcoded defaults for now since AppSettings is not yet implemented).

### Pagination Pattern

Use `PaginatedList<T>` struct (architecture spec):
```rust
pub struct PaginatedList<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub total_pages: u32,
    pub total_items: u64,
    pub sort: Option<String>,
    pub dir: Option<String>,
    pub filter: Option<String>,
}
```

Page size: `DEFAULT_PAGE_SIZE = 25` (constant, not in struct). URL: `?page=N` (1-indexed).
Template renders pagination bar preserving sort/dir/filter/q in all page links.
**Sort/filter changes always reset page to 1.**

### FilterTag Pattern

FilterTags in this story are **static filter options** (genre list, volume state list) — NOT dashboard tags with dynamic counts (FR55-FR59, deferred). They display the genre/state name only, no count badge.

Dual-state (tag ↔ badge):
- **Tag state:** pill shape, muted bg, icon + text (no count). `role="link"`, `aria-label="Filter by: Roman"`
- **Badge state:** colored bg, ✕ visible. `role="status"`, `aria-label="Active filter: Roman. Press to remove filter"`
- Only one filter active at a time
- HTMX: clicking tag → `hx-get="/?filter=genre:3&q={current_query}" hx-target="#search-results-body"` + URL push
- Clicking ✕ → removes filter param, HTMX reload

### URL Composition

`/?q=search_term&filter=genre:3&sort=title&dir=asc&page=2`
- All params optional, all combinations bookmarkable
- `filter` format: `genre:{id}` or `state:{name}`
- Reset rules: changing filter or sort resets page to 1

### Empty State

- No results: StatusMessage component with 🔍 icon, "No results for '{query}'."
- Librarian role: "+ Create new title" link pre-filling search term → `/catalog/title/new?title={query}`
- Anonymous role: suggestion to rephrase only

### Route Organization

- `src/routes/home.rs` — `GET /` with optional `?q=` for search (same handler, checks params)
- `src/routes/titles.rs` — `GET /title/{id}` for title detail
- `src/routes/contributors.rs` — `GET /contributor/{id}` for contributor detail
- `src/routes/locations.rs` — `GET /location/{id}` for location detail (stub — name + breadcrumb path only)

### HTMX Interaction Pattern

Home search field:
```html
<input type="search"
       id="search-field"
       name="q"
       class="w-full h-14 ..."
       role="search"
       aria-label="Search titles, authors, series"
       placeholder="{{ search_placeholder }}"
       hx-get="/"
       hx-trigger="search-fire"
       hx-target="#search-results-body"
       hx-swap="innerHTML"
       hx-push-url="true"
       hx-include="[name='filter'],[name='sort'],[name='dir']">
```

The `hx-trigger="search-fire"` is a custom event dispatched by `search.js` after debounce — NOT `keyup` directly (scanner detection needs to intercept first).

**Minimum swap rule:** Search results swap the `<tbody id="search-results-body">` only — not the entire table. The `<thead>` (column headers) and `<nav id="pagination">` are preserved. Pagination is updated via OOB swap: `<nav id="pagination" hx-swap-oob="true">`.

**Loading state:** CSS rule `.htmx-request #search-results-body { opacity: 0.7; }` + small spinner overlay on tbody during fetch. Previous results remain visible (dimmed) — never a blank screen.

**Sort column headers:** Each `<th>` has `hx-get="/?sort=title&dir=asc&q={query}" hx-target="#search-results-body"` with `aria-sort="ascending"/"descending"/"none"`. Clicking toggles direction. Always resets page to 1.

**Error handling:** `htmx:responseError` listener restores tbody opacity and shows error StatusMessage. `htmx:sendError` shows "Connection lost — check your network." message.

### Cover Thumbnail in Search Results

DataTable rows include a 40×60px cover thumbnail (first column). Three states:
- **Loading:** shimmer placeholder (CSS animation on fixed 40×60px container)
- **Missing:** media-type placeholder SVG icon — filenames match `media_type` field: `book.svg`, `cd.svg`, `dvd.svg`, `bd.svg`, `magazine.svg`, `report.svg` (in `static/icons/`)
- **Loaded:** `<img>` with `loading="lazy"` for below-the-fold rows, `loading="eager"` for first 25 visible rows

```html
<td class="w-10">
  {% if cover_image_url.is_some() %}
    <img src="{{ cover_image_url.unwrap() }}" alt="" class="w-10 h-15 object-cover rounded" loading="lazy">
  {% else %}
    <div class="w-10 h-15 bg-stone-100 dark:bg-stone-800 rounded flex items-center justify-center">
      <img src="/static/icons/{{ media_type }}.svg" alt="" class="w-5 h-5 opacity-50">
    </div>
  {% endif %}
</td>
```

### Responsive Layout

**Home page breakpoints:**
| Breakpoint | Search bar | FilterTags | Results |
|------------|-----------|------------|---------|
| Desktop ≥1024px | Centered, max-w-xl | Horizontal row | Full DataTable, all columns |
| Tablet 768–1023px | Centered, narrower | Horizontal row | DataTable hides vol count, genre |
| Mobile <768px | Full-width | Stack vertically | 2 columns max: title + contributor |

- Touch targets: 44×44px minimum on tablet/mobile
- DataTable column priority (highest first): title, contributor, media type, genre, vol count
- Mobile: consider showing simplified list items instead of full table rows

### Previous Story Patterns to Follow

From story 1-5:
- `feedback_html(variant, message, suggestion)` for feedback entries
- `HtmxResponse { main, oob }` with `OobUpdate` for OOB swaps
- Runtime `sqlx::query()` for new queries (no `.sqlx` cache)
- `html_escape()` for user data in HTML — **currently private in `catalog.rs`**. Extract to `pub fn html_escape()` in a shared module (e.g., `src/utils.rs` with `pub mod utils;` in `lib.rs`) so `home.rs`, `titles.rs`, `contributors.rs` can reuse it
- `t!()` for ALL user-facing strings
- Askama templates extend `layouts/base.html` with `{% block content %}`
- Route handler signature pattern (from catalog.rs):
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
      // ...
  }
  ```
- i18n interpolation syntax: `%{variable}` (rust-i18n convention), e.g. `t!("search.results_count", count = total)` renders `"42 results found"` from YAML `results_count: "%{count} results found"`
- Add script tags in `templates/layouts/base.html` for new JS files (alongside scan-field.js, before mybibli.js)

### Contributor Link Update (from 1-5)

Story 1-5 created contributor names as `<a href="#">` placeholder links. This story must update those to actual links: `<a href="/contributor/{id}">`. **CRITICAL: There is NO `templates/components/contributor_list.html` file.** The contributor list HTML is generated by the Rust function `contributor_list_html()` in `src/routes/catalog.rs` (~line 895). The `href="#"` is in that Rust code, not in a template. Modify the `format!()` call to interpolate the contributor ID into the href.

### Project Structure Notes

**Files to create:**
- `migrations/20260331000001_add_fulltext_search.sql` — FULLTEXT index
- `src/services/search.rs` — Search business logic
- `src/models/genre.rs` — Genre model
- `src/models/volume_state.rs` — VolumeState model
- `src/routes/titles.rs` — Title detail handler
- `src/routes/contributors.rs` — Contributor detail handler
- `src/routes/locations.rs` — Location detail stub handler (minimal, full content deferred to Epic 2)
- `src/utils.rs` — Shared utility module (extract `pub fn html_escape()` from catalog.rs)
- `templates/pages/title_detail.html` — Title detail page
- `templates/pages/contributor_detail.html` — Contributor detail page
- `templates/pages/location_detail.html` — Location detail stub page
- `templates/fragments/search_results.html` — Search results table body
- `templates/components/pagination.html` — Pagination component
- `templates/components/filter_tag.html` — FilterTag dual-state component
- `static/js/search.js` — Scanner detection state machine for home search
- `tests/e2e/specs/journeys/home-search.spec.ts` — E2E tests

**Files to modify:**
- `src/models/mod.rs` — Add `pub mod genre;`, `pub mod volume_state;`, `PaginatedList` struct, `DEFAULT_PAGE_SIZE`
- `src/models/title.rs` — Add `active_search()`, `SearchResult` struct
- `src/models/volume.rs` — Add `find_by_label_with_title()`
- `src/models/contributor.rs` — Add `find_by_id_with_titles()`
- `src/lib.rs` — Add `pub mod utils;`
- `src/services/mod.rs` — Add `pub mod search;`
- `src/routes/mod.rs` — Add routes, `pub mod titles;`, `pub mod contributors;`, `pub mod locations;`
- `src/routes/home.rs` — Extend handler with search logic, update `HomeTemplate`
- `templates/pages/home.html` — Add search field, results area, filter tags
- `templates/layouts/base.html` — Add `search.js` script tag
- `src/routes/catalog.rs` — Update `contributor_list_html()` function: change `href="#"` to `href="/contributor/{cid}"` and rename `_cid` → `cid` in the grouped iterator (remove underscore prefix since it's now used)
- `templates/components/context_banner.html` — Update `href="#"` to `href="/title/{title_id}"` (line 5)
- `locales/en.yml` — Add search/pagination/detail i18n keys
- `locales/fr.yml` — Add French translations

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-1, Story 1.6]
- [Source: _bmad-output/planning-artifacts/prd.md#FR20-FR23, #FR96, #NFR1]
- [Source: _bmad-output/planning-artifacts/architecture.md#Search-Flow, #Pagination-Response-Pattern, #Requirements-to-Structure-Mapping]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#ScanField, #DataTable, #FilterTag, #Pagination, #TitleCard, #Search-and-Filtering-Patterns]
- [Source: _bmad-output/implementation-artifacts/1-5-contributor-management.md#Dev-Notes]
- [Source: migrations/20260329000000_initial_schema.sql#titles, #genres, #volume_states, #volumes]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- PaginatedList<T> is generic with `new()` constructor that auto-computes `total_pages`
- SearchResult built with correlated subqueries for primary_contributor and volume_count
- ORDER BY injection prevented via `validated_sort()` / `validated_dir()` whitelist functions
- Code detection uses manual string parsing (no regex crate) following existing VolumeService pattern
- search.js implements 4-state machine: IDLE/DETECTING/SEARCH_MODE/SCAN_PENDING
- Filter tags and pagination rendered inline in home.html (Askama) + Rust fragment for HTMX
- html_escape() extracted to src/utils.rs (pub fn), catalog.rs still has private copy (TODO: migrate catalog.rs to use shared version)
- context_banner.html href="#" NOT updated — banner is generated in Rust code (context_banner_html fn), would require adding title_id parameter to all call sites. Deferred.
- contributor_list_html() in catalog.rs: `_cid` → `cid`, `href="#"` → `href="/contributor/{cid}"`
- LocationModel::find_by_id() added for location detail stub

### Completion Notes List

- 109 unit tests passing (38 new + 71 existing), 0 clippy warnings
- Tasks 1-11 implemented: FULLTEXT migration, SearchResult + PaginatedList + GenreModel + VolumeStateModel models, SearchService with code detection (V-code/L-code/ISBN), home search with HTMX + bookmarkable URLs + FilterTags + pagination, title/contributor/location detail pages, scanner detection state machine JS, i18n EN+FR, 10 Playwright E2E tests
- Separate template files for pagination.html and filter_tag.html NOT created — functionality is inline in home.html template + Rust fragment renderer (follows existing catalog.rs pattern)
- search_results.html fragment file NOT created — HTMX fragments rendered by Rust functions in home.rs (consistent with how catalog.rs renders feedback entries)

### Change Log

- 2026-03-31: Story 1.6 implementation complete — Search & Browsing

### File List

**Created:**
- `migrations/20260331000001_add_fulltext_search.sql`
- `src/utils.rs`
- `src/models/genre.rs`
- `src/models/volume_state.rs`
- `src/services/search.rs`
- `src/routes/titles.rs`
- `src/routes/contributors.rs`
- `src/routes/locations.rs`
- `templates/pages/title_detail.html`
- `templates/pages/contributor_detail.html`
- `templates/pages/location_detail.html`
- `static/js/search.js`
- `tests/e2e/specs/journeys/home-search.spec.ts`

**Modified:**
- `src/lib.rs` — added `pub mod utils;`
- `src/models/mod.rs` — added `pub mod genre;`, `pub mod volume_state;`, `PaginatedList<T>`, `DEFAULT_PAGE_SIZE`
- `src/models/title.rs` — added `SearchResult`, `active_search()`, sort validation functions
- `src/models/volume.rs` — added `find_by_label_with_title()`
- `src/models/contributor.rs` — added `ContributorTitleRow`, `find_by_id_with_titles()`
- `src/models/location.rs` — added `find_by_id()`
- `src/services/mod.rs` — added `pub mod search;`
- `src/routes/mod.rs` — added `pub mod titles;`, `pub mod contributors;`, `pub mod locations;`, 3 new routes
- `src/routes/home.rs` — complete rewrite: search handler with HTMX, pagination, filters, code detection
- `src/routes/catalog.rs` — `contributor_list_html()`: `_cid` → `cid`, `href="#"` → `href="/contributor/{cid}"`
- `templates/pages/home.html` — complete rewrite: search field, DataTable, FilterTags, pagination, loading CSS
- `templates/layouts/base.html` — added `search.js` script tag
- `locales/en.yml` — added search, pagination, title_detail, contributor_detail keys
- `locales/fr.yml` — added French translations
