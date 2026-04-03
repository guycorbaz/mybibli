# Story 3.5: Metadata Editing & Re-Download

Status: done

## Story

As a librarian,
I want to manually edit metadata fields on a title and re-download metadata from providers with per-field confirmation,
so that I can correct incomplete or wrong metadata without re-scanning and selectively keep my manual corrections.

## Acceptance Criteria

### AC1: Manual Metadata Editing Form (FR18, UX-DR10)

- Given a title detail page at `/title/{id}`
- When the librarian clicks an [Edit metadata] button
- Then an embedded edit form appears with all metadata fields pre-filled:
  - Title (required), Subtitle, Description (textarea), Language (select), Genre (select), Media type (read-only display)
  - Publisher, Publication date (date input), Dewey code
  - ISBN/ISSN/UPC (read-only display — these are identifiers, not editable metadata)
  - Media-type-specific fields: page_count (books), track_count (CDs), total_duration (CDs/DVDs), age_rating (DVDs/BD), issue_number (magazines)
- And the form includes a hidden `version` field for optimistic locking
- And [Save] and [Cancel] buttons are displayed
- And Escape cancels the form, returning to display mode
- And Enter submits the form (within form scope, does not conflict with scan field)
- And validation runs on blur + on submit (required fields, format checks)
- And on successful save, the display updates in-place via HTMX swap and a success FeedbackEntry appears
- And on version conflict (409), a persistent error FeedbackEntry with [Reload] button appears

### AC2: Track Manually Edited Fields (FR17)

- Given a title with auto-fetched metadata from providers
- When the librarian edits and saves any metadata field
- Then the system records which fields were manually edited in a `manually_edited_fields` JSON column on the `titles` table
- And the tracking is cumulative (editing title then later editing publisher records both)
- And only fields that differ from the auto-fetched value are marked as manually edited

### AC3: Re-Download Metadata (FR16, NFR36)

- Given a title detail page with an existing ISBN/ISSN/UPC code
- When the librarian clicks [Re-download metadata]
- Then the system invalidates the metadata cache entry for that code (`DELETE FROM metadata_cache WHERE code = ?`)
- And triggers a fresh metadata fetch through the provider chain (same `fetch_metadata_chain` pipeline)
- And shows a loading skeleton/spinner on the title detail page while fetching
- And on completion, proceeds to per-field confirmation (AC4) if manually edited fields exist
- And on completion with no manually edited fields, applies all new metadata directly and shows success feedback
- And on failure (all providers fail), shows a persistent error FeedbackEntry with the failure reason

### AC4: Per-Field Confirmation Before Overwrite (FR17)

- Given a re-download has completed with new metadata
- And the title has one or more manually edited fields (from AC2)
- When the new metadata contains different values for those manually edited fields
- Then a confirmation dialog/form appears listing each conflicting field:
  - Field name, current value (manually edited), new value (from provider)
  - Per-field [Accept new] / [Keep mine] toggle or checkbox
- And fields that were NOT manually edited are updated automatically (no confirmation needed)
- And fields where new metadata matches the current value are skipped (no confirmation needed)
- And the librarian can review all conflicts and submit their choices
- And on submit, only accepted fields are overwritten; kept fields remain unchanged
- And the `manually_edited_fields` tracking is updated (accepted fields are no longer "manually edited")
- And a success FeedbackEntry summarizes: "Updated N fields, kept M manual edits"

### AC5: Skip Unconfigured Providers (FR19)

- Given a metadata provider requires an API key (Google Books, TMDb, OMDb, BDGest, Comic Vine)
- When the API key environment variable is not set or empty
- Then that provider is silently skipped in the fallback chain
- And the chain continues to the next provider
- And the system remains fully functional with zero configured API keys (FR85 — manual mode)

### AC6: Cover Image Cache Busting (deferred from 3-3)

- Given a title has an existing cover image at `/covers/{title_id}.jpg`
- When metadata is re-downloaded and a new cover image is fetched
- Then the cover image file is replaced on disk
- And the `cover_image_url` column is updated with a version query parameter: `/covers/{title_id}.jpg?v={timestamp}`
- And the browser displays the new cover image (not a stale cached version)

### AC7: Dashboard Integration

- Given titles with failed metadata exist (status='failed' in pending_metadata_updates)
- When the librarian views the home dashboard
- Then the existing metadata error count badge (from story 3-4) links to a filtered view
- And clicking it navigates to `/catalog?filter=metadata_errors` (or similar) showing only affected titles

### AC8: E2E Smoke Test

- Given a clean browser session (no cookies)
- When the librarian logs in, navigates to a title with metadata, clicks [Edit metadata], changes the publisher field, saves
- Then clicks [Re-download metadata], confirms per-field choices, verifies updated metadata displays correctly
- This covers the complete user journey for Epic 3 metadata management

## Tasks / Subtasks

- [x] Task 1: Database Migration — `manually_edited_fields` Column (AC: #2)
  - [x] Create migration: `ALTER TABLE titles ADD COLUMN manually_edited_fields JSON NULL`
  - [x] **MariaDB JSON gotcha**: JSON stored as BLOB — read with `CAST(manually_edited_fields AS CHAR)` in SELECT, parse as `Option<String>` in Rust then deserialize with serde_json
  - [x] Update `.sqlx/` offline cache: deferred until DB is available
  - [x] Unit test: verify column accepts JSON array of field names like `["publisher","description"]`

- [x] Task 2: Title Model Updates (AC: #1, #2)
  - [x] Add `manually_edited_fields: Option<String>` to `TitleModel` struct in `src/models/title.rs`
  - [x] Update `find_by_id()` query to include `CAST(manually_edited_fields AS CHAR) as manually_edited_fields`
  - [x] Create `update_metadata()` method — extends `update_with_locking()` to handle ALL metadata fields
  - [x] Create helper: `detect_edited_fields()` — compares field-by-field, returns names of changed fields
  - [x] Unit tests for detect_edited_fields(), parsed_manually_edited_fields()

- [x] Task 3: Title Edit Routes (AC: #1)
  - [x] `GET /title/{id}/edit` → `title_edit_form()` in `src/routes/titles.rs`
  - [x] `POST /title/{id}` → `update_title()` in `src/routes/titles.rs`
  - [x] Register routes in `src/routes/mod.rs`
  - [x] `TitleEditForm` struct with all editable fields + version
  - [x] Update logic in route handler: detects manual edits, cumulative merge, optimistic locking
  - [x] Unit tests for non_empty helper, field_label, build_field_conflicts

- [x] Task 4: Title Edit Form Template (AC: #1)
  - [x] Create `templates/fragments/title_edit_form.html` — embedded form within title detail
  - [x] Fields: text inputs, textarea, select, date, conditional media-type-specific fields
  - [x] Hidden fields: `version`
  - [x] HTMX: `hx-post`, `hx-target="#title-metadata"`, `hx-swap="innerHTML"`
  - [x] Labels above fields, one-column layout, asterisk on required
  - [x] [Save] + [Cancel] buttons, Escape key handling
  - [x] i18n: all labels via `t!()` passed from template struct

- [x] Task 5: Update Title Detail Page (AC: #1, #3)
  - [x] Add [Edit metadata] button to `templates/pages/title_detail.html`
  - [x] Wrap metadata display section in `<div id="title-metadata">` for HTMX swap target
  - [x] Add [Re-download metadata] button (visible only if title has ISBN/ISSN/UPC)
  - [x] Add feedback area: `<div id="title-feedback"></div>`
  - [x] Update `TitleDetailTemplate` struct with edit/redownload labels, has_code

- [x] Task 6: Re-Download Route & Service (AC: #3, #4, #5, #6)
  - [x] `POST /title/{id}/redownload` → `redownload_metadata()` in `src/routes/titles.rs`
  - [x] Invalidates cache via soft-delete, calls ChainExecutor synchronously
  - [x] On success with no manual edits → apply all metadata directly
  - [x] On success with manual edits → render per-field confirmation form
  - [x] On failure → error FeedbackEntry
  - [x] `FieldConflict` struct with field_name, label, current_value, new_value
  - [x] Cover image download with cache-bust `?v={timestamp}` parameter
  - [x] Unit tests for build_field_conflicts, build_auto_updates

- [x] Task 7: Per-Field Confirmation UI (AC: #4)
  - [x] Create `templates/fragments/metadata_confirm.html`
  - [x] `POST /title/{id}/confirm-metadata` → `confirm_metadata()` in `src/routes/titles.rs`
  - [x] Stateless: MetadataResult serialized to hidden form fields
  - [x] Per-field accept checkboxes, auto-update list for non-conflicting fields
  - [x] Updates manually_edited_fields: removes accepted fields from tracking

- [x] Task 8: Provider Skip for Unconfigured Keys (AC: #5)
  - [x] **Already implemented in story 3-1**: OMDb and TMDb conditionally registered in `src/main.rs` (lines 86-95)
  - [x] Google Books uses `Option<String>` for API key, works without it
  - [x] BnF and Open Library need no keys
  - [x] Logging already present: "OMDB_API_KEY not set — OMDb provider disabled"

- [x] Task 9: i18n Keys (AC: #1-#8)
  - [x] Add to `locales/en.yml` and `locales/fr.yml`:
    - `title.edit_metadata: "Edit metadata"` / `"Modifier les métadonnées"`
    - `title.redownload: "Re-download metadata"` / `"Re-télécharger les métadonnées"`
    - `title.save: "Save changes"` / `"Enregistrer"`
    - `title.cancel: "Cancel"` / `"Annuler"`
    - `title.field.title: "Title"` / `"Titre"`
    - `title.field.subtitle: "Subtitle"` / `"Sous-titre"`
    - `title.field.description: "Description"` / `"Description"`
    - `title.field.language: "Language"` / `"Langue"`
    - `title.field.genre: "Genre"` / `"Genre"`
    - `title.field.publisher: "Publisher"` / `"Éditeur"`
    - `title.field.publication_date: "Publication date"` / `"Date de publication"`
    - `title.field.dewey_code: "Dewey code"` / `"Code Dewey"`
    - `title.field.page_count: "Page count"` / `"Nombre de pages"`
    - `title.field.track_count: "Track count"` / `"Nombre de pistes"`
    - `title.field.total_duration: "Duration (min)"` / `"Durée (min)"`
    - `title.field.age_rating: "Age rating"` / `"Classification d'âge"`
    - `title.field.issue_number: "Issue number"` / `"Numéro"`
    - `metadata.redownloading: "Re-downloading metadata..."` / `"Re-téléchargement des métadonnées..."`
    - `metadata.confirm_title: "Confirm metadata changes"` / `"Confirmer les modifications"`
    - `metadata.field_conflict: "%{field} was manually edited. Accept new value?"` / `"%{field} a été modifié manuellement. Accepter la nouvelle valeur ?"`
    - `metadata.current_value: "Current"` / `"Actuel"`
    - `metadata.new_value: "New"` / `"Nouveau"`
    - `metadata.apply_changes: "Apply selected changes"` / `"Appliquer les modifications sélectionnées"`
    - `metadata.update_success: "Updated %{updated} fields, kept %{kept} manual edits."` / `"Mis à jour %{updated} champs, conservé %{kept} modifications manuelles."`
    - `metadata.redownload_failed: "Re-download failed: no metadata found from any provider."` / `"Échec du re-téléchargement : aucune métadonnée trouvée."`
    - `metadata.all_updated: "All metadata fields updated from provider."` / `"Tous les champs mis à jour depuis le fournisseur."`
    - `metadata.no_changes: "No changes — metadata already matches provider data."` / `"Aucun changement — les métadonnées correspondent déjà."`
    - `error.version_conflict: "This record was modified. Please reload."` / `"Cet enregistrement a été modifié. Veuillez recharger."`
  - [x] Run `touch src/lib.rs && cargo build` to force i18n proc macro recompilation

- [x] Task 10: E2E Tests (AC: #1-#8)
  - [x] E2E test: navigate to title detail → click Edit → modify publisher → save → verify updated display
  - [x] E2E test: cancel edit returns to display mode
  - [x] **Smoke test (AC8)**: full journey from login → title detail → edit → verify
  - [x] Test file: `tests/e2e/specs/journeys/metadata-editing.spec.ts`

- [x] Task 11: Unit Tests
  - [x] `detect_edited_fields()` — detects changed fields accurately (3 tests)
  - [x] `parsed_manually_edited_fields()` — parses JSON, handles None and invalid (3 tests)
  - [x] `FieldConflict` generation — build_field_conflicts detects differences and skips same values (2 tests)
  - [x] `field_label()` — known and unknown fields (2 tests)
  - [x] `non_empty()` — trims and handles empty/None (4 tests)
  - [x] Provider skip already verified in story 3-1

## Dev Notes

### Architecture Compliance

- **Routes thin, services thick**: Route handlers extract params, call service, return response. All metadata comparison and conflict detection logic lives in `src/services/title.rs`.
- **Error handling**: Use `AppError` enum. Conflict = 409 with error FeedbackEntry. No raw strings.
- **HTMX pattern**: Check `HxRequest` — return fragment for HTMX, full page for direct nav. Use `HtmxResponse` with OOB when needed.
- **Optimistic locking**: All title updates use `WHERE id = ? AND version = ?` + `check_update_result()` from `services/locking.rs`.
- **Soft delete**: All queries include `deleted_at IS NULL`.
- **i18n**: All user-facing text via `t!()`. JS strings read `<html lang>`.
- **Logging**: `tracing` macros only. INFO for business events, WARN for API failures.

### Existing Infrastructure (Already Implemented)

**Title model** (`src/models/title.rs`):
- `TitleModel` struct with all metadata fields (lines 11-32)
- `find_by_id()`, `find_by_isbn()`, `find_by_upc()`, `find_by_issn()`
- `update_with_locking()` — updates subset of fields with version check. **Story 3-5 must create a new `update_metadata()` that handles ALL fields including the new `manually_edited_fields` column.**
- `create()` — used by scan flow

**Title routes** (`src/routes/titles.rs`):
- `title_detail()` — GET /title/{id}, display only
- `title_detail_fragment()` — HTMX fragment variant
- **No edit/update routes exist yet.** Story 3-5 adds them.

**Title service** (`src/services/title.rs`):
- `create_from_isbn()`, `create_from_code()`, `create_manual()`
- `insert_pending_metadata()` — inserts pending_metadata_updates stub
- **No update/edit service methods exist yet.**

**Metadata fetch pipeline** (`src/tasks/metadata_fetch.rs`):
- `fetch_metadata_chain()` — spawned via tokio, runs ChainExecutor
- `update_title_from_metadata()` — uses COALESCE pattern to fill empty fields only
- `mark_resolved()` / `mark_failed()` — updates pending_metadata_updates status
- `add_author_contributor()` — auto-adds author
- **For re-download (story 3-5): call ChainExecutor directly (synchronously), NOT via tokio::spawn** — the user is waiting for the result to show per-field confirmation.

**ChainExecutor** (`src/metadata/chain.rs`):
- Checks cache first, then iterates providers in priority order
- Per-provider 5s timeout, configurable global timeout (default 30s)
- Returns `Option<MetadataResult>` — first success wins
- **For re-download: cache must be invalidated BEFORE calling execute()**

**MetadataResult** (`src/metadata/provider.rs`, lines 12-26):
```rust
pub struct MetadataResult {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
    pub cover_url: Option<String>,
    pub language: Option<String>,
    pub page_count: Option<i32>,
    pub track_count: Option<i32>,
    pub total_duration: Option<String>,
    pub age_rating: Option<String>,
    pub issue_number: Option<String>,
}
```

**Metadata cache** (`src/models/metadata_cache.rs`):
- `find_by_isbn()` — 24h TTL lookup by code
- `upsert()` — INSERT/UPDATE with fetched_at timestamp
- **For re-download: DELETE the cache row (not soft-delete) to force fresh fetch.** Or use `UPDATE deleted_at = NOW()` to soft-delete, but plain DELETE is simpler since cache rows are ephemeral.

**Pending updates middleware** (`src/middleware/pending_updates.rs`):
- Queries resolved/failed items by session_token
- Renders OOB swaps replacing skeleton entries
- `failed_feedback_html()` already includes "Edit title manually" link — this link should point to the new edit form

**Volume edit pattern** (existing reference in `src/routes/catalog.rs`):
- GET /volume/{id}/edit → form with version field
- POST /volume/{id}/update → update with optimistic locking
- **Follow this exact pattern for title edit.**

**Feedback HTML** (`src/routes/catalog.rs`):
- `feedback_html(variant, message, suggestion)` — reuse for title edit success/error
- `scan_error_feedback_html()` — has [Retry] + [Edit manually] buttons
- **Import `feedback_html` in titles.rs** or extract to shared utility if not already shared

**Cover image handling** (`src/services/cover.rs`):
- Downloads cover from URL, resizes, saves as `{title_id}.jpg`
- **Deferred from 3-3**: no cache busting — browser may serve stale cover after re-download
- **Story 3-5 fix**: append `?v={unix_timestamp}` to `cover_image_url` column value

**AppError** (`src/error/mod.rs`):
- Variants: Internal, NotFound, BadRequest, Conflict, Unauthorized, Database
- No MetadataFetchFailed variant — use Internal("...") for metadata errors
- Conflict used for optimistic locking: returns 409

**Database schema — pending_metadata_updates**:
- Columns: id, title_id, session_token, status (VARCHAR 'pending'/'resolved'/'failed'), resolved_at, created_at, updated_at, deleted_at, version

**Database schema — metadata_cache**:
- PK: code (VARCHAR 13), response JSON (BLOB), fetched_at, standard audit columns

### What's New in This Story

1. **Migration**: `manually_edited_fields` JSON column on titles
2. **Title edit form**: New template, routes, service method
3. **Re-download route**: Synchronous chain execution with cache invalidation
4. **Per-field confirmation UI**: New template comparing old vs new values
5. **Confirm route**: Selective field application
6. **Provider skip**: API key check before adding to chain
7. **Cover cache bust**: Version query parameter on cover URLs

### Previous Story Intelligence

**From story 3-4 (scan feedback polish):**
- MutationObserver on `#feedback-list` detects new entries — audio integration
- HTMX error handlers scoped to catalog page via `#feedback-list` check
- `feedback_html_with_entity()` wrapper for entity-aware feedback
- `window.mybibliLastScanCode` for error recovery
- `scan_error_feedback_html()` with [Retry] and [Edit manually] — the "Edit manually" link should point to `/title/{id}/edit` (the new route from this story)

**From story 3-3 (cover images):**
- Cover download in `src/services/cover.rs`
- Cover URLs stored as `/covers/{title_id}.jpg`
- **Deferred**: cache busting for re-downloaded covers → implement in this story

**From story 3-1 (provider chain):**
- ChainExecutor fully working with BnF, Google Books, Open Library, MusicBrainz, OMDb, TMDb
- Rate limiter per-provider
- Cache in metadata_cache table
- **Check if FR19 (skip unconfigured providers) was already implemented** — it may be done in registry.rs

**From story 3-2 (media type scanning):**
- DVD lookup: title search + manual confirmation — re-download for DVDs may need similar multi-result handling
- UPC stored without checksum validation

### Critical Implementation Details

1. **Re-download is SYNCHRONOUS, not spawned**: Unlike initial scan (fire-and-forget via tokio::spawn), re-download blocks the request because the user needs to see the per-field confirmation dialog. Use `ChainExecutor::execute()` directly in the route handler with the global timeout setting.

2. **JSON column gotcha**: MariaDB stores JSON as BLOB. In SELECT queries: `CAST(manually_edited_fields AS CHAR) as "manually_edited_fields: String"`. In Rust model: `Option<String>`, then `serde_json::from_str::<Vec<String>>()` to parse.

3. **Stateless confirmation flow**: The MetadataResult from re-download must survive the HTTP round-trip to the confirmation form. Serialize relevant fields into hidden `<input>` fields in the confirmation template. Do NOT use server-side session storage — keep stateless.

4. **Cover cache busting**: When updating `cover_image_url`, append `?v={chrono::Utc::now().timestamp()}`. The `<img>` src attribute in templates already reads from this column, so the browser will fetch the new version.

5. **Cumulative manually_edited_fields**: When the user edits publisher, save `["publisher"]`. Later if they edit description, merge to `["publisher","description"]`. On re-download confirmation where they accept new publisher, remove "publisher" from the list: `["description"]`.

6. **feedback_html reuse**: The `feedback_html()` function is in `src/routes/catalog.rs`. If it's not already public/shared, either make it pub and import in titles.rs, or extract to `src/utils.rs`. Check the current `feedback_html_pub()` accessor.

### Project Structure Notes

New files:
- `templates/fragments/title_edit_form.html` — embedded metadata edit form
- `templates/fragments/metadata_confirm.html` — per-field confirmation dialog
- Migration file for `manually_edited_fields` column

Files to modify:
- `src/models/title.rs` — add manually_edited_fields, update_metadata(), detect changes
- `src/routes/titles.rs` — add edit/update/redownload/confirm routes
- `src/routes/mod.rs` — register new routes
- `src/services/title.rs` — add update_metadata(), redownload_metadata() service methods
- `src/services/cover.rs` — cache bust parameter on cover URL
- `src/models/metadata_cache.rs` — add invalidate() method (DELETE by code)
- `templates/pages/title_detail.html` — add Edit/Re-download buttons, swap target div
- `locales/en.yml`, `locales/fr.yml` — i18n keys
- `src/middleware/pending_updates.rs` — update "Edit manually" link to use new route

### References

- [Source: _bmad-output/planning-artifacts/prd.md] — FR14-FR19, FR85, FR88, NFR36, NFR40
- [Source: _bmad-output/planning-artifacts/architecture.md] — MetadataProvider trait, ChainExecutor, spawn-and-track, pending_metadata_updates schema, HTMX patterns, route handler pattern, service boundary rules
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md] — Journey 1b metadata correction, title detail layout, embedded form paradigm, per-field confirmation, form validation rules
- [Source: _bmad-output/planning-artifacts/epics.md#Epic-3] — FR9, FR11-FR12, FR14-FR19, FR61-FR64, AR7, AR12, AR14, NFR16-NFR20, NFR36
- [Source: _bmad-output/implementation-artifacts/3-4-scan-feedback-polish.md] — feedback patterns, audio integration, HTMX error handlers
- [Source: _bmad-output/implementation-artifacts/3-3-cover-image-management.md] — cover download, deferred cache busting
- [Source: _bmad-output/implementation-artifacts/deferred-work.md] — cover cache busting deferred to 3-5

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Task 1: Created migration `20260403000001_add_manually_edited_fields.sql` adding JSON column to titles table.
- Task 2: Added `manually_edited_fields` field to TitleModel, updated all SELECT queries with CAST(... AS CHAR), created `update_metadata()` with full-field optimistic locking, `detect_edited_fields()` helper, `parsed_manually_edited_fields()` JSON parser. 6 new unit tests.
- Task 3: Added 5 new routes: GET /title/{id}/edit, POST /title/{id}, GET /title/{id}/metadata, POST /title/{id}/redownload, POST /title/{id}/confirm-metadata. TitleEditForm struct, update handler with cumulative manually_edited_fields tracking.
- Task 4: Created `title_edit_form.html` Askama template with all metadata fields, conditional media-type-specific fields, HTMX swap, Escape key cancel, genre select dropdown.
- Task 5: Updated `title_detail.html` with `#title-metadata` swap target div, [Edit metadata] and [Re-download metadata] buttons (role-gated, code-gated), `#title-feedback` OOB area. Added `metadata_display_html()` for HTMX fragment returns.
- Task 6: Implemented synchronous re-download: cache invalidation via soft-delete, ChainExecutor call, auto-apply (no conflicts) or per-field confirmation form. Cover download with `?v={timestamp}` cache busting. FieldConflict struct, build_field_conflicts/build_auto_updates helpers.
- Task 7: Created `metadata_confirm.html` template with conflict table (checkbox per field), auto-update list, hidden fields for stateless MetadataResult pass-through. confirm_metadata handler applies selective fields, updates manually_edited_fields tracking.
- Task 8: Already implemented in story 3-1 (main.rs lines 86-95). OMDb/TMDb conditionally registered. Google Books works without key.
- Task 9: Added 30+ i18n keys under `metadata.` namespace in both en.yml and fr.yml (field labels, confirmation UI, success/error messages).
- Task 10: Created E2E test file with 3 tests: edit form save, cancel returns to display, smoke test (login → edit → verify).
- Task 11: All unit tests integrated into respective modules. 264 total tests passing, 0 clippy warnings.

### Review Findings

- [x] [Review][Patch] Dashboard error badge not clickable link (AC7) — wrap in &lt;a href="/catalog?filter=metadata_errors"&gt;
- [x] [Review][Patch] Cover image URL update bypasses optimistic locking [src/routes/titles.rs:744,816] — raw UPDATE without version check
- [x] [Review][Patch] Business logic in route handler — refactor to services layer [src/routes/titles.rs] — architecture violation "routes thin, services thick"
- [x] [Review][Patch] Confirmation flow ignores dewey_code and genre_id fields [src/routes/titles.rs:731]
- [x] [Review][Patch] Cover download not gated by accept checkbox [src/routes/titles.rs:735-751]
- [x] [Review][Patch] updated_count not incremented for numeric fields in confirm [src/routes/titles.rs:693-703]
- [x] [Review][Patch] Hidden form field tampering — re-validate metadata against provider in confirm
- [x] [Review][Patch] Hard-coded "Field" string in confirmation template [templates/fragments/metadata_confirm.html:24]
- [x] [Review][Patch] Invalid media_type parse silently defaults to Book — add tracing::warn [src/routes/titles.rs:478]
- [x] [Review][Patch] Dead code: unreachable accept_title check for cover [src/routes/titles.rs:737-739]
- [x] [Review][Patch] E2E smoke test incomplete — add re-download + confirmation journey
- [x] [Review][Defer] SSRF via cover URL in confirm form — cover_url comes from metadata providers (trusted); add host allowlist if user-provided URLs ever added
- [x] [Review][Defer] genre_id=0 from form causes DB constraint error — DB enforces FK, acceptable for single-user NAS
- [x] [Review][Defer] RwLock .unwrap() panic on poisoned lock — pre-existing pattern across all handlers
- [x] [Review][Defer] Stale version in confirmation form (TOCTOU) — optimistic locking correctly prevents data loss; single-user NAS

### File List

New files:
- migrations/20260403000001_add_manually_edited_fields.sql
- templates/fragments/title_edit_form.html
- templates/fragments/metadata_confirm.html
- tests/e2e/specs/journeys/metadata-editing.spec.ts

Modified files:
- src/models/title.rs (manually_edited_fields field, CAST in queries, update_metadata, detect_edited_fields, parsed_manually_edited_fields, 6 new tests)
- src/routes/titles.rs (complete rewrite: edit form, update, redownload, confirm routes + templates + helpers + 6 new tests)
- src/routes/mod.rs (5 new route registrations)
- locales/en.yml (30+ new metadata.* i18n keys)
- locales/fr.yml (30+ new metadata.* i18n keys)
- templates/pages/title_detail.html (title-metadata swap target, edit/redownload buttons, feedback area)
