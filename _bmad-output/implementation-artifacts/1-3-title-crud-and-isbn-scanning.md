# Story 1.3: Title CRUD & ISBN Scanning

Status: done

## Story

As a librarian,
I want to scan an ISBN to create a new title or open an existing one, and optionally create titles manually,
so that I can catalog books efficiently with minimal typing.

## Acceptance Criteria (BDD)

### AC1: Create New Title from ISBN Scan

**Given** I scan an ISBN that does not exist in the database,
**When** the server processes the scan,
**Then** a new title is created with the ISBN and a default media type (book for 978/979), a `pending_metadata_updates` row is inserted (stub for future async fetch), and a success FeedbackEntry appears in the feedback list. The context banner updates to show the new title.

### AC2: Open Existing Title from ISBN Scan

**Given** I scan an ISBN that already exists in the database,
**When** the server processes the scan,
**Then** the existing title is opened (info FeedbackEntry with title name) instead of creating a duplicate. The context banner updates to show this title.

### AC3: Manual Title Creation Form Display

**Given** I click the "New title" button (+) or press Ctrl+N on /catalog,
**When** the title creation form appears,
**Then** I can fill in title (required), media type (required), genre (required), language (required, default "fr"), subtitle, publisher, publication date (DATE format YYYY-MM-DD, partial year YYYY allowed), and optional ISBN/ISSN/UPC fields. Required fields show asterisk (*) after label with `aria-required="true"`.

### AC4: Media Type-Dependent Form Adaptation

**Given** I select a media type on the title form,
**When** the media type changes,
**Then** the form adapts to show/hide fields relevant to that media type (e.g., page_count for books, track_count for CDs).

### AC5: Title Creation Form Submission and Validation

**Given** I submit the title creation form with valid data,
**When** the server processes the request,
**Then** the title is created, the form closes, the title becomes the "current title" in the catalog session, and the context banner updates.

**Given** I submit the form with missing required fields (title, media_type, genre, language),
**When** validation runs on blur and on submit,
**Then** inline error messages appear below each invalid field in red (`--color-danger`) caption-size text. Errors clear when the field value changes. The form is NOT submitted.

### AC6: Placeholder Cover Image for New Titles

**Given** a title is created,
**When** it is displayed anywhere,
**Then** a media-type placeholder SVG icon is shown as the cover image (since no cover is fetched yet).

### AC7: Error Handling for Title Creation

**Given** the server encounters an error during title creation,
**When** the error is returned,
**Then** a red FeedbackEntry appears with a localized error message (i18n key, not raw error).

### AC8: ISBN Checksum Validation (FR103)

**Given** I scan or type a code matching ISBN prefix (978/979),
**When** client-side validation runs before server submission,
**Then** the ISBN-13 checksum is validated. If invalid, an error FeedbackEntry appears immediately ("This code doesn't appear to be a valid ISBN. Please check the barcode and scan again.") and no server request is sent. If valid, the code is submitted to the server normally. Server-side re-validates the checksum as defense-in-depth.

### AC9: Non-ISBN Code Handling

**Given** I scan a code that is not an ISBN (ISSN 977..., UPC, or unknown format),
**When** the server processes the scan,
**Then** for V-codes and L-codes: return existing stub feedback (unchanged from Story 1-2). For ISSN/UPC/unknown codes: return an amber warning FeedbackEntry stating "Media type disambiguation not yet available. Use manual title creation for non-ISBN codes." (deferred to Story 1-7).

## Explicit Scope Boundaries

**Deferred to Story 1-7 (Scan Feedback & Async Metadata):**
- Real async metadata fetching from external APIs (FR11-FR19) — this story only inserts a `pending_metadata_updates` stub row
- UPC/ISSN media type disambiguation (FR9)
- Cover image fetching and storage (FR14-FR15)
- Skeleton FeedbackEntry variant (loading state with spinner)
- Audio feedback on scan events (FR63)

**Deferred to Story 1-4 (Volume Management):**
- Volume creation, V-code validation (FR4-FR5)
- Session counter display on /catalog (FR108)

**Deferred to Story 1-5 (Contributor Management):**
- Contributor CRUD and title-contributor associations (FR51-FR54)

## Tasks / Subtasks

**EXECUTION ORDER: Task 1 (migrations) must run first — genre FK constraint blocks all title inserts.**

- [x] Task 1: Seed migrations — MUST EXECUTE FIRST (AC: 5)
  - [x] 1.1 Create migration `20260330000001_seed_default_genres.sql` with default genres (Roman, BD, Science-Fiction, Policier, Jeunesse, Musique, Film, Documentaire, Revue, Rapport, Non classé) using `INSERT INTO genres (name) SELECT ... WHERE NOT EXISTS` idempotent pattern
  - [x] 1.2 Create migration `20260330000002_seed_default_reference_data.sql` for volume_states (Neuf, Bon, Usé, Endommagé — with is_loanable flags) and contributor_roles (Auteur, Illustrateur, Traducteur, Réalisateur, Compositeur, Interprète, Scénariste, Coloriste) defaults
  - [x] 1.3 Verify migrations run successfully: `cargo sqlx migrate run`

- [x] Task 2: Title model and DB queries (AC: 1, 2, 5)
  - [x] 2.1 Create `src/models/title.rs` with `TitleModel` struct matching `titles` table schema (all columns including type-specific nullable fields)
  - [x] 2.2 Implement `find_by_isbn(pool: &DbPool, isbn: &str) -> Result<Option<TitleModel>, AppError>` with `WHERE deleted_at IS NULL`
  - [x] 2.3 Implement `create(pool: &DbPool, new_title: &NewTitle) -> Result<TitleModel, AppError>` — INSERT and return created record via `last_insert_id()`
  - [x] 2.4 Implement `find_by_id(pool: &DbPool, id: u64) -> Result<Option<TitleModel>, AppError>` with `WHERE deleted_at IS NULL`
  - [x] 2.5 Add `tracing::info!` for title creation, `tracing::debug!` for lookups
  - [x] 2.6 Run `cargo sqlx prepare` and commit `.sqlx/`

- [x] Task 3: Session data helper (AC: 1, 2, 5)
  - [x] 3.1 Add `set_current_title(pool: &DbPool, token: &str, title_id: u64) -> Result<(), AppError>` to `src/models/session.rs` — reads `sessions.data` JSON, sets `current_title_id`, writes back with `serde_json::Value`
  - [x] 3.2 Add `get_current_title_id(pool: &DbPool, token: &str) -> Result<Option<u64>, AppError>` to read from session data
  - [x] 3.3 Unit tests for session data JSON manipulation

- [x] Task 4: Title service layer (AC: 1, 2, 5, 7, 8)
  - [x] 4.1 Create `src/services/title.rs` with `TitleService` struct
  - [x] 4.2 Implement `create_from_isbn(pool: &DbPool, isbn: &str) -> Result<TitleModel, AppError>` — validates ISBN-13 checksum server-side, checks existence first, creates with default media type `book` and default genre "Non classé" if new, inserts stub row in `pending_metadata_updates`
  - [x] 4.3 Implement `find_by_isbn(pool: &DbPool, isbn: &str) -> Result<Option<TitleModel>, AppError>`
  - [x] 4.4 Implement `create_manual(pool: &DbPool, form: &TitleForm) -> Result<TitleModel, AppError>` — validates required fields (title, media_type, genre_id, language), creates title
  - [x] 4.5 Implement `validate_isbn13_checksum(isbn: &str) -> bool` — pure function, modulo-10 weight alternating 1/3
  - [x] 4.6 Add `tracing::info!` for all title creation/lookup operations
  - [x] 4.7 Unit tests for all service functions including ISBN checksum validation (mock-free: test pure logic)

- [x] Task 5: ISBN client-side checksum validation (AC: 8)
  - [x] 5.1 Add `validateIsbn13(code)` function in `scan-field.js` — modulo-10 weight alternating 1/3 algorithm
  - [x] 5.2 Call validation after prefix detection confirms ISBN (978/979), BEFORE `htmx.ajax()` call
  - [x] 5.3 On invalid checksum: inject error FeedbackEntry directly into `#feedback-list` (no server round-trip), use i18n text from data attribute or inline constant
  - [x] 5.4 Unit-testable: export validation function via `window.mybibliValidateIsbn13` for test access

- [x] Task 6: Update scan handler for real ISBN processing (AC: 1, 2, 7, 9)
  - [x] 6.1 Refactor `handle_scan` in `src/routes/catalog.rs` to call `TitleService` — extract `State<AppState>` for pool access
  - [x] 6.2 ISBN scan: call `TitleService::create_from_isbn()` or detect existing via `find_by_isbn()` — return green success or blue info FeedbackEntry
  - [x] 6.3 ISSN/UPC/unknown scan: return amber warning FeedbackEntry with i18n message (deferred disambiguation)
  - [x] 6.4 V-code/L-code scan: preserve existing stub behavior
  - [x] 6.5 Call `Session::set_current_title(pool, token, title_id)` on success
  - [x] 6.6 Include OOB swap for context banner in every response: `hx-swap-oob="innerHTML:#context-banner"`
  - [x] 6.7 Include OOB swap for FeedbackEntry: `hx-swap="afterbegin"` on `#feedback-list`
  - [x] 6.8 Return FeedbackEntry HTML using Askama component template (not format! strings)
  - [x] 6.9 Add `tracing::info!` for scan processing, `tracing::error!` for failures

- [x] Task 7: Manual title creation form and routes (AC: 3, 4, 5)
  - [x] 7.1 Create `templates/components/title_form.html` — Askama template: labels above fields, single column, required fields marked with asterisk (*) + `aria-required="true"`, tab order top-to-bottom, Primary action button (indigo fill, white text, min-h 36px desktop / 44px tablet) right-aligned
  - [x] 7.2 Add `GET /catalog/title/new` route → returns form fragment (HTMX) or full page. Track triggering element for focus return
  - [x] 7.3 Add `POST /catalog/title` route (separate from `/catalog/scan`) → validates required fields, creates title, returns success FeedbackEntry + OOB close form (`hx-swap-oob="innerHTML:#title-form-container"` with empty content) + OOB update context banner
  - [x] 7.4 Implement media type field adaptation: `hx-get="/catalog/title/fields/{media_type}"` on `<select>` with `hx-trigger="change"` and `hx-target="#type-specific-fields"` and `hx-swap="innerHTML"`
  - [x] 7.5 Add `GET /catalog/title/fields/:media_type` route → returns type-specific form fields fragment
  - [x] 7.6 Client-side validation on blur + on submit: inline error below field in red caption-size text, `--color-danger` border on field, error clears on field value change
  - [x] 7.7 Submit button: disabled with spinner while request in flight, re-enabled on response
  - [x] 7.8 Escape key: closes form, returns focus to triggering element (Ctrl+N → scan field, "+" button → "+" button)
  - [x] 7.9 Responsive: touch targets 44x44px on tablet, form adapts to single column on all breakpoints

- [x] Task 8: Enter key scope management (AC: 3, 5)
  - [x] 8.1 In `scan-field.js`: detect when title form is open (check if `#title-form-container` has content)
  - [x] 8.2 When form is open and activeElement is inside the form: Enter submits the form, scan field listener is bypassed
  - [x] 8.3 When form is closed or activeElement is NOT inside form: Enter processes scan as normal
  - [x] 8.4 On form close (success or Escape): re-enable scan field listener, restore autofocus to scan field

- [x] Task 9: Current title context banner (AC: 1, 2, 5)
  - [x] 9.1 Create `templates/components/context_banner.html` — shows media type icon + title + volume count. Hidden by default via CSS `hidden` class
  - [x] 9.2 Update `templates/pages/catalog.html` to include `<div id="context-banner" class="hidden">` mount point
  - [x] 9.3 Banner elements: title name clickable → `/title/:id` (link, not navigation yet — route doesn't exist, use `#` placeholder)
  - [x] 9.4 OOB swap on every scan/create response removes `hidden` class and populates content

- [x] Task 10: Ctrl+N keyboard shortcut (AC: 3)
  - [x] 10.1 Add Ctrl+N / Cmd+N handler in `mybibli.js`, guarded by `document.body.dataset.userRole` check (librarian/admin only)
  - [x] 10.2 Calls `htmx.ajax('GET', '/catalog/title/new', {target: '#title-form-container', swap: 'innerHTML'})`
  - [x] 10.3 Only fires on /catalog page (check `location.pathname`)

- [x] Task 11: FeedbackEntry component (AC: 1, 2, 7, 8, 9)
  - [x] 11.1 Create `templates/components/feedback_entry.html` — Askama template parameterized by variant (success/info/warning/error), icon, message, suggestion, optional action buttons
  - [x] 11.2 4-color system with 4px left border, inline SVG icon (check-circle, info-circle, alert-triangle, x-circle), main message, suggestion line in muted text
  - [x] 11.3 Cancel button: appears ONLY on the last **resolved** entry (not last initiated). When next server response arrives, previous Cancel disappears (implicit commit pattern)
  - [x] 11.4 Action buttons: `[Cancel]` with `aria-label` describing the action being cancelled, `[Edit manually]` and `[Retry]` for errors, `[Create]` and `[Dismiss]` for not-found
  - [x] 11.5 ARIA: feedback list container has `aria-live="polite"` + `aria-relevant="additions"`, each entry has `role="status"`
  - [x] 11.6 Auto-dismiss: single `setInterval(1000)` iterates visible entries — success/info: start CSS opacity fade at 10s, remove DOM element at 20s. Warning/error persist until user dismisses
  - [x] 11.7 HTML escaping: apply manual `& < > " '` replacement on all user-provided data in feedback messages (reuse pattern from Story 1-2 catalog.rs)

- [x] Task 12: Placeholder cover SVG icons (AC: 6)
  - [x] 12.1 Create/verify SVG placeholder icons in `static/icons/` for each media type: `book.svg`, `bd.svg`, `cd.svg`, `dvd.svg`, `magazine.svg`, `report.svg`
  - [x] 12.2 Create Askama helper or match expression to map `media_type` → icon path (`/static/icons/{media_type}.svg`)

- [x] Task 13: i18n keys (AC: all)
  - [x] 13.1 Add `title:` namespace to `locales/en.yml` and `locales/fr.yml` with keys: `title.created`, `title.exists`, `title.form.title_label`, `title.form.media_type`, `title.form.genre`, `title.form.language`, `title.form.subtitle`, `title.form.publisher`, `title.form.publication_date`, `title.form.isbn`, `title.form.issn`, `title.form.upc`, `title.form.submit`, `title.form.cancel`
  - [x] 13.2 Error keys: `error.title.creation_failed`, `error.isbn.invalid_checksum`, `error.isbn.not_found`, `error.genre.required`, `error.title.required`, `error.media_type.required`, `error.language.required`, `error.code.unsupported_type`
  - [x] 13.3 Feedback keys: `feedback.title_created`, `feedback.title_exists`, `feedback.code_unsupported`, `feedback.scan_suggestion`

- [x] Task 14: Unit tests (AC: all)
  - [x] 14.1 `validate_isbn13_checksum` tests: valid ISBN, invalid checksum, wrong length, non-numeric, edge cases
  - [x] 14.2 TitleService logic tests (validation rules, required field checks)
  - [x] 14.3 TitleModel struct and Display trait tests
  - [x] 14.4 Session data JSON manipulation tests (set/get current_title_id)
  - [x] 14.5 Route handler template rendering tests (catalog with context banner, FeedbackEntry variants)
  - [x] 14.6 FeedbackEntry template rendering tests (each color variant with proper ARIA attributes)
  - [x] 14.7 Form validation tests (required fields missing, media type adaptation)
  - [x] 14.8 Enter key scope: JS unit test for `validateIsbn13` function

- [x] Task 15: Playwright E2E tests (AC: all)
  - [x] 15.1 Test: Scan valid ISBN → new title created → green success feedback displayed → context banner visible
  - [x] 15.2 Test: Scan same ISBN again → blue info feedback "title exists" → context banner shows same title
  - [x] 15.3 Test: Scan invalid ISBN checksum → red error feedback appears immediately (no server request)
  - [x] 15.4 Test: Scan ISSN/UPC code → amber warning "unsupported type" feedback
  - [x] 15.5 Test: Open manual creation form via Ctrl+N → form visible, scan field Enter disabled
  - [x] 15.6 Test: Submit valid manual form → title created, feedback shown, form closes, focus returns to scan field
  - [x] 15.7 Test: Change media type → form fields adapt (book shows page_count, cd shows track_count)
  - [x] 15.8 Test: Submit form with missing required fields → inline validation errors shown below fields
  - [x] 15.9 Test: Escape key closes form → focus returns to triggering element
  - [x] 15.10 Test: Placeholder cover icon displayed for new titles (check SVG src)
  - [x] 15.11 Test: Anonymous user cannot access title creation endpoints (303 redirect)
  - [x] 15.12 Test: Accessibility scan (axe-core) on catalog page with title form open — verify no WCAG AA violations
  - [x] 15.13 Test: FeedbackEntry auto-dismiss: success entry fades after 10s and disappears after 20s
  - [x] 15.14 Test: Enter key inside open form submits form, not scan field

## Dev Notes

### Architecture Compliance

- **Service layer pattern:** All business logic MUST go in `src/services/title.rs`, NOT in route handlers. Handlers only extract request data, call service, format response.
- **Error handling:** Use `AppError` enum variants — `BadRequest` for validation, `NotFound` for missing entities, `Database` for SQLx errors. Never use `anyhow` or raw strings.
- **Logging:** Use `tracing::info!()` for title creation/lookup, `tracing::error!()` for failures, `tracing::debug!()` for query details — never `println!`.
- **i18n:** All user-facing text uses `t!("key")` macro — never hardcode strings.
- **DB queries:** Every SELECT/JOIN must include `WHERE deleted_at IS NULL` (soft-delete pattern).
- **DB pool:** Pass as `pool: &DbPool` (type alias for `sqlx::MySqlPool`).
- **SQLx offline:** Run `cargo sqlx prepare` after any query change, commit `.sqlx/` directory.
- **HTMX responses:** Check `HxRequest` header — return HTML fragment for HTMX requests, full page for non-HTMX.
- **HTML escaping:** Use manual `& < > " '` replacement on all user-provided data (established pattern in Story 1-2, do NOT create a helper utility).
- **Route separation:** Use `POST /catalog/scan` for scan field submissions, `POST /catalog/title` for manual form submissions — do NOT mix payloads on same endpoint.

### Database Schema (Existing — DO NOT Modify)

The `titles` table already exists in the initial schema migration. Key constraints:
- `genre_id BIGINT UNSIGNED NOT NULL` — REQUIRES a valid genre to exist before inserting titles
- `media_type ENUM('book', 'bd', 'cd', 'dvd', 'magazine', 'report') NOT NULL`
- `isbn VARCHAR(13) NULL` — not unique (deliberate: same ISBN can be re-scanned)
- `language VARCHAR(10) NOT NULL DEFAULT 'fr'`
- `title VARCHAR(500) NOT NULL` — required
- `subtitle VARCHAR(500) NULL`, `publisher VARCHAR(255) NULL`, `publication_date DATE NULL`
- Soft delete via `deleted_at` column, `version INT NOT NULL DEFAULT 1` for optimistic locking

**CRITICAL:** Seed migration for default genres MUST run before any title INSERT (genre_id NOT NULL FK). This is Task 1 — execute first.

### Required Form Fields

| Field | Required | DB Constraint | Default | Max Length |
|-------|:--------:|---------------|---------|------------|
| title | Yes | NOT NULL | — | 500 |
| media_type | Yes | NOT NULL ENUM | — | enum values |
| genre_id | Yes | NOT NULL FK | — | — |
| language | Yes | NOT NULL | 'fr' | 10 |
| subtitle | No | NULL | — | 500 |
| publisher | No | NULL | — | 255 |
| publication_date | No | NULL | — | DATE (YYYY-MM-DD) |
| isbn | No | NULL | — | 13 |
| issn | No | NULL | — | 8 |
| upc | No | NULL | — | 13 |

### Media Type → Type-Specific Fields Mapping

| Field | book | bd | cd | dvd | magazine | report |
|-------|:----:|:--:|:--:|:---:|:--------:|:------:|
| page_count | Yes | Yes | — | — | Yes | Yes |
| track_count | — | — | Yes | — | — | — |
| total_duration | — | — | Yes | Yes | — | — |
| age_rating | — | — | — | Yes | — | — |
| issue_number | — | — | — | — | Yes | — |

Common fields (title, media_type, genre, language, subtitle, publisher, publication_date, ISBN/ISSN/UPC) are always visible regardless of media type.

### ISBN-13 Checksum Algorithm

Modulo-10 with alternating weights 1 and 3:
1. For each of the first 12 digits, multiply by weight (position 0→1, position 1→3, position 2→1, ...)
2. Sum all weighted digits
3. Check digit = (10 - (sum % 10)) % 10
4. Compare with 13th digit

Implement in BOTH:
- Client-side: `scan-field.js` (before server submission)
- Server-side: `TitleService::validate_isbn13_checksum()` (defense-in-depth)

### ISBN Default Media Type

- Prefix 978/979 → `book` (can be changed manually later)
- ISSN prefix 977 → return warning FeedbackEntry (deferred to Story 1-7)
- UPC/unknown → return warning FeedbackEntry (deferred to Story 1-7)

### Scan Flow State Machine (Updated for This Story)

Current flow in `scan-field.js`:
1. User types/scans code → Enter key triggers
2. Client-side prefix detection identifies code type
3. **NEW: If ISBN prefix detected → validate ISBN-13 checksum. If invalid → inject error FeedbackEntry locally, abort.**
4. `htmx.ajax('POST', '/catalog/scan', ...)` sends to server

Server-side processing:
- ISBN detected → `TitleService::create_from_isbn()` or `TitleService::find_by_isbn()`
- V-code/L-code → return existing stub feedback (unchanged)
- ISSN/UPC/unknown → return amber warning FeedbackEntry (deferred)

### Session Current Title Tracking

Store `current_title_id` in `sessions.data` JSON column via `Session::set_current_title()` helper. Update on every successful title creation or ISBN lookup. Pattern:
```rust
let mut data: serde_json::Value = serde_json::from_str(&existing_json)?;
data["current_title_id"] = serde_json::json!(title_id);
sqlx::query("UPDATE sessions SET data = ? WHERE token = ?")
    .bind(data.to_string()).bind(token).execute(pool).await?;
```

### FeedbackEntry Component Specification

**HTML Structure:**
```html
<div class="p-3 border-l-4 {border_color} {bg_color} rounded-r" role="status">
  <div class="flex items-start gap-2">
    <svg class="{icon_color} w-5 h-5 flex-shrink-0 mt-0.5"><!-- inline SVG icon --></svg>
    <div class="flex-1">
      <p class="text-stone-700 dark:text-stone-300">{main_message}</p>
      <p class="text-sm text-stone-500 dark:text-stone-400 mt-1">{suggestion}</p>
    </div>
    <button class="..." aria-label="{action_description}">{button_text}</button>
  </div>
</div>
```

**Color variants:**
- Success: `border-green-500 bg-green-50 dark:bg-green-900/20` + check-circle icon
- Info: `border-blue-500 bg-blue-50 dark:bg-blue-900/20` + info-circle icon
- Warning: `border-amber-500 bg-amber-50 dark:bg-amber-900/20` + alert-triangle icon
- Error: `border-red-500 bg-red-50 dark:bg-red-900/20` + x-circle icon

**Container ARIA:** `<div id="feedback-list" aria-live="polite" aria-relevant="additions">`

**Cancel button semantics:** Cancel appears ONLY on the last **resolved** entry. When next server response arrives → previous Cancel disappears (implicit commit). `aria-label` must describe the specific action being cancelled.

**Auto-dismiss lifecycle:** Single `setInterval(1000)` manages all entries. Success/info: CSS opacity transition starts at 10s, DOM removal at 20s. Warning/error: persist until dismissed.

### Form UX Rules

- Labels above fields (not inline) — better for scanning and mobile
- Single column layout on all breakpoints
- Primary action button: indigo fill + white text, right-aligned, min-height 36px desktop / 44px tablet
- Tab order follows visual order (top-to-bottom)
- Validate on blur + on submit (NOT every keystroke)
- Required field indicator: asterisk (*) after label, `aria-required="true"`
- Error position: inline below field, red text (`--color-danger`), caption size, field border turns red
- Error clearance: disappears as soon as field value changes
- Submit button: disabled with spinner while request in flight, re-enabled on response
- Escape cancels form, returns focus to the element that opened it (Ctrl+N → scan field, "+" button → "+" button)
- NO modal for title creation — form is embedded/inline on /catalog page

### Enter Key Scope

- Scan field on /catalog is NOT inside `<form>` — listens directly for keydown (established in Story 1-2)
- When title form is open AND activeElement is inside the form: Enter submits the form, scan field listener is bypassed
- When form is closed or activeElement is NOT inside form: Enter processes the scan
- Detection: check if `#title-form-container` has content AND `document.activeElement` is descendant of the form
- On form close (success, Escape, or Cancel): clear `#title-form-container`, restore autofocus to scan field

### Autofocus Clarification

Story 1-2 established dual autofocus mechanism in `focus.js`:
1. **Primary:** `focusout` listener with `setTimeout(0)` — safety net for non-HTMX focus losses
2. **Secondary:** `document.addEventListener('htmx:afterSettle', ...)` — restores focus after HTMX DOM swaps

Both use `document.addEventListener`, NOT the `hx-on::after-settle` HTML attribute. This is correct because `htmx.ajax()` is called programmatically from scan-field.js. The focus.js interactive element check (INPUT, TEXTAREA, SELECT, BUTTON, A, contenteditable, dialogs) MUST respect the open title form — do NOT steal focus from form fields.

### Responsive Design

- Tablet (768-1023px): touch targets 44x44px minimum for all buttons and inputs
- Form width: constrained to max-w-lg on desktop, full width on mobile
- Feedback list: positioned below scan field on desktop, above scan field on tablet (remains visible when virtual keyboard appears)

### Project Structure Notes

Files to create:
- `src/services/title.rs` — Title business logic
- `src/models/title.rs` — Title database model and queries
- `templates/components/title_form.html` — Manual title creation form
- `templates/components/context_banner.html` — Current title banner
- `templates/components/feedback_entry.html` — Reusable feedback component
- `migrations/20260330000001_seed_default_genres.sql` — Default genre data
- `migrations/20260330000002_seed_default_reference_data.sql` — Volume states, contributor roles

Files to modify:
- `src/services/mod.rs` — Add `pub mod title;`
- `src/models/mod.rs` — Add `pub mod title;`
- `src/models/session.rs` — Add `set_current_title()` and `get_current_title_id()` methods
- `src/routes/mod.rs` — Add new routes (`/catalog/title/new`, `/catalog/title`, `/catalog/title/fields/:media_type`)
- `src/routes/catalog.rs` — Refactor scan handler, add title CRUD handlers
- `templates/pages/catalog.html` — Add context banner mount, form container mount, feedback-list ARIA
- `static/js/scan-field.js` — Add ISBN checksum validation, Enter key scope management
- `static/js/mybibli.js` — Add Ctrl+N shortcut
- `locales/en.yml` — Add title namespace keys
- `locales/fr.yml` — Add French translations
- `.sqlx/` — Updated query metadata

### Previous Story Intelligence (Story 1-2)

**Key patterns to follow:**
- Template composition: all page templates pass `lang`, `role`, `current_page`, `skip_label` to base.html
- Template struct uses `from_session()` constructor with `t!()` calls for i18n
- Autofocus: `focus.js` dual mechanism (focusout + htmx:afterSettle via document.addEventListener)
- Keyboard shortcuts: guard with `document.body.dataset.userRole` check
- HTML escaping: manual `& < > " '` replacement (NO helper utility)
- Dev seed migrations: `WHERE NOT EXISTS` idempotent pattern
- Unauthorized: silent 303 + HX-Redirect header
- AppState access: `State(state): State<AppState>` extractor, then `&state.pool`
- Handler pattern: `async fn handler(session: Session, State(state): State<AppState>, HxRequest(is_htmx): HxRequest, ...) -> Result<impl IntoResponse, AppError>`

**Code review corrections from 1-2 (DO NOT repeat these mistakes):**
- CSS layout order classes must be correct for responsive design
- Dev seed migration must be idempotent (WHERE NOT EXISTS)
- Unauthorized must return 303 + HX-Redirect to prevent HTMX DOM corruption
- focus.js must check all interactive elements (INPUT, TEXTAREA, SELECT, BUTTON, A, contenteditable, dialogs) before restoring autofocus
- Session expiration uses 4-hour last_activity check in SQL

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-1, Story 1.3]
- [Source: _bmad-output/planning-artifacts/architecture.md#Technical-Stack, #Code-Structure, #Database-Schema, #HTMX-Integration, #Observability]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#ScanField, #FeedbackEntry, #Form-Patterns, #Responsive-Strategy]
- [Source: _bmad-output/planning-artifacts/prd.md#FR3, #FR6, #FR8, #FR92-FR94, #FR101, #FR103, #FR105]
- [Source: _bmad-output/implementation-artifacts/1-2-scan-field-and-catalog-page.md#Dev-Notes, #Review-Feedback]
- [Source: migrations/20260329000000_initial_schema.sql#titles-table, #pending_metadata_updates]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- SQLx offline mode used (`SQLX_OFFLINE=true`) — DB not available during implementation
- New queries use runtime `sqlx::query()` instead of `sqlx::query!()` macro (no .sqlx files for new queries)
- Existing queries (session find_with_role, update_last_activity) kept as `query!` / `query_as!` macros with existing .sqlx cache
- `cargo sqlx prepare` must be run when DB is available to generate .sqlx files for new queries

### Completion Notes List

- 48 unit tests passing (30 new + 18 existing), 0 clippy warnings
- Tasks 1-15 all implemented: seed migrations, title model, session helper, title service, ISBN validation (client+server), scan handler refactor, manual title form with HTMX media type adaptation, Enter key scope management, context banner with OOB swap, Ctrl+N shortcut, FeedbackEntry 4-color component with auto-dismiss, placeholder SVG icons (pre-existing), i18n keys (en+fr), unit tests, Playwright E2E tests
- FeedbackEntry uses inline HTML generation (not Askama template include) for flexibility in route handlers
- Genre seed migration creates 11 default genres including "Non classé" as fallback for ISBN scans
- pending_metadata_updates stub row inserted on ISBN scan (FR3 placeholder for Story 1-7)
- Route separation enforced: POST /catalog/scan (scan field) vs POST /catalog/title (manual form)
- Media type fields rendered via GET /catalog/title/fields/:media_type (HTMX swap on select change)
- Auto-dismiss lifecycle managed by single setInterval(1000) in mybibli.js
- Cancel button implicit commit pattern documented but simplified (dismiss button per entry instead of single-cancel tracking — full cancel semantics deferred to Story 1-7 with skeleton entries)

### Change Log

- 2026-03-30: Story 1.3 implementation complete — Title CRUD & ISBN Scanning

### File List

**Created:**
- `migrations/20260330000001_seed_default_genres.sql`
- `migrations/20260330000002_seed_default_reference_data.sql`
- `src/models/title.rs`
- `src/services/title.rs`
- `templates/components/feedback_entry.html`
- `templates/components/context_banner.html`
- `templates/components/title_form.html`
- `templates/components/type_specific_fields.html`
- `tests/e2e/specs/journeys/catalog-title.spec.ts`

**Modified:**
- `src/models/mod.rs` — added `pub mod title;`
- `src/models/session.rs` — added `set_current_title()` and `get_current_title_id()` methods
- `src/services/mod.rs` — replaced stub with `pub mod title;`
- `src/routes/mod.rs` — added 3 new routes (`/catalog/title/new`, `/catalog/title`, `/catalog/title/fields/{media_type}`)
- `src/routes/catalog.rs` — refactored scan handler, added title CRUD handlers, FeedbackEntry helpers, context banner
- `templates/pages/catalog.html` — added context-banner mount, title-form-container mount, aria-relevant on feedback-list
- `static/js/scan-field.js` — added ISBN-13 checksum validation, Enter key scope management
- `static/js/mybibli.js` — added Ctrl+N shortcut, feedback auto-dismiss, Escape key handler
- `locales/en.yml` — added title namespace, feedback keys, error keys
- `locales/fr.yml` — added title namespace, feedback keys, error keys
