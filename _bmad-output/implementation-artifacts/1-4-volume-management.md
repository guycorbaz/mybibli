# Story 1.4: Volume Management

Status: done

## Story

As a librarian,
I want to scan V-code labels to create physical volumes and attach them to the current title,
so that I can track individual copies of each title in my collection.

## Acceptance Criteria (BDD)

### AC1: Create Volume from V-code Scan with Current Title

**Given** a title is set as "current title" in the catalog session,
**When** I scan a V-code (e.g., V0042) that does not exist in the database,
**Then** a new volume is created with that label attached to the current title, a success FeedbackEntry appears ("Volume V0042 attached to *{title_name}*. Scan L-code to shelve."), and the context banner updates to show the incremented volume count.

### AC2: Reject Duplicate V-code

**Given** I scan a V-code that already exists in the database,
**When** the server processes the scan,
**Then** an error FeedbackEntry appears ("Label V0042 is already assigned to *{title_name}*. Scan a different label.") and no volume is created.

### AC3: V-code Without Current Title Context

**Given** no title is set as "current title" in the catalog session,
**When** I scan a V-code,
**Then** a warning FeedbackEntry appears ("No title selected. Scan an ISBN first to establish a title context.") and no volume is created.

### AC4: V-code Format Validation

**Given** I scan a code starting with V,
**When** client-side and server-side validation runs,
**Then** only the format uppercase V followed by exactly 4 digits (V0001-V9999) is accepted. Lowercase "v", wrong length (V123, V00001), or non-numeric (VABC) are rejected with an error FeedbackEntry. V0000 is also rejected (range is V0001-V9999). Client-side validation rejects before server submission (same pattern as ISBN checksum).

### AC5: Volume Count in Context Banner

**Given** a title has one or more volumes,
**When** the context banner is displayed,
**Then** it shows the title name, media type icon, and volume count (e.g., "Current: L'Étranger — 2 vol").

### AC6: L-code Scan Assigns Location to Volume

**Given** I scan a V-code that exists in the database (already attached to a title),
**When** I then scan an L-code that exists in the database,
**Then** the volume's location is updated to the scanned location, and a success FeedbackEntry appears ("Volume V0042 shelved at *{location_path}*."). If the L-code does not exist, a warning FeedbackEntry appears.

### AC7: L-code Without Volume Context

**Given** no volume has been recently scanned (no active volume context),
**When** I scan an L-code,
**Then** first check if L-code exists in DB: if not found, return warning "Location {label} not found." regardless of context. If found but no volume context, return info FeedbackEntry stub ("Location contents — coming in Story 1-6").

### AC8: Session Counter Increments

**Given** a volume is successfully created,
**When** the feedback is displayed,
**Then** the session counter in the catalog toolbar increments by 1 (tracked in session data, visible as "N items this session").

## Explicit Scope Boundaries

**Deferred to Story 1-6 (Search & Browsing):**
- Title detail page with full volume list and status summary (FR90 display)
- Location contents display on L-code scan
- Search by V-code (FR96)

**Deferred to Epic 2 (Storage Locations):**
- Location hierarchy CRUD (FR32-FR33)
- Location barcode generation (FR116)
- Location deletion protection (FR34)
- L-code retirement (FR117)

**Deferred to Story 1-8 (Cross-cutting Patterns):**
- Volume soft-delete with trash management (FR109)
- Optimistic locking conflict handling (FR82)

**NOT in scope:**
- Volume condition/state assignment — `condition_state_id` is NULL on creation, editable in a future story
- Volume edition comment editing — `edition_comment` is NULL on creation
- "Add volume" button on title detail page (FR7 — title detail page doesn't exist yet, deferred to Story 1-6)
- Batch shelving mode (L-code → V-code → V-code...) — Story 1-4 supports V-code → L-code (single shelving) only; the "set active location then scan volumes" workflow is deferred
- Volume status breakdown in banner (shelved/not shelved/on loan) — Story 1-4 shows count only, status breakdown deferred to Story 1-6 with FR90 full display
- Soft-deleted V-code reuse: if a V-code was soft-deleted, scanning it creates a new volume (soft-deleted volumes have `deleted_at IS NOT NULL` and are excluded by `find_by_label` query)

## Tasks / Subtasks

**EXECUTION ORDER: Task 1 (model) first, then service, then routes/templates.**

- [x] Task 1: Volume model and DB queries (AC: 1, 2, 4)
  - [x] 1.1 Create `src/models/volume.rs` with `VolumeModel` struct matching `volumes` table (id, title_id, label, condition_state_id, edition_comment, location_id, version)
  - [x] 1.2 Implement `find_by_label(pool: &DbPool, label: &str) -> Result<Option<VolumeModel>, AppError>` with `WHERE deleted_at IS NULL`
  - [x] 1.3 Implement `create(pool: &DbPool, title_id: u64, label: &str) -> Result<VolumeModel, AppError>` — INSERT with title_id and label, return created record
  - [x] 1.4 Implement `update_location(pool: &DbPool, id: u64, location_id: u64) -> Result<(), AppError>` — UPDATE location_id
  - [x] 1.5 Implement `count_by_title(pool: &DbPool, title_id: u64) -> Result<u64, AppError>` — count volumes for a title (for banner)
  - [x] 1.6 Add `tracing::info!` for volume creation, `tracing::debug!` for lookups
  - [x] 1.7 Add `pub mod volume;` to `src/models/mod.rs`

- [x] Task 2: Location model (AC: 6, 7)
  - [x] 2.1 Create `src/models/location.rs` with `LocationModel` struct (id, parent_id, name, node_type, label)
  - [x] 2.2 Implement `find_by_label(pool: &DbPool, label: &str) -> Result<Option<LocationModel>, AppError>` with `WHERE deleted_at IS NULL`
  - [x] 2.3 Implement `get_path(pool: &DbPool, id: u64) -> Result<String, AppError>` — walk parent_id chain to build breadcrumb path (e.g., "Salon → Bibliothèque 1 → Étagère 3")
  - [x] 2.4 Add `pub mod location;` to `src/models/mod.rs`

- [x] Task 3: Volume service layer (AC: 1, 2, 3, 4, 6)
  - [x] 3.1 Create `src/services/volume.rs` with `VolumeService` struct
  - [x] 3.2 Implement `validate_vcode(label: &str) -> bool` — pure function, checks V + exactly 4 digits regex
  - [x] 3.3 Implement `create_volume(pool: &DbPool, label: &str, title_id: u64) -> Result<VolumeModel, AppError>` — validates V-code format, checks label uniqueness, checks title exists, creates volume
  - [x] 3.4 Implement `assign_location(pool: &DbPool, label: &str, location_label: &str) -> Result<(VolumeModel, String), AppError>` — finds volume by label, finds location by label, updates volume location, returns (volume, location_path)
  - [x] 3.5 Add `tracing::info!` for all operations
  - [x] 3.6 Unit tests: V-code validation (valid, too short, too long, non-numeric, lowercase)
  - [x] 3.7 Add `pub mod volume;` to `src/services/mod.rs`

- [x] Task 4: Session helpers for volume context (AC: 3, 8)
  - [x] 4.1 Add `set_last_volume_label(pool: &DbPool, token: &str, label: &str) -> Result<(), AppError>` to `src/models/session.rs` — stores last scanned V-code in session data JSON for L-code→volume association
  - [x] 4.2 Add `get_last_volume_label(pool: &DbPool, token: &str) -> Result<Option<String>, AppError>`
  - [x] 4.3 Add `increment_session_counter(pool: &DbPool, token: &str) -> Result<u64, AppError>` — reads/increments `session_item_count` in session data, returns new count
  - [x] 4.4 Add `get_session_counter(pool: &DbPool, token: &str) -> Result<u64, AppError>`
  - [x] 4.5 Unit tests for session data manipulation

- [x] Task 5: Client-side V-code validation in scan-field.js (AC: 4)
  - [x] 5.1 Add `validateVcode(code)` function — checks `/^V\d{4}$/` AND code !== "V0000"
  - [x] 5.2 In keydown handler: if prefix is "vcode" and `!validateVcode(code)`, inject local error FeedbackEntry (from `data-vcode-error` attribute on scan field), abort server submission
  - [x] 5.3 Add `data-vcode-error="{{ vcode_error }}"` attribute to scan_field.html template
  - [x] 5.4 Add `vcode_error: String` to CatalogTemplate, populated from `t!("feedback.vcode_invalid")`
  - [x] 5.5 Export via `window.mybibliValidateVcode` for test access

- [x] Task 6: Update scan handler for V-code and L-code processing (AC: 1, 2, 3, 4, 6, 7)
  - [x] 6.1 In `handle_scan` V-code branch: validate format server-side, check current_title_id from session, call `VolumeService::create_volume`, return success/error FeedbackEntry with OOB context banner update
  - [x] 6.2 In `handle_scan` V-code branch: store last_volume_label in session for subsequent L-code scan
  - [x] 6.3 In `handle_scan` V-code branch: increment session counter on success (in handler, after service call), include OOB session counter update
  - [x] 6.4 In `handle_scan` L-code branch: first check if L-code exists in DB — if not found, return warning regardless of volume context
  - [x] 6.5 In `handle_scan` L-code branch: if L-code exists and last_volume_label in session, call `VolumeService::assign_location`, return success FeedbackEntry
  - [x] 6.6 In `handle_scan` L-code branch: if L-code exists but no volume context, return info stub ("Location contents — coming in Story 1-6")
  - [x] 6.7 Handle UNIQUE constraint violation on V-code INSERT gracefully — catch DB error, return user-friendly "already assigned" message with title name

- [x] Task 7: Update context banner for volume count (AC: 5)
  - [x] 7.1 Update `context_banner_html` signature to accept `volume_count: u64` parameter
  - [x] 7.2 Update all call sites of `context_banner_html` (ISBN scan + manual title + volume scan)
  - [x] 7.3 Query volume count via `VolumeModel::count_by_title` for current title on every banner OOB swap
  - [x] 7.4 Banner format: use i18n key `title.current_banner_with_volumes` with count interpolation

- [x] Task 8: Session counter display in catalog toolbar (AC: 8)
  - [x] 8.1 Update `templates/components/catalog_toolbar.html` — add `<span id="session-counter" class="text-xs text-stone-500 dark:text-stone-400" aria-label="{{ session_counter_aria }}">` right-aligned
  - [x] 8.2 OOB swap target `#session-counter` in scan handler responses (both title and volume creation)
  - [x] 8.3 Counter reads from session data, displays "N items this session" (right-aligned, caption size)
  - [x] 8.4 Add `session_counter_aria: String` to CatalogTemplate from `t!("catalog.session_counter_aria")`

- [x] Task 9: i18n keys for volume operations (AC: all)
  - [x] 9.1 Add volume feedback keys to en.yml and fr.yml: `feedback.volume_created`, `feedback.volume_created_suggestion`, `feedback.volume_duplicate`, `feedback.volume_no_title`, `feedback.volume_shelved`, `feedback.vcode_invalid`, `feedback.lcode_not_found`, `feedback.location_stub`
  - [x] 9.2 Add session counter keys: `catalog.session_counter` ("N items this session"), `catalog.session_counter_aria` ("N items cataloged this session")
  - [x] 9.3 Add banner key with volume count: `title.current_banner_with_volumes` (e.g., "Current: %{title} — %{count} vol")
  - [x] 9.4 Add V-code error key: `feedback.vcode_invalid` ("Invalid volume code format. Expected V followed by 4 digits (e.g., V0042).")
  - [x] 9.5 Add location reassignment: no warning needed — reassignment replaces silently (overwrite semantics)

- [x] Task 10: Unit tests (AC: all)
  - [x] 10.1 VolumeService: validate_vcode (valid V0001-V9999, invalid V0000, V123, V00001, VABC, v0042 lowercase)
  - [x] 10.2 VolumeModel: Display trait, struct construction
  - [x] 10.3 LocationModel: path building logic (single node, 3-level chain, null parent)
  - [x] 10.4 detect_code_type: V-code and L-code edge cases (already partially tested, add V0000)
  - [x] 10.5 Session helpers: set/get last_volume_label, increment/get session counter
  - [x] 10.6 Feedback HTML tests for volume messages (green created, red duplicate with title name, amber no title)
  - [x] 10.7 Context banner with volume count rendering
  - [x] 10.8 JS validateVcode function tests

- [x] Task 11: Playwright E2E tests (AC: all)
  - [x] 11.1 Test: Scan ISBN then V-code → volume created, success feedback, banner shows "1 vol"
  - [x] 11.2 Test: Scan same V-code again → error feedback "already assigned to {title}"
  - [x] 11.3 Test: Scan V-code without prior ISBN → warning "no title selected"
  - [x] 11.4 Test: Scan invalid V-code format (V123) → client-side error feedback (no server request)
  - [x] 11.5 Test: Scan V0000 → client-side error (out of range)
  - [x] 11.6 Test: Scan ISBN, V-code, second V-code → banner shows "2 vol"
  - [x] 11.7 Test: Session counter increments on volume creation (check #session-counter text)
  - [x] 11.8 Test: Scan V-code then L-code → volume shelved, success feedback with location path
  - [x] 11.9 Test: Scan L-code without volume context → info stub
  - [x] 11.10 Test: Anonymous user cannot create volumes (303 redirect)
  - [x] 11.11 Test: Accessibility scan (axe-core) on catalog after volume operations

## Dev Notes

### Architecture Compliance

- **Service layer:** Business logic in `src/services/volume.rs`, NOT in route handlers
- **Error handling:** `AppError` enum — `BadRequest` for validation, `NotFound` for missing, `Database` for SQLx
- **Logging:** `tracing::info!` for creation, `tracing::debug!` for lookups — never `println!`
- **i18n:** All user-facing text via `t!("key")` — never hardcode strings
- **DB queries:** `WHERE deleted_at IS NULL` on every SELECT/JOIN
- **DB pool:** Pass as `pool: &DbPool`
- **HTMX:** Check `HxRequest` header, return fragment or full page
- **HTML escaping:** Manual `& < > " '` on all user data (established pattern)

### Database Schema (Existing — DO NOT Modify)

**volumes table:**
- `id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY`
- `title_id BIGINT UNSIGNED NOT NULL` — FK to titles
- `label CHAR(5) NOT NULL` — V-code (UNIQUE constraint: `uq_volumes_label`)
- `condition_state_id BIGINT UNSIGNED NULL` — FK to volume_states (not used in this story)
- `edition_comment VARCHAR(255) NULL` — not used in this story
- `location_id BIGINT UNSIGNED NULL` — FK to storage_locations
- `deleted_at`, `version` — soft delete + optimistic locking

**storage_locations table:**
- `id`, `parent_id` (self-referencing FK), `name`, `node_type`, `label CHAR(5) UNIQUE`
- Hierarchical tree via `parent_id` — walk chain for breadcrumb path

**CRITICAL:** `label` has UNIQUE constraint — duplicate V-code INSERT will fail with DB error. Handle this gracefully in VolumeService with a user-friendly error message, don't let raw SQLx error reach the user.

### V-code Format

- Pattern: `V` + exactly 4 digits → `V0001` to `V9999` (V0000 rejected)
- Case-sensitive: uppercase `V` only — lowercase `v` is rejected
- Regex (Rust): `^V\d{4}$` + check != "V0000"
- Regex (JS): `/^V\d{4}$/` (already in scan-field.js `detectPrefix`) + V0000 check in `validateVcode`
- Storage: CHAR(5) in database
- Ceiling: V9999 max (future migration can extend to V00001)
- Soft-deleted V-codes: `find_by_label` uses `WHERE deleted_at IS NULL`, so soft-deleted labels can be reused

### Location Reassignment Semantics

- A volume can have its location changed freely — scanning a new L-code after a V-code **replaces** the existing location silently (no warning, no confirmation)
- This supports physical reorganization: move books between shelves without extra steps

### Scan Flow State Machine (Updated for This Story)

Current V-code/L-code handling in `handle_scan` returns stub feedback. This story replaces stubs with real processing:

**V-code scan:**
1. Validate format (V + 4 digits)
2. Check `current_title_id` from session → if None, return warning
3. Check label uniqueness via `VolumeModel::find_by_label` → if exists, return error with title name
4. Create volume via `VolumeModel::create(pool, title_id, label)`
5. Store `last_volume_label` in session (for subsequent L-code association)
6. Increment session counter
7. Return success FeedbackEntry + OOB banner (with vol count) + OOB session counter

**L-code scan:**
1. Validate format (L + 4 digits — already detected by `detect_code_type`)
2. Check `last_volume_label` from session → if None, return info stub
3. Find location by label → if not found, return warning
4. Find volume by `last_volume_label` → assign location
5. Return success FeedbackEntry with location path

### Session Data Structure (Extended)

Session `data` JSON now tracks:
```json
{
  "current_title_id": 42,
  "last_volume_label": "V0042",
  "session_item_count": 5
}
```

### Context Banner Format (Updated)

```
Current: L'Étranger — 3 vol
```

Update `context_banner_html` signature to:
```rust
fn context_banner_html(title_name: &str, media_type: &str, volume_count: u64) -> String
```

### Feedback Messages (Volume-specific)

- **Success (green):** "Volume V0042 attached to *L'Étranger*." + suggestion "Scan L-code to shelve."
- **Error (red):** "Label V0042 is already assigned to *L'Écume des jours*. Scan a different label."
- **Warning (amber):** "No title selected. Scan an ISBN first to establish a title context."
- **Error (red):** "Invalid volume code format. Expected V followed by 4 digits (e.g., V0042)."
- **Success (green):** "Volume V0042 shelved at *Salon → Bibliothèque 1 → Étagère 3*."
- **Warning (amber):** "Location L9999 not found."

### Previous Story Intelligence (Story 1-3)

**Patterns to follow:**
- `feedback_html(variant, message, suggestion)` for generating feedback entries
- `context_banner_html(title, media_type)` for OOB banner swaps
- `HtmxResponse { main, oob: vec![OobUpdate{...}] }` for responses with OOB
- `SessionModel::set_current_title()` pattern for session data manipulation
- Runtime `sqlx::query()` (not macro) for new queries without `.sqlx` cache
- `detect_code_type()` already identifies V-codes and L-codes correctly
- `injectLocalFeedback()` in JS for client-side feedback without server
- `html_escape()` for all user-provided data in HTML output

**Code review corrections from 1-3 (DO NOT repeat):**
- Use `t!()` for ALL user-facing strings — no hardcoded English
- Log warnings on session update failures (`if let Err(e) = ...`)
- `stopImmediatePropagation` (not `stopPropagation`) for form validation
- Dismiss button on warning/error feedback entries
- Trim form values before storage
- Check `is_htmx` flag and provide non-HTMX fallback

### Project Structure Notes

Files to create:
- `src/models/volume.rs` — Volume database model
- `src/models/location.rs` — Location database model (minimal, for path lookup)
- `src/services/volume.rs` — Volume business logic

Files to modify:
- `src/models/mod.rs` — Add `pub mod volume; pub mod location;`
- `src/services/mod.rs` — Add `pub mod volume;`
- `src/models/session.rs` — Add volume/counter session helpers
- `src/routes/catalog.rs` — Replace V-code/L-code stubs with real processing, update context_banner_html signature
- `templates/components/catalog_toolbar.html` — Add session counter display
- `locales/en.yml` — Add volume/location/counter i18n keys
- `locales/fr.yml` — Add French translations
- `tests/e2e/specs/journeys/catalog-title.spec.ts` — Or create new `catalog-volume.spec.ts`

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-1, Story 1.4]
- [Source: _bmad-output/planning-artifacts/prd.md#FR4, #FR5, #FR7, #FR30, #FR90]
- [Source: _bmad-output/planning-artifacts/architecture.md#volumes-table, #V-code-pattern]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#ScanField-sequences, #FeedbackEntry, #CatalogToolbar]
- [Source: _bmad-output/implementation-artifacts/1-3-title-crud-and-isbn-scanning.md#Dev-Notes]
- [Source: migrations/20260329000000_initial_schema.sql#volumes-table, #storage_locations-table]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- SQLx offline mode (`SQLX_OFFLINE=true`) — DB not available
- Runtime `sqlx::query()` for all new queries
- Session data helpers refactored — `load_session_data`/`save_session_data` internal helpers eliminate duplication
- context_banner_html signature updated to include volume_count (breaking change from Story 1-3, all call sites updated)

### Completion Notes List

- 62 unit tests passing (14 new + 48 existing), 0 clippy warnings
- Tasks 1-11 implemented: VolumeModel, LocationModel (with parent chain path), VolumeService (validate_vcode, create_volume, assign_location), session helpers (last_volume_label, session counter), client-side V-code validation, scan handler V-code/L-code processing, context banner with volume count, session counter OOB display, i18n keys (en+fr), unit tests, Playwright E2E tests
- Session data refactored: set_current_title and get_current_title_id now use shared load/save helpers
- UNIQUE constraint on volumes.label handled gracefully — duplicate detection via DB error message pattern matching
- V-code validation: V0000 rejected, case-sensitive (uppercase V only), client-side + server-side
- L-code scan: existence check first, then volume context check, then assignment or stub

### Change Log

- 2026-03-30: Story 1.4 implementation complete — Volume Management

### File List

**Created:**
- `src/models/volume.rs`
- `src/models/location.rs`
- `src/services/volume.rs`
- `tests/e2e/specs/journeys/catalog-volume.spec.ts`

**Modified:**
- `src/models/mod.rs` — added `pub mod volume; pub mod location;`
- `src/models/session.rs` — added volume/counter session helpers, refactored to shared load/save
- `src/services/mod.rs` — added `pub mod volume;`
- `src/routes/catalog.rs` — replaced V-code/L-code stubs with real processing, updated context_banner_html signature, added session_counter_html, added VolumeService/VolumeModel imports
- `templates/components/scan_field.html` — added `data-vcode-error` attribute
- `templates/components/catalog_toolbar.html` — added session counter display element
- `static/js/scan-field.js` — added validateVcode function, client-side V-code validation
- `locales/en.yml` — added volume feedback, session counter, banner with volumes keys
- `locales/fr.yml` — added French translations for all new keys
