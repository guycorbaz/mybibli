# Story 1.8: Cross-Cutting Patterns

Status: done

## Story

As a developer,
I want the application to implement soft-delete enforcement, optimistic locking, dark/light theme toggle, session expiry with inactivity timeout, and a complete navigation bar,
so that all entity operations follow consistent patterns and the UI is production-ready.

## Acceptance Criteria (BDD)

### AC1: Soft Delete - Entity Visibility

**Given** a user deletes any entity (title, volume, contributor),
**When** the delete is processed,
**Then** the entity's `deleted_at` is set (soft-delete) and it becomes invisible in all normal views but remains in the database.

### AC2: Soft Delete - Referential Integrity (Deferred to Epic 8)

**Given** a soft-deleted entity is referenced by active entities,
**When** an admin tries to permanently delete it from Trash,
**Then** the system prevents permanent deletion with an error listing the referencing entities (FR80).

> **NOTE:** This AC is about permanent deletion from Trash (Epic 8 scope). In story 1-8, only the soft-delete action itself is implemented. The referential integrity check will be implemented when the Trash/purge UI is built in Epic 8.

### AC3: Cascade Delete Prevention

**Given** a title has its last volume deleted,
**When** the delete is processed,
**Then** the title itself is preserved (not cascade deleted) per FR81.

### AC4: Optimistic Locking - Conflict Detection

**Given** two users edit the same title simultaneously,
**When** the second user submits their changes,
**Then** the system detects the version mismatch and returns a Conflict error with a "Reload" action (FR82).

### AC5: Theme - System Preference Detection

**Given** a user's browser theme preference is "dark",
**When** they first visit the application,
**Then** dark mode is applied automatically via `prefers-color-scheme` detection (FR79).

### AC6: Theme Toggle - Persistence

**Given** a user clicks the theme toggle,
**When** the toggle is clicked,
**Then** the theme switches between light and dark mode and the preference is persisted in localStorage (FR78).

### AC7: Session Expiry - Browser Close

**Given** a librarian is authenticated,
**When** they close the browser and reopen it,
**Then** their session is expired (session cookie with no max-age) and they must re-authenticate (FR69).

### AC8: Session Token Security

**Given** a user authenticates,
**When** the session is created,
**Then** the session token is cryptographically random (256-bit), stored as HttpOnly SameSite=Strict cookie (NFR9, NFR10).

### AC9: Navigation Bar - Verification & Enhancement

**Given** the navigation bar is rendered (`templates/components/nav_bar.html` already exists),
**When** the user views any page,
**Then** it shows links to Home, Catalog (if Librarian/Admin), Loans (if Librarian/Admin), Admin (if Admin), theme toggle, with the current page highlighted. Mobile hamburger toggle already stubbed.

### AC10: Soft Delete - Query Convention

**Given** all queries in the codebase,
**When** they select from entity tables,
**Then** they include `deleted_at IS NULL` on every table in JOINs (this is already enforced — verify no gaps).

## Explicit Scope Boundaries

**In scope:**
- Soft delete service for titles, volumes, contributors (soft-delete endpoints)
- Optimistic locking enforcement on UPDATE queries (version check)
- `AppError::Conflict` variant for version mismatch
- Theme toggle verification — `static/js/theme.js` already exists with `prefers-color-scheme` detection and localStorage. Verify and enhance if needed.
- Navigation bar verification — `templates/components/nav_bar.html` already exists with role-based links, theme toggle, hamburger stub. Verify dimensions, a11y, and enhance if needed.
- Session cookie: HttpOnly, SameSite=Strict, no max-age (expires on browser close)
- Session inactivity timeout verification — `SessionModel::find_with_role()` already has `AND s.last_activity > DATE_SUB(NOW(), INTERVAL 4 HOUR)` hardcoded. Verify this works; parameterization deferred.
- Inactivity Toast warning 5 minutes before expiry (client-side JS timer — NEW)
- i18n keys for all new user-facing text

**NOT in scope (deferred to later epics):**
- Admin Trash view (Epic 8)
- Permanent purge from Trash (Epic 8)
- Login/logout UI (Epic 7) — dev seed session is sufficient for Epic 1
- Language toggle (Epic 8)
- Series link, Locations link, Borrowers link (future epics — Loans and Admin links already exist and stay)
- Mobile hamburger menu full slide-out drawer (current toggle stub is sufficient)
- Keyboard shortcut Ctrl+L for /loans (future epic)

## Tasks / Subtasks

- [x] Task 1: AppError::Conflict variant for optimistic locking (AC: 4)
  - [x] 1.1 Added `Conflict(String)` variant to `AppError` in `src/error/mod.rs` — returns HTTP 409
  - [x] 1.2 Implemented `IntoResponse` for Conflict: 409 status with client message
  - [x] 1.3 Unit tests: 6 tests for all AppError variants (Conflict display, status codes)

- [x] Task 2: Optimistic locking enforcement (AC: 4)
  - [x] 2.1 Created `src/services/locking.rs` with `check_update_result()` — returns `AppError::Conflict` if 0 rows
  - [x] 2.2 Added `TitleModel::update_with_locking()` in `src/models/title.rs` with version check
  - [x] 2.3 Added i18n keys: `error.conflict` in EN and FR
  - [x] 2.4 Unit tests: locking service (success, multiple rows, conflict)

- [x] Task 3: Soft delete service (AC: 1, 3, 10)
  - [x] 3.1 Created `src/services/soft_delete.rs` with `SoftDeleteService::soft_delete()` + table whitelist
  - [x] 3.2 Table whitelist: titles, volumes, contributors, storage_locations, borrowers, series
  - [x] 3.3 Added delete endpoints: `DELETE /catalog/title/{id}`, `DELETE /catalog/volume/{id}`
  - [x] 3.4 Audited all existing queries for `deleted_at IS NULL` — all compliant
  - [x] 3.5 Added i18n keys: `feedback.deleted`, `error.delete_has_references`
  - [x] 3.6 Unit tests: whitelist validation, injection prevention

- [x] Task 4: Theme toggle — Verified & enhanced (AC: 5, 6)
  - [x] 4.1 VERIFIED existing theme.js — prefers-color-scheme, localStorage, toggle all working
  - [x] 4.2 VERIFIED system preference change listener present
  - [x] 4.3 FIXED: Added 300ms transition on toggle with prefers-reduced-motion guard
  - [x] 4.4 VERIFIED Tailwind dark: variants work with class-based dark mode
  - [x] 4.5 E2E tests created for theme toggle

- [x] Task 5: Navigation bar — Verified & enhanced (AC: 9)
  - [x] 5.1 VERIFIED existing nav_bar.html with role-based links, theme toggle, hamburger
  - [x] 5.2 VERIFIED current page indicator (aria-current + border)
  - [x] 5.3 VERIFIED dimensions: h-12 desktop, md:h-14 tablet. Relative position kept (acceptable for MVP)
  - [x] 5.4 VERIFIED skip link in base.html
  - [x] 5.5 FIXED: Dynamic aria-label via theme.js, added mobile nav aria-current="page", added i18n keys
  - [x] 5.6 VERIFIED responsive hamburger toggle

- [x] Task 6: Session expiry and inactivity timeout (AC: 7, 8)
  - [x] 6.1 VERIFIED session cookie via dev seed migration
  - [x] 6.2 VERIFIED inactivity timeout in SessionModel::find_with_role() (hardcoded 4h)
  - [x] 6.3 Created `static/js/session-timeout.js` with Toast warning 5min before expiry
  - [x] 6.4 Added `POST /session/keepalive` route
  - [x] 6.5 Added i18n keys: session.expiry_warning, session.stay_connected
  - [x] 6.6 Keepalive tested via E2E

- [x] Task 7: Unit tests (AC: all)
  - [x] 7.1 AppError::Conflict: Display + IntoResponse (409)
  - [x] 7.2 Optimistic locking: check_update_result success/conflict
  - [x] 7.3 Soft delete: whitelist validation, injection prevention
  - [x] 7.4 TitleModel::update_with_locking uses check_update_result

- [x] Task 8: Playwright E2E tests (AC: all)
  - [x] 8.1 Test: Delete volume → disappears
  - [x] 8.2 (Deferred: requires two concurrent sessions — E2E limitation)
  - [x] 8.3 Test: Theme toggle → dark mode class, persists after reload
  - [x] 8.4 (Covered by theme toggle test with initial state detection)
  - [x] 8.5 Test: Nav bar Catalog link visible for librarian, hidden for anonymous
  - [x] 8.6 Test: Current page highlighted + accessible aria-label on theme toggle

### Review Findings (Pass 1 — 2026-03-31)

- [x] [Review][Patch] **i18n in session-timeout.js** — reads `<html lang>` and uses embedded FR/EN string map ✅
- [x] [Review][Patch] **delete_title checks child volumes** — returns warning if active volumes exist ✅
- [x] [Review][Patch] **Session keepalive returns 401 for anonymous** ✅
- [x] [Review][Patch] **Toast dismiss uses hideWarning()** — proper toastEl reset ✅
- [x] [Review][Defer] Conflict error conflates version mismatch with entity deletion — acceptable for MVP, both mean "can't update"
- [x] [Review][Defer] No HTTP endpoint calls update_with_locking — title edit form doesn't exist yet; infrastructure ready for future story
- [x] [Review][Defer] Timer drifts from server if failed requests reset JS timer — low impact single-user NAS
- [x] [Review][Defer] Theme aria-label selector is fragile (matches onclick attribute content) — works now
- [x] [Review][Defer] Soft-delete already-deleted entity returns 404 instead of idempotent 200 — acceptable REST semantics
- [x] [Review][Defer] htmx might not be loaded when keepAlive called — fetch() fallback works
- [x] [Review][Defer] resetTimer on every htmx:afterRequest without debounce — timer reset is cheap

## Dev Notes

### Architecture Compliance

- **Service layer:** Soft delete logic in `src/services/soft_delete.rs`, locking in `src/services/locking.rs` — NOT in route handlers
- **Error handling:** `AppError` enum — add `Conflict` variant for optimistic locking. No `anyhow` or raw strings
- **Logging:** `tracing` macros only — no `println!`
- **i18n:** `t!("key")` for all user-facing text — never hardcode strings
- **DB queries:** `WHERE deleted_at IS NULL` in every query/JOIN — audit all existing queries
- **HTMX:** Check `HxRequest` header. Conflict error should return feedback HTML for HTMX, redirect for non-HTMX
- **Theme:** Client-side only. No server-side theme state. Use `<html class="dark">` with Tailwind `dark:` variants
- **Pool access:** `pool: &DbPool` from `AppState`. For spawned tasks: `pool.clone()`

### Database Schema Notes

**Existing columns already in all entity tables:**
- `deleted_at TIMESTAMP NULL DEFAULT NULL` — soft delete (already exists)
- `version INT NOT NULL DEFAULT 1` — optimistic locking (already exists)
- `created_at`, `updated_at` — timestamps (already exist)

**No new migrations needed** — all columns are already in the initial schema.

### Existing Patterns to Follow

From stories 1-1 through 1-7:
- `feedback_html(variant, message, suggestion)` in `src/routes/catalog.rs` for feedback entries
- `HtmxResponse { main, oob }` with `OobUpdate` for OOB swaps in `src/middleware/htmx.rs`
- `html_escape()` in `src/utils.rs` — shared utility, DO NOT duplicate
- `t!()` for ALL user-facing strings with `%{variable}` interpolation
- Askama templates extend `layouts/base.html` with `{% block content %}`
- Session cookie name is `"session"` (NOT "session_token") — verified in auth middleware
- AppSettings accessed via `state.settings.read().unwrap().field_name`

### Key File Locations

**Files to create:**
- `src/services/soft_delete.rs` — soft delete service
- `src/services/locking.rs` — optimistic locking helpers
- `static/js/session-timeout.js` — inactivity warning Toast (NEW)
- `tests/e2e/specs/journeys/cross-cutting.spec.ts` — E2E tests

**Files already existing (verify & enhance only):**
- `static/js/theme.js` — already has prefers-color-scheme, localStorage, toggle. DO NOT recreate.
- `templates/components/nav_bar.html` — already has role-based links, theme toggle, hamburger stub. DO NOT recreate.

**Files to modify:**
- `src/error/mod.rs` — add `Conflict(String)` variant to AppError
- `src/services/mod.rs` — add `pub mod soft_delete;`, `pub mod locking;`
- `src/models/title.rs` — add `update_with_locking()` method
- `src/routes/mod.rs` — add delete routes, keepalive route
- `src/routes/catalog.rs` — add delete handlers for title/volume
- `templates/layouts/base.html` — add `data-session-timeout` attribute, verify nav bar include, verify theme.js
- `locales/en.yml` — add conflict, delete, theme, session i18n keys
- `locales/fr.yml` — add French translations

### Previous Story Learnings (from 1-7 review)

- **Cookie name:** The session cookie is named `"session"`, not `"session_token"`. Verified and fixed in story 1-7 review.
- **DRY:** `html_escape()` was duplicated between catalog.rs and pending_updates.rs — fixed in review. Use `crate::utils::html_escape` everywhere.
- **AppError compliance:** All error returns must use `AppError` variants — raw `String` errors were caught and fixed in review.
- **Settings validation:** `AppSettings` fields have validation (timeout >= 1s). The `session_timeout_secs` is already loaded and validated.
- **Body size limits:** Any middleware that reads response bodies should use bounded reads (10 MB limit pattern from pending_updates).

### Deferred Work Items Relevant to This Story

From deferred-work.md:
- "Soft-delete not enforced at FK level — by design, all queries must include `deleted_at IS NULL`" → This story enforces the pattern
- "Hardcoded French role name 'Auteur' in SQL queries" → NOT this story's scope, but be aware
- "Raw error strings in template rendering" → Pre-existing pattern, consider fixing if touching those files

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-1, Story 1.8]
- [Source: _bmad-output/planning-artifacts/prd.md#FR69, #FR78, #FR79, #FR80, #FR81, #FR82, #FR86, #FR109, #NFR9, #NFR10, #NFR12, #NFR22, #NFR31]
- [Source: _bmad-output/planning-artifacts/architecture.md#Soft-Delete, #Optimistic-Locking, #Theme, #Session-Lifecycle, #NavigationBar]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#UX-DR6, #UX-DR25, #UX-DR29, #NavigationBar]
- [Source: _bmad-output/implementation-artifacts/1-7-scan-feedback-and-async-metadata.md#Dev-Agent-Record]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- Theme.js and nav_bar.html already existed from previous stories — verified and enhanced, not recreated
- Session timeout already enforced in SQL query (hardcoded 4 HOUR) — parameterization deferred

### Completion Notes List

- **Task 1:** AppError::Conflict variant (HTTP 409) with 6 unit tests for all error variants
- **Task 2:** Optimistic locking: `check_update_result()` service + `TitleModel::update_with_locking()` with version check
- **Task 3:** SoftDeleteService with table whitelist, delete endpoints for title/volume, query audit passed
- **Task 4:** theme.js verified complete; added 300ms transition with prefers-reduced-motion guard
- **Task 5:** nav_bar.html verified; fixed mobile nav aria-current, dynamic aria-label via JS
- **Task 6:** Session timeout verified in SQL; created session-timeout.js Toast + keepalive route
- **Task 7:** 155 unit tests pass (12 new: error, locking, soft_delete)
- **Task 8:** E2E tests for theme toggle, nav bar visibility, current page indicator

### Change Log

- 2026-03-31: Implemented story 1-8: Cross-Cutting Patterns — all 8 tasks complete

### File List

**New files:**
- `src/services/locking.rs`
- `src/services/soft_delete.rs`
- `static/js/session-timeout.js`
- `tests/e2e/specs/journeys/cross-cutting.spec.ts`

**Modified files:**
- `src/error/mod.rs` — added Conflict variant + 6 unit tests
- `src/services/mod.rs` — added pub mod locking, soft_delete
- `src/models/title.rs` — added update_with_locking()
- `src/routes/catalog.rs` — added delete_title, delete_volume, session_keepalive handlers
- `src/routes/mod.rs` — registered delete routes + keepalive route
- `static/js/theme.js` — added 300ms transition + dynamic aria-label
- `templates/components/nav_bar.html` — added mobile nav aria-current="page"
- `templates/layouts/base.html` — added data-session-timeout attribute, session-timeout.js script
- `locales/en.yml` — added conflict, deleted, delete_has_references, theme_toggle, session keys
- `locales/fr.yml` — added French translations
