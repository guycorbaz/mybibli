# Story 4.1: Borrower CRUD & Search

Status: done

## Story

As a librarian,
I want to create, edit, search, and delete borrowers,
so that I can manage the people who borrow from my library.

## Acceptance Criteria

### AC1: Borrowers List Page (FR41)

- Given a librarian navigates to `/borrowers`
- When the page loads
- Then a list of all active borrowers is displayed (name, email, phone) with pagination (25 per page)
- And an "Add borrower" button is visible
- And anonymous users are redirected to login (NFR11)

### AC2: Create Borrower (FR41)

- Given the librarian clicks "Add borrower" on `/borrowers`
- When they fill in name (required), address, email, phone and submit
- Then the borrower is created and appears in the list
- And a success FeedbackEntry is displayed
- And name validation rejects empty or whitespace-only input
- Note: Librarian role can create borrowers (unlike locations which require Admin) — borrower creation is a frequent cataloging workflow action, not an admin task

### AC3: Borrower Detail Page (FR41)

- Given a borrower exists
- When the librarian navigates to `/borrower/{id}`
- Then the borrower's contact details are displayed (name, address, email, phone)
- And an "Edit" button and "Delete" button are visible for Admin role
- And anonymous users are redirected to login (NFR11)

### AC4: Edit Borrower (FR98)

- Given the borrower detail page at `/borrower/{id}`
- When the admin clicks "Edit" and modifies contact details and saves
- Then changes are persisted with optimistic locking (version check)
- And a success FeedbackEntry is displayed
- And on version conflict (409), an error message appears

### AC5: Search Borrowers with Autocomplete (FR42)

- Given a search endpoint at `/borrowers/search?q=`
- When the librarian types 2+ characters
- Then matching borrowers are returned as JSON (id, name) for autocomplete
- And results are limited to 10 matches
- And LIKE wildcards in user input are escaped

### AC6: Delete Borrower — No Active Loans (FR119)

- Given a borrower with no active loans
- When the admin clicks "Delete" on the borrower detail page
- Then a confirmation modal appears
- And on confirmation, the borrower is soft-deleted
- And a success FeedbackEntry is displayed

### AC7: Delete Borrower — Active Loans Block (FR50, FR119)

- Given a borrower with active loans (loans WHERE returned_at IS NULL)
- When the admin clicks "Delete"
- Then deletion is blocked with a message showing active loan count
- And no data is modified

### AC8: Navigation Integration

- Given the nav bar
- When a librarian is logged in
- Then a "Borrowers" link appears in the navigation (after Locations, before Admin)
- And Ctrl+Shift+B keyboard shortcut navigates to /borrowers (Ctrl+B conflicts with browser bold/bookmarks)

## Tasks / Subtasks

- [x] Task 1: Borrower Model (AC: #1, #2, #3)
  - [ ] Create `src/models/borrower.rs` with `BorrowerModel` struct: id, name, address (Option), email (Option), phone (Option), version
  - [ ] `find_by_id(pool, id)` — SELECT with deleted_at IS NULL
  - [ ] `list_active(pool, page)` — paginated list ordered by name, returns PaginatedList<BorrowerModel>
  - [ ] `create(pool, name, address, email, phone)` — INSERT with validation
  - [ ] `update_with_locking(pool, id, version, name, address, email, phone)` — UPDATE with WHERE version = ?
  - [ ] `soft_delete(pool, id)` — UPDATE deleted_at = NOW()
  - [ ] `search_by_name(pool, query, limit)` — LIKE search with escaped wildcards, returns Vec<BorrowerModel>
  - [ ] `count_active_loans(pool, id)` — SELECT COUNT(*) FROM loans WHERE borrower_id = ? AND returned_at IS NULL AND deleted_at IS NULL
  - [ ] Register module in `src/models/mod.rs`
  - [ ] Unit tests for struct construction, search escaping

- [x] Task 2: Borrower Service (AC: #2, #4, #6, #7)
  - [ ] Create `src/services/borrowers.rs` with `BorrowerService` struct
  - [ ] `create_borrower(pool, name, address, email, phone)` — validate name not empty, trim fields, call model create
  - [ ] `update_borrower(pool, id, version, name, address, email, phone)` — validate name, call model update_with_locking
  - [ ] `delete_borrower(pool, id)` — check active loans count, block if > 0, call `SoftDeleteService::soft_delete(pool, "borrowers", id)` if 0 (borrowers already in whitelist)
  - [ ] Register module in `src/services/mod.rs`
  - [ ] Unit tests for validation logic

- [x] Task 3: Borrower Routes (AC: #1-#8)
  - [ ] Create `src/routes/borrowers.rs`
  - [ ] `GET /borrowers` → `borrowers_page()` — Librarian role, loads paginated list, renders template
  - [ ] `POST /borrowers` → `create_borrower()` — Librarian role, form submission, redirect to /borrowers
  - [ ] `GET /borrower/{id}` → `borrower_detail()` — Librarian role, loads borrower, renders detail template
  - [ ] `GET /borrower/{id}/edit` → `edit_borrower_page()` — Admin role, renders edit form
  - [ ] `POST /borrower/{id}` → `update_borrower()` — Admin role, form submission with optimistic locking
  - [ ] `DELETE /borrower/{id}` → `delete_borrower()` — Admin role, checks active loans, soft-deletes
  - [ ] `GET /borrowers/search` → `borrower_search()` — Librarian role, JSON autocomplete (same pattern as contributor_search in catalog.rs)
  - [ ] Register all routes in `src/routes/mod.rs`
  - [ ] Unit tests for handlers

- [x] Task 4: Borrower Templates (AC: #1, #2, #3, #4)
  - [ ] Create `templates/pages/borrowers.html` — extends base.html, paginated table (name, email, phone), "Add borrower" button, add form (inline or modal)
  - [ ] Create `templates/pages/borrower_detail.html` — extends base.html, contact details display, Edit/Delete buttons (Admin only), future: active loans list (story 4-4)
  - [ ] Create `templates/pages/borrower_edit.html` — extends base.html, form with name (required), address (textarea), email, phone, hidden version field, Save/Cancel buttons
  - [ ] All templates use i18n labels via template struct fields

- [x] Task 5: i18n Keys (AC: #1-#8) — **Must be completed BEFORE Task 6 and Task 7** (templates won't compile without i18n keys)
  - [ ] Add to `locales/en.yml` and `locales/fr.yml`:
    - `nav.borrowers: "Borrowers"` / `"Emprunteurs"`
    - `borrower.list_title: "Borrowers"` / `"Emprunteurs"`
    - `borrower.add: "Add borrower"` / `"Ajouter un emprunteur"`
    - `borrower.name: "Name"` / `"Nom"`
    - `borrower.address: "Address"` / `"Adresse"`
    - `borrower.email: "Email"` / `"Email"`
    - `borrower.phone: "Phone"` / `"Téléphone"`
    - `borrower.edit: "Edit borrower"` / `"Modifier l'emprunteur"`
    - `borrower.delete: "Delete"` / `"Supprimer"`
    - `borrower.created: "Borrower created: %{name}"` / `"Emprunteur créé : %{name}"`
    - `borrower.updated: "Borrower updated."` / `"Emprunteur mis à jour."`
    - `borrower.deleted: "Borrower deleted."` / `"Emprunteur supprimé."`
    - `borrower.delete_has_loans: "Cannot delete: %{name} has %{count} active loan(s)."` / `"Suppression impossible : %{name} a %{count} prêt(s) actif(s)."`
    - `borrower.name_required: "Borrower name is required."` / `"Le nom de l'emprunteur est obligatoire."`
    - `borrower.save: "Save"` / `"Enregistrer"`
    - `borrower.cancel: "Cancel"` / `"Annuler"`
    - `borrower.empty_state: "No borrowers yet. Add one to start lending!"` / `"Aucun emprunteur. Ajoutez-en un pour commencer les prêts !"`
    - `borrower.detail_title: "Borrower details"` / `"Détails de l'emprunteur"`
    - `borrower.confirm_delete: "Delete this borrower?"` / `"Supprimer cet emprunteur ?"`
  - [ ] Run `touch src/lib.rs && cargo build`

- [x] Task 6: Navigation Integration (AC: #8)
  - [ ] Add "Borrowers" link to `templates/components/nav_bar.html` — nav order: Catalog → Locations → Borrowers → Loans → Admin
  - [ ] Add `nav_borrowers` field (plural, matching `nav_locations` pattern) to ALL page template structs that use the nav bar
  - [ ] Update ALL route handlers that render full pages to include `nav_borrowers: rust_i18n::t!("nav.borrowers").to_string()`
  - [ ] Add Ctrl+Shift+B keyboard shortcut in `static/js/mybibli.js` in `initKeyboardShortcuts()` (alongside existing Ctrl+K pattern for /catalog)
  - [ ] **Warning**: This touches many files (every route module with a full-page template). Grep for `nav_locations` to find all locations to update.

- [x] Task 7: E2E Tests (AC: #1-#8)
  - [ ] E2E: navigate to /borrowers → see empty state → add borrower → see in list
  - [ ] E2E: edit borrower → verify changes saved
  - [ ] E2E: delete borrower with no loans → verify removed from list
  - [ ] E2E: search borrower via autocomplete → verify results
  - [ ] **Smoke test**: login → /borrowers → create → edit → delete → verify full journey
  - [ ] Test file: `tests/e2e/specs/journeys/borrowers.spec.ts`

- [x] Task 8: Unit Tests
  - [ ] BorrowerModel: struct construction, search_by_name escaping
  - [ ] BorrowerService: validate name, create, update, delete with loan guard
  - [ ] Routes: form struct deserialization, autocomplete JSON format

## Dev Notes

### Architecture Compliance

- **Routes thin, services thick**: Route handlers extract params, call service, return response. All validation and loan-guard logic lives in `src/services/borrowers.rs`.
- **Error handling**: Use `AppError` enum. Conflict = 409, BadRequest = 400, Unauthorized = 303 redirect.
- **HTMX pattern**: Check `HxRequest` — return fragment for HTMX, full page for direct nav.
- **Optimistic locking**: All updates use `WHERE id = ? AND version = ?` + `check_update_result()`.
- **Soft delete**: All queries include `deleted_at IS NULL`. Use `services/soft_delete.rs` whitelist.
- **i18n**: All user-facing text via `t!()`.
- **Logging**: `tracing` macros only.

### Existing Patterns to Follow

**Location CRUD** (`src/routes/locations.rs`, `src/models/location.rs`, `src/services/locations.rs`):
- List page: `GET /locations` → Librarian, renders template with tree
- Create: `POST /locations` → Admin, form submission, redirect
- Edit page: `GET /locations/{id}/edit` → Admin, form template
- Update: `POST /locations/{id}` → Admin, optimistic locking
- Delete: `DELETE /locations/{id}` → Admin, guard checks

**Contributor search** (`src/routes/catalog.rs:1125-1150`):
- `GET /catalog/contributors/search?q=` → Librarian, JSON array response
- `ContributorModel::search_by_name(pool, q, 10)` — LIKE with escaped wildcards
- Returns `Vec<{id, name}>` as JSON

**Template struct pattern** (all page templates):
- Fields: lang, role, current_page, skip_label, nav_catalog, nav_loans, nav_locations, nav_admin, nav_login, nav_logout + page-specific fields
- `current_page: &'static str` for active nav highlighting

**Route registration** (`src/routes/mod.rs`):
- Group related routes together
- CRUD routes follow REST patterns: GET list, POST create, GET detail, GET edit, POST update, DELETE

### Database Schema (Already Exists)

```sql
CREATE TABLE borrowers (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    address TEXT NULL,
    email VARCHAR(255) NULL,
    phone VARCHAR(50) NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_borrowers_deleted_at (deleted_at)
);
```

No migration needed — table already exists in initial schema.

### Active Loans Check Query

```sql
SELECT COUNT(*) FROM loans
WHERE borrower_id = ? AND returned_at IS NULL AND deleted_at IS NULL
```

### Nav Bar Impact

Adding "Borrowers" to the nav bar requires updating ALL template structs and route handlers that render full pages. Follow the exact pattern used when "Locations" was added (commit `780c261`). Key files:
- `templates/components/nav_bar.html`
- ALL `*Template` structs in routes/*.rs
- ALL route handler functions that populate template structs

### Previous Story Intelligence

**From Epic 3 retro:**
- HTMX fragments MUST include swap target IDs matching the full-page template
- Tests must cover the real user workflow, not isolated steps
- Bug fixes must include regression tests (CLAUDE.md rules 2+3 strengthened)

### References

- [Source: _bmad-output/planning-artifacts/prd.md] — FR41, FR42, FR50, FR89, FR98, FR119, NFR11
- [Source: _bmad-output/planning-artifacts/architecture.md] — borrowers table schema, routes/borrowers.rs, services/loans.rs, loans table
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md] — /borrowers page layout, autocomplete pattern, deletion guards, NFR11 access control
- [Source: src/routes/locations.rs] — CRUD route pattern
- [Source: src/routes/catalog.rs:1125-1150] — Autocomplete search pattern
- [Source: src/services/locations.rs] — Service layer with validation + guards

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Task 1: Created BorrowerModel with find_by_id, list_active, create, update_with_locking, search_by_name, count_active_loans. 3 unit tests.
- Task 2: Created BorrowerService with create/update/delete. Delete guards against active loans via count_active_loans. Uses SoftDeleteService. 4 unit tests.
- Task 3: Created 7 route handlers in borrowers.rs: list, create, detail, edit page, update, delete, search autocomplete. Registered in mod.rs. 3 unit tests.
- Task 4: Created 3 Askama templates: borrowers.html (list + inline add form), borrower_detail.html (display + edit/delete buttons), borrower_edit.html (form with optimistic locking).
- Task 5: Added 18 i18n keys under borrower.* + nav.borrowers in both en.yml and fr.yml.
- Task 6: Added Borrowers link to nav bar (desktop + mobile), nav_borrowers field to ALL 10 template structs across 6 route files, Ctrl+Shift+B shortcut in mybibli.js.
- Task 7: Created borrowers.spec.ts with 7 E2E tests including smoke test.
- Task 8: Unit tests integrated into model, service, and route modules. 274 total tests passing.

### File List

New files:
- src/models/borrower.rs
- src/services/borrowers.rs
- src/routes/borrowers.rs
- templates/pages/borrowers.html
- templates/pages/borrower_detail.html
- templates/pages/borrower_edit.html
- tests/e2e/specs/journeys/borrowers.spec.ts

Modified files:
- src/models/mod.rs (added borrower module)
- src/services/mod.rs (added borrowers module)
- src/routes/mod.rs (added borrowers module + 4 routes)
- templates/components/nav_bar.html (added Borrowers link desktop + mobile)
- static/js/mybibli.js (Ctrl+Shift+B shortcut)
- locales/en.yml (nav.borrowers + 17 borrower.* keys)
- locales/fr.yml (nav.borrowers + 17 borrower.* keys)
- src/routes/auth.rs (nav_borrowers field + value)
- src/routes/home.rs (nav_borrowers field + value)
- src/routes/locations.rs (nav_borrowers field + value)
- src/routes/titles.rs (nav_borrowers field + value)
- src/routes/catalog.rs (nav_borrowers field + value)
- src/routes/contributors.rs (nav_borrowers field + value)
