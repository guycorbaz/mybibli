# Story 1.5: Contributor Management

Status: done

## Story

As a librarian,
I want to manage contributors (authors, illustrators, etc.) and associate them with titles via roles,
so that I can find titles by contributor and maintain accurate bibliographic data.

## Acceptance Criteria (BDD)

### AC1: Add Contributor to Title via Autocomplete

**Given** I am on the catalog page with a current title set,
**When** I open the "Add contributor" inline form (via button on context banner area),
**Then** I can search for an existing contributor by name (autocomplete with 300ms debounce, min 2 chars, `role="combobox"` + `role="listbox"` dropdown with `aria-live="polite"`) or create a new one inline by typing a name that doesn't match. Arrow keys navigate the dropdown, Enter selects, Escape closes.

### AC2: Assign Contributor Role

**Given** I add a contributor to a title,
**When** I select a role from the contributor_roles dropdown (e.g., Auteur, Illustrateur, Traducteur),
**Then** a `title_contributors` junction record is created linking the title, contributor, and role. A success FeedbackEntry appears ("*{contributor_name}* added as *{role}* to *{title}*.").

### AC3: Reject Duplicate Contributor-Role Assignment

**Given** a contributor is already associated with a title in a specific role,
**When** I try to add the same contributor with the same role again,
**Then** the system rejects the duplicate with an error FeedbackEntry ("*{contributor_name}* is already *{role}* on this title.").

### AC4: Allow Multiple Roles per Contributor

**Given** a contributor is associated with a title in one role (e.g., Auteur),
**When** I add the same contributor with a different role (e.g., Traducteur),
**Then** the system accepts it, creating a second junction record. The contributor list shows both roles: "Clint Eastwood (réalisateur, acteur)".

### AC5: Prevent Deletion of Referenced Contributor

**Given** a contributor is referenced by at least one title,
**When** I try to delete that contributor,
**Then** the system prevents deletion with an error FeedbackEntry ("Cannot delete: *{contributor_name}* is associated with *{count}* title(s).").

### AC6: Edit Contributor Details (FR97)

**Given** I am viewing a contributor in the contributor form,
**When** I modify the contributor's name (required, non-empty, trimmed, max 255 chars) or biography (optional, plain text) and submit,
**Then** the changes are saved and a success FeedbackEntry appears. Whitespace-only names are rejected with a validation error.

### AC7: Contributor List Display on Context Banner

**Given** a title has contributors,
**When** the context banner is displayed after adding a contributor,
**Then** the primary contributor (first author) is shown in the banner: "Current: *{title}* — *{author}* — *{vol_count}* vol".

### AC8: Remove Contributor from Title

**Given** a contributor is associated with a title,
**When** I click the remove button next to the contributor-role assignment,
**Then** the junction record is soft-deleted and the contributor list updates. If the contributor has no other title associations, they remain in the system (not auto-deleted).

## Explicit Scope Boundaries

**Deferred to Story 1-6 (Search & Browsing):**
- Contributor detail page (`/contributor/:id`) with all titles by that contributor
- Cross-navigation from title → contributor → other titles
- Search by contributor name in global search

**Deferred to Epic 7 (Administration):**
- Contributor role CRUD (admin can add/edit/delete roles via FR70-FR76)
- Role deletion protection ("Cannot delete: 28 assignments use this role")

**Deferred to Story 1-7 (Async Metadata):**
- Auto-populating contributors from metadata API responses

**Deferred to Story 1-8 (Cross-cutting):**
- Contributor soft-delete with trash management
- Optimistic locking conflict handling on contributor edits

**NOT in scope:**
- Title detail page (doesn't exist yet — contributor management happens via catalog toolbar/form)
- Contributor biography rich text editing (plain text only)
- Contributor photo/avatar

## Tasks / Subtasks

- [x] Task 1: Contributor model and DB queries (AC: 1, 2, 3, 5, 6, 8)
  - [x] 1.1 Create `src/models/contributor.rs` with `ContributorModel` struct (id, name, biography, version)
  - [x] 1.2 Implement `find_by_id(pool, id) -> Result<Option<ContributorModel>>` with `WHERE deleted_at IS NULL`
  - [x] 1.3 Implement `search_by_name(pool, query, limit) -> Result<Vec<ContributorModel>>` — `WHERE name LIKE ? AND deleted_at IS NULL ORDER BY name LIMIT ?` for autocomplete
  - [x] 1.4 Implement `create(pool, name, biography) -> Result<ContributorModel>` — INSERT and return
  - [x] 1.5 Implement `update(pool, id, name, biography) -> Result<()>` — UPDATE with soft-delete check
  - [x] 1.6 Implement `count_title_associations(pool, id) -> Result<u64>` — count active title_contributors for deletion guard
  - [x] 1.7 Implement `delete(pool, id) -> Result<()>` — soft delete (SET deleted_at = NOW()) only if count_title_associations == 0
  - [x] 1.8 Add `pub mod contributor;` to `src/models/mod.rs`

- [x] Task 2: Title-contributor junction queries (AC: 2, 3, 4, 7, 8)
  - [x] 2.1 Create `TitleContributorModel` struct in `src/models/contributor.rs` (id, title_id, contributor_id, role_id, contributor_name, role_name)
  - [x] 2.2 Implement `find_by_title(pool, title_id) -> Result<Vec<TitleContributorModel>>` — JOIN contributors + contributor_roles, WHERE deleted_at IS NULL on all tables
  - [x] 2.3 Implement `add_to_title(pool, title_id, contributor_id, role_id) -> Result<()>` — INSERT with UNIQUE constraint handling (duplicate → user-friendly error)
  - [x] 2.4 Implement `remove_from_title(pool, id) -> Result<()>` — soft delete junction record
  - [x] 2.5 Implement `get_primary_contributor(pool, title_id) -> Result<Option<String>>` — return first contributor name (for banner display)

- [x] Task 3: Contributor service layer (AC: 1, 2, 3, 4, 5, 6, 8)
  - [x] 3.1 Create `src/services/contributor.rs` with `ContributorService` struct
  - [x] 3.2 Implement `find_or_create(pool, name) -> Result<ContributorModel>` — search exact match first, create if not found
  - [x] 3.3 Implement `add_to_title(pool, title_id, contributor_name, role_id) -> Result<TitleContributorModel>` — find_or_create contributor, then add junction, handle UNIQUE constraint
  - [x] 3.4 Implement `remove_from_title(pool, junction_id) -> Result<()>`
  - [x] 3.5 Implement `update_details(pool, id, name, biography) -> Result<()>` — validate non-empty name
  - [x] 3.6 Implement `delete_contributor(pool, id) -> Result<()>` — check associations, prevent if referenced
  - [x] 3.7 Unit tests: find_or_create logic, validation, deletion guard
  - [x] 3.8 Add `pub mod contributor;` to `src/services/mod.rs`

- [x] Task 4: Contributor routes and templates (AC: 1, 2, 6, 7, 8)
  - [x] 4.1 Add `GET /catalog/contributors/search?q=` route — returns JSON array of matching contributors (for autocomplete)
  - [x] 4.2 Add `POST /catalog/contributors/add` route — accepts title_id, contributor_name, role_id; returns FeedbackEntry + OOB contributor list update
  - [x] 4.3 Add `POST /catalog/contributors/remove` route — accepts junction_id; returns FeedbackEntry + OOB contributor list update
  - [x] 4.4 Add `POST /catalog/contributors/update` route — accepts id, name, biography; returns FeedbackEntry
  - [x] 4.5 Add `DELETE /catalog/contributors/:id` route — soft delete contributor (with deletion guard)
  - [x] 4.6 Create `templates/components/contributor_form.html` — inline form with name autocomplete input + role dropdown + submit button
  - [x] 4.7 Create `templates/components/contributor_list.html` — displays contributors with roles and remove buttons, used as OOB swap target
  - [x] 4.8 Add contributor form mount point and list display to catalog page

- [x] Task 5: Autocomplete JavaScript (AC: 1)
  - [x] 5.1 Add autocomplete behavior in `static/js/contributor.js` — debounced fetch to `/catalog/contributors/search?q=`, display dropdown with matching names
  - [x] 5.2 On selecting a match: populate hidden contributor_id field
  - [x] 5.3 On typing a new name (no match selected): leave contributor_id empty (service will create new)
  - [x] 5.4 Close autocomplete dropdown on Escape, click outside, or selection

- [x] Task 6: Update context banner with primary contributor (AC: 7)
  - [x] 6.1 Update `context_banner_html` to accept optional `author: Option<&str>` parameter
  - [x] 6.2 Query primary contributor via `get_primary_contributor(pool, title_id)` on every banner OOB swap
  - [x] 6.3 Banner format: "Current: {title} — {author} — {vol_count} vol" (author omitted if no contributors)
  - [x] 6.4 Update i18n key: `title.current_banner_with_author`

- [x] Task 7: i18n keys (AC: all)
  - [x] 7.1 Add contributor keys to en.yml and fr.yml: `contributor.added`, `contributor.duplicate`, `contributor.updated`, `contributor.deleted`, `contributor.delete_blocked`, `contributor.removed`, `contributor.form.name`, `contributor.form.role`, `contributor.form.biography`, `contributor.form.submit`, `contributor.form.add_button`
  - [x] 7.2 Update banner key: `title.current_banner_with_author`

- [x] Task 8: Unit tests (AC: all)
  - [x] 8.1 ContributorModel: Display trait, struct construction
  - [x] 8.2 TitleContributorModel: struct construction, junction relationships
  - [x] 8.3 ContributorService: validation (empty name rejected), deletion guard logic
  - [x] 8.4 Route tests: contributor search response format, add/remove feedback HTML
  - [x] 8.5 Context banner with author rendering

- [x] Task 9: Playwright E2E tests (AC: all)
  - [x] 9.1 Test: Open contributor form, search existing name → autocomplete shows matches
  - [x] 9.2 Test: Add new contributor with role → success feedback, contributor list updates
  - [x] 9.3 Test: Add same contributor+role again → error "already assigned"
  - [x] 9.4 Test: Add same contributor with different role → success, both roles shown
  - [x] 9.5 Test: Remove contributor from title → contributor list updates
  - [x] 9.6 Test: Edit contributor name → success feedback
  - [x] 9.7 Test: Delete contributor with associations → error "cannot delete"
  - [x] 9.8 Test: Context banner shows primary author after adding contributor
  - [x] 9.9 Test: Anonymous user cannot access contributor endpoints (303 redirect)
  - [x] 9.10 Test: Accessibility scan (axe-core) on catalog with contributor form

## Dev Notes

### Architecture Compliance

- **Service layer:** Business logic in `src/services/contributor.rs`, NOT in route handlers
- **Error handling:** `AppError` enum — `BadRequest` for validation, `NotFound` for missing, `Database` for SQLx
- **Logging:** `tracing::info!` for creation/update/delete, `tracing::debug!` for lookups
- **i18n:** All user-facing text via `t!("key")` — never hardcode strings
- **DB queries:** `WHERE deleted_at IS NULL` on ALL tables in JOINs (contributors, title_contributors, contributor_roles)
- **HTMX:** Fragment for HTMX requests, full page for non-HTMX
- **HTML escaping:** Manual `& < > " '` on all user data

### Database Schema (Existing — DO NOT Modify)

**contributors:** id, name (VARCHAR 255 NOT NULL), biography (TEXT NULL), soft delete + version
**contributor_roles:** id, name (VARCHAR 255 NOT NULL UNIQUE), soft delete + version — 8 roles seeded (Auteur, Illustrateur, etc.)
**title_contributors:** id, title_id FK, contributor_id FK, role_id FK, UNIQUE(title_id, contributor_id, role_id), soft delete + version

**CRITICAL:** The UNIQUE constraint on `(title_id, contributor_id, role_id)` in `title_contributors` handles AC3 (duplicate rejection). Handle the DB constraint error gracefully with a user-friendly message.

### Contributor Display Format

Full variant: `Albert Camus (auteur) · Jean Mineur (illustrateur)`
- Names are `<a>` links (href="#" placeholder — contributor detail page deferred to 1-6)
- Separator `·` with `aria-hidden="true"`
- Multiple roles same person: "Clint Eastwood (réalisateur, acteur)" — group by contributor, list roles comma-separated
- Each name link: `aria-label="{Name}, {role}"`

### Autocomplete Pattern

- Debounce: 300ms after last keystroke
- Endpoint: `GET /catalog/contributors/search?q={query}` returns JSON `[{id, name}]`
- Min chars: 2 before triggering search
- Display: dropdown below input with matching names
- Selection: click or Enter selects, populates hidden field
- New contributor: if user submits without selecting from dropdown, service creates new contributor

### Context Banner Update

Extend `context_banner_html` signature to add `author: Option<&str>`:
```rust
fn context_banner_html(title_name: &str, media_type: &str, volume_count: u64, author: Option<&str>) -> String
```
Format: "Current: {title} — {author} — {vol_count} vol" (author omitted if None).

**Primary author selection query:**
```sql
SELECT c.name FROM title_contributors tc
JOIN contributors c ON tc.contributor_id = c.id
JOIN contributor_roles cr ON tc.role_id = cr.id
WHERE tc.title_id = ? AND tc.deleted_at IS NULL AND c.deleted_at IS NULL
ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC
LIMIT 1
```
This prioritizes "Auteur" role, then falls back to first contributor by creation order.

### Duplicate Detection Pattern

Same pattern as `volume.rs`: catch `sqlx::Error`, check `err_str.contains("Duplicate entry")`, map to `AppError::BadRequest` with user-friendly i18n message. The UNIQUE constraint `uq_title_contributor_role(title_id, contributor_id, role_id)` handles AC3.

### Validation Rules

- Contributor name: required, trimmed, non-empty after trim, max 255 chars
- Role ID: must exist in `contributor_roles` WHERE `deleted_at IS NULL` — validate before INSERT
- Title ID: must exist in `titles` WHERE `deleted_at IS NULL` — validate before INSERT (use existing `TitleModel::find_by_id`)
- Biography: optional, plain text, no length limit (TEXT column)

### Route Organization

All contributor routes go in `src/routes/catalog.rs` (contributors are part of the catalog workflow). No separate `routes/contributors.rs` — keep feature cohesion with scan/title/volume handlers.

### Contributor List Accessibility

- Container: `<ul role="list" aria-label="Contributors">`
- Each item: `<li>` with `<a href="#" aria-label="{Name}, {role}">{Name}</a> <span aria-hidden="true">(role)</span>`
- Remove button: `<button aria-label="Remove {Name} as {role}">`
- Separator `·`: `<span aria-hidden="true"> · </span>`

### Previous Story Patterns to Follow

- `feedback_html(variant, message, suggestion)` for feedback entries
- `HtmxResponse { main, oob }` with `OobUpdate` for OOB swaps
- Runtime `sqlx::query()` for new queries (no `.sqlx` cache)
- `html_escape()` for user data in HTML
- Session helpers pattern from `SessionModel`
- Client-side validation before server submission
- Dismiss button on warning/error feedback entries
- `t!()` for ALL user-facing strings
- Log warnings on session update failures

### Project Structure Notes

Files to create:
- `src/models/contributor.rs` — Contributor + TitleContributor models
- `src/services/contributor.rs` — Contributor business logic
- `templates/components/contributor_form.html` — Add contributor inline form
- `templates/components/contributor_list.html` — Contributor list with remove buttons
- `static/js/contributor.js` — Autocomplete behavior

Files to modify:
- `src/models/mod.rs` — Add `pub mod contributor;`
- `src/services/mod.rs` — Add `pub mod contributor;`
- `src/routes/mod.rs` — Add contributor routes
- `src/routes/catalog.rs` — Add contributor handlers, update context_banner_html
- `templates/pages/catalog.html` — Add contributor form/list mount points
- `locales/en.yml` — Add contributor i18n keys
- `locales/fr.yml` — Add French translations

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-1, Story 1.5]
- [Source: _bmad-output/planning-artifacts/prd.md#FR51-FR54, #FR97]
- [Source: _bmad-output/planning-artifacts/architecture.md#contributors-model]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#ContributorList]
- [Source: _bmad-output/implementation-artifacts/1-4-volume-management.md#Dev-Notes]
- [Source: migrations/20260329000000_initial_schema.sql#contributors, #title_contributors, #contributor_roles]
- [Source: migrations/20260330000002_seed_default_reference_data.sql#contributor-roles-seed]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- Runtime sqlx::query() for all new queries (no .sqlx cache)
- context_banner_html signature extended: added `author: Option<&str>` parameter — all call sites updated
- ContributorModel + TitleContributorModel + ContributorRoleModel in single file (models/contributor.rs)
- Contributor list HTML built with grouped roles per contributor (avoid duplicating names)

### Completion Notes List

- 71 unit tests passing (8 new + 63 existing), 0 clippy warnings
- Tasks 1-9 implemented: ContributorModel (CRUD + search + deletion guard), TitleContributorModel (junction queries + primary author), ContributorRoleModel (existence check + list), ContributorService (find_or_create, add/remove/update/delete with validation), 7 routes (form, search, add, remove, update, delete), contributor form template with autocomplete, contributor list with ARIA + role grouping + remove buttons, autocomplete JS with combobox/listbox ARIA, context banner with primary author, i18n keys (en+fr), unit tests, 10 E2E Playwright tests
- Duplicate detection uses same pattern as volume.rs (string matching on DB error)
- Primary author query: prioritizes "Auteur" role, then falls back to first contributor by creation order
- Contributor.js uses MutationObserver to initialize autocomplete on dynamically loaded forms

### Change Log

- 2026-03-30: Story 1.5 implementation complete — Contributor Management

### File List

**Created:**
- `src/models/contributor.rs`
- `src/services/contributor.rs`
- `templates/components/contributor_form.html`
- `static/js/contributor.js`
- `tests/e2e/specs/journeys/catalog-contributor.spec.ts`

**Modified:**
- `src/models/mod.rs` — added `pub mod contributor;`
- `src/services/mod.rs` — added `pub mod contributor;`
- `src/routes/mod.rs` — added 7 contributor routes
- `src/routes/catalog.rs` — added contributor handlers, contributor_list_html, contributor_form_page, updated context_banner_html with author parameter
- `templates/pages/catalog.html` — added contributor-list and contributor-form-container mount points
- `templates/layouts/base.html` — added contributor.js script load
- `locales/en.yml` — added contributor namespace keys, banner with author key
- `locales/fr.yml` — added French translations
