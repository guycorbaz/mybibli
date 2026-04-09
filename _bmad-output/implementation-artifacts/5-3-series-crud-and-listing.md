# Story 5.3: Series CRUD & Listing

Status: done

## Story

As a librarian,
I want to create, edit, and browse series,
so that I can organize my titles into coherent collections.

## Acceptance Criteria

1. **Create series:** Given `/series` page, when librarian creates a series with name, type (open/closed), and (if closed) total volume count, then the series is created and appears in the series list
2. **List with stats:** Given series exist, when any user visits `/series`, then the list shows name, type, owned count, total count (for closed), and gap count, paginated 25/page per NFR39
3. **Edit with optimistic locking:** Given a series detail page `/series/{id}`, when librarian edits name/type/total count with optimistic locking, then changes are persisted (409 on version mismatch)
4. **Total count validation:** Given a closed series, when librarian tries to set total count below owned count, then the edit is blocked with a preventive validation message
5. **Public read access:** Given an anonymous user, when they visit `/series` or `/series/{id}`, then they see the list/detail (public read per FR95) — no auth required
6. **Soft delete:** `series` table has `deleted_at`, `version`, `created_at`, `updated_at` columns; unique(name) WHERE deleted_at IS NULL
7. **Unit tests:** SeriesModel CRUD, optimistic locking
8. **E2E smoke:** create closed series -> visit detail -> edit -> verify persistence

## Tasks / Subtasks

- [x] Task 1: Create SeriesModel (AC: #6, #7)
  - [x] 1.1 Created `src/models/series.rs` with `SeriesModel` struct, `SeriesType` enum with Display+FromStr
  - [x] 1.2 Implemented `active_find_by_id`, `active_list` (paginated), `active_find_by_name`, `create`, `update_with_locking`, `soft_delete`
  - [x] 1.3 Added `active_count_titles(pool, series_id)` with JOIN on titles table for soft-delete filtering
  - [x] 1.4 Registered module in `src/models/mod.rs`
  - [x] 1.5 6 unit tests: SeriesType Display/FromStr roundtrip, case insensitive, unknown, Display, gap count
- [x] Task 2: Create SeriesService (AC: #1, #3, #4)
  - [x] 2.1 Created `src/services/series.rs` with SeriesService struct
  - [x] 2.2 `create_series` with name validation, uniqueness check, total_volume_count validation for closed series
  - [x] 2.3 `update_series` with optimistic locking via `check_update_result()`, total >= owned validation
  - [x] 2.4 `delete_series` delegates to `SeriesModel::soft_delete()`
  - [x] 2.5 Registered in `src/services/mod.rs`
  - [x] 2.6 6 unit tests for validation logic
- [x] Task 3: Create series routes and templates (AC: #1, #2, #3, #5)
  - [x] 3.1-3.11 Created full CRUD: list, detail, create form, edit form, update, delete
  - [x] Templates: series_list.html, series_detail.html, series_form.html
  - [x] Routes registered in mod.rs: GET/POST /series, GET/POST/DELETE /series/{id}, GET /series/new, GET /series/{id}/edit
  - [x] Added `deserialize_optional_i32` for form fields that may be empty string
- [x] Task 4: Add i18n keys (AC: #1, #2, #3, #4)
  - [x] 4.1 Added 20+ series i18n keys to en.yml and fr.yml
  - [x] 4.2 Added nav.series label
  - [x] 4.3 i18n recompilation verified
- [x] Task 5: Add nav link (AC: #5)
  - [x] 5.1 Added Series link to nav_bar.html (desktop + mobile) between Locations and Borrowers
  - [x] 5.2 Updated 15 template structs across 8 route files with nav_series field
- [x] Task 6: sqlx cache — no new query!() macros, existing .sqlx/ cache restored from git
- [x] Task 7: E2E smoke test (AC: #8)
  - [x] 7.1 Created `tests/e2e/specs/journeys/series.spec.ts`
  - [x] 7.2 Smoke: create closed series → detail → edit name → verify persistence — PASS
  - [x] 7.3 Anonymous access: clear cookies → /series visible — PASS
  - [x] 7.4 Delete: create → delete → verify removed — PASS
- [x] Task 8: Verification
  - [x] 8.1 `cargo clippy -- -D warnings` — clean
  - [x] 8.2 `cargo test` — 303 passed, 0 failed
  - [x] 8.3 `.sqlx/` cache valid (no new query macros)
  - [x] 8.4 E2E: 122/123 passed (1 pre-existing borrower-loans data isolation flake)

### Review Findings

- [x] [Review][Decision] Silent enum default — added `tracing::warn!` on invalid series_type parse before defaulting to Open
- [x] [Review][Patch] Gap count arithmetic — extracted `compute_gap()` helper with `saturating_sub` [src/routes/series.rs]
- [x] [Review][Patch] Hidden form field — JS now clears `#series-total` value when switching to "open" [series_form.html]
- [x] [Review][Defer] Soft delete doesn't check version — no optimistic locking on delete. Pre-existing pattern shared by all entities (borrower, location, contributor).
- [x] [Review][Defer] TOCTOU race on name uniqueness — application-level check without DB constraint. Pre-existing pattern.
- [x] [Review][Defer] Delete allows orphaned title_series assignments — story 5.4 will add title assignments + deletion guard.

## Dev Notes

### Database Schema Already Exists

The `series` table and `title_series` junction table are already defined in `migrations/20260329000000_initial_schema.sql`:

**`series` table (lines 177-188):**
```sql
CREATE TABLE series (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT NULL,
    series_type ENUM('open', 'closed') NOT NULL DEFAULT 'open',
    total_volume_count INT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    deleted_at TIMESTAMP NULL DEFAULT NULL,
    version INT NOT NULL DEFAULT 1,
    INDEX idx_series_deleted_at (deleted_at)
);
```

**`title_series` junction table (lines 190-206):** exists but is NOT used in this story — title assignment is story 5.4.

**No unique constraint on `name`** — the AC says unique(name) WHERE deleted_at IS NULL. This needs a migration to add a partial unique index. MariaDB doesn't support partial unique indexes natively. Use a workaround: either application-level uniqueness check, or a unique index on `(name, deleted_at)` with a sentinel value. The simplest approach for this project: **application-level uniqueness in the service** (check for existing active series with same name before create/update). This is the pattern used by other entities.

### No Rust Code Exists Yet

There are no files for series in `src/models/`, `src/services/`, `src/routes/`, or `templates/`. Everything must be created from scratch.

### Patterns to Follow

**Primary reference: Borrower CRUD** (`src/models/borrower.rs`, `src/services/borrowers.rs`, `src/routes/borrowers.rs`)
- Simple entity with pagination, optimistic locking, soft delete
- `PaginatedList<T>` from `src/models/mod.rs:16-60` with `DEFAULT_PAGE_SIZE = 25`
- Offset calculation: `(page.saturating_sub(1)) * DEFAULT_PAGE_SIZE`
- Count query + items query pattern for pagination

**Borrower list route pattern** (`src/routes/borrowers.rs:53-94`):
```rust
#[derive(Deserialize)]
pub struct ListQuery { #[serde(default = "default_page")] pub page: u32 }
fn default_page() -> u32 { 1 }
```

**Borrower detail route pattern** (`src/routes/borrowers.rs:161-215`):
- Fetches entity by ID → renders template with nav labels
- Template struct has `lang`, `role`, `current_page`, all nav labels, entity data

**Optimistic locking** (`src/services/locking.rs`):
- `check_update_result(rows_affected: u64, entity_type: &str) -> Result<(), AppError>` — verifies rows_affected > 0, returns `AppError::Conflict` with i18n message on mismatch
- UPDATE query: `WHERE id = ? AND version = ?` + `SET version = version + 1`
- Call: `check_update_result(result.rows_affected(), "series")?;`

**Delete with redirect** (from story 5-2 learnings):
```rust
if is_htmx {
    Ok((StatusCode::OK, [("hx-redirect", "/series")], String::new()).into_response())
} else {
    Ok(Redirect::to("/series").into_response())
}
```

**Route registration pattern** (from `src/routes/mod.rs`):
```rust
.route("/series", get(series::series_list_page).post(series::create_series))
.route("/series/{id}", get(series::series_detail_page).post(series::update_series).delete(series::delete_series))
.route("/series/{id}/edit", get(series::edit_series_page))
```

### Series Type Handling

The `series_type` column is `ENUM('open', 'closed')`. Follow the project pattern from `src/models/media_type.rs`: create a `SeriesType` Rust enum with `Display` + `FromStr` traits. Read from DB as `String`, then parse via `.parse::<SeriesType>()`. Store by calling `.to_string()` when binding to queries. Example:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesType { Open, Closed }
impl fmt::Display for SeriesType { /* "open" / "closed" */ }
impl FromStr for SeriesType { /* parse "open" / "closed" */ }
```

For the form: use a `<select>` dropdown with two options. When type is "closed", show the `total_volume_count` field (use JS to toggle visibility, or always show but make it optional and validate server-side).

### Owned Count / Gap Count Computation

For story 5.3, the `title_series` table will likely have zero rows (title assignment is story 5.4). But the model should still support the query with proper JOIN filtering per soft-delete rules:

```sql
SELECT COUNT(*) FROM title_series ts
JOIN titles t ON ts.title_id = t.id
WHERE ts.series_id = ? AND ts.deleted_at IS NULL AND t.deleted_at IS NULL
```

The gap count for a closed series = `total_volume_count - owned_count`. For open series, gap count is not shown.

### Nav Bar Update

Adding a nav link requires updating `templates/components/nav_bar.html` AND adding `nav_series: String` to **12 template structs** across these files. Search for `nav_catalog` to find them all:

| File | Structs |
|------|---------|
| `src/routes/home.rs` | HomeTemplate (line 28) |
| `src/routes/auth.rs` | LoginTemplate (line 14) |
| `src/routes/catalog.rs` | CatalogTemplate (line 177) |
| `src/routes/borrowers.rs` | BorrowersTemplate (25), BorrowerDetailTemplate (125), BorrowerEditTemplate (219) |
| `src/routes/loans.rs` | LoansTemplate (29) |
| `src/routes/locations.rs` | LocationsTemplate (220), LocationDetailTemplate (32), LocationEditTemplate (338) |
| `src/routes/contributors.rs` | ContributorDetailTemplate (12) |
| `src/routes/titles.rs` | TitleDetailTemplate (21) |

**NOTE**: `src/routes/admin.rs` does not exist — no update needed there.

Insert `nav_series` between `nav_locations` and `nav_borrowers` in each struct. Also populate `nav_series: rust_i18n::t!("nav.series").to_string()` in each handler.

### E2E Test Strategy

**specId:** `SE` (new, for series)

**Smoke test flow:**
1. `loginAs(page)` → navigate to `/series`
2. Click "Add series" button → fill form (name: `SE-Test-{Date.now()}`, type: closed, total: 10) → submit
3. Verify new series appears in list with name, type "closed", owned: 0, total: 10, gap: 10
4. Click series name → detail page shows correct data
5. Click edit → change name → submit → verify updated name persists
6. Navigate back to list → verify updated name in list

**Anonymous access test:**
1. Clear cookies → visit `/series` → verify list renders (no redirect to login)

**Delete test:**
1. Create series → navigate to detail → click delete → confirm → verify redirect + removed from list

### Project Structure Notes

- This is a **full new entity CRUD** — the largest task type. Many files to create and several cross-cutting template updates.
- The `series` migration already exists, so NO new migration is needed.
- `cargo sqlx prepare` MUST be run after adding the new queries (Task 6).
- The gap grid visualization (SeriesGapGrid component) is story 5.4 scope — do NOT implement it here. The detail page shows text stats only (owned/total/gap counts).

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Story 5.3 AC]
- [Source: _bmad-output/planning-artifacts/architecture.md — Lines 195, 216, 245-262 (file structure), 380-397 (query naming), 407-409 (optimistic locking), 991-1004 (new route checklist)]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md — Lines 1044-1082 (series management journey), 2263-2312 (SeriesGapGrid component), 3282-3288 (responsive design)]
- [Source: migrations/20260329000000_initial_schema.sql — Lines 177-206 (series + title_series tables)]
- [Source: src/models/borrower.rs — reference CRUD pattern with pagination]
- [Source: src/routes/borrowers.rs — reference route pattern with list/detail/create/edit/delete]
- [Source: src/services/locking.rs — check_update_result() for optimistic locking]
- [Source: src/routes/borrowers.rs:315-336 — delete with dual HTMX/non-HTMX redirect]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Created full Series entity CRUD: model, service, routes, 3 templates
- SeriesType enum follows MediaType pattern (Display + FromStr)
- Application-level name uniqueness (no DB constraint — consistent with borrower pattern)
- Optimistic locking via check_update_result() from services/locking.rs
- Closed series validate total_volume_count > 0 on create and >= owned count on update
- Open series clear total_volume_count to NULL
- Form deserialization: custom `deserialize_optional_i32` handles empty string from hidden form field
- Nav bar updated across 15 template structs in 8 files (cross-cutting change)
- Anonymous read access: series list and detail omit session.require_role() — Session extractor returns Anonymous
- Delete uses dual HTMX/non-HTMX redirect pattern (from story 5-2 learnings)
- Gap count computed as total - owned for closed series; not shown for open series
- E2E: 3 tests — smoke lifecycle, anonymous access, delete confirmation
- Pre-existing flake in borrower-loans.spec.ts (duplicate borrower name data isolation) — not caused by this story

### File List

**Created:**
- `src/models/series.rs` — SeriesModel struct, SeriesType enum, CRUD query methods
- `src/services/series.rs` — SeriesService with validation, uniqueness, optimistic locking
- `src/routes/series.rs` — 6 route handlers (list, detail, create form, create, edit form, update, delete)
- `templates/pages/series_list.html` — Paginated list with type badges, owned/total/gap stats
- `templates/pages/series_detail.html` — Detail with stats, edit/delete buttons (Librarian+)
- `templates/pages/series_form.html` — Create/edit form with dynamic total count field
- `tests/e2e/specs/journeys/series.spec.ts` — 3 E2E tests (smoke, anonymous, delete)

**Modified:**
- `src/models/mod.rs` — Added `pub mod series`
- `src/services/mod.rs` — Added `pub mod series`
- `src/routes/mod.rs` — Added `pub mod series` + 4 route registrations
- `templates/components/nav_bar.html` — Added Series nav link (desktop + mobile)
- `locales/en.yml` — Added nav.series, 20+ series i18n keys
- `locales/fr.yml` — Same i18n additions (French)
- `src/routes/home.rs` — Added nav_series field to HomeTemplate
- `src/routes/auth.rs` — Added nav_series field to LoginTemplate
- `src/routes/catalog.rs` — Added nav_series to CatalogTemplate, VolumeDetailTemplate, VolumeEditTemplate
- `src/routes/borrowers.rs` — Added nav_series to 3 template structs
- `src/routes/loans.rs` — Added nav_series to LoansTemplate
- `src/routes/locations.rs` — Added nav_series to 3 template structs
- `src/routes/contributors.rs` — Added nav_series to ContributorDetailTemplate
- `src/routes/titles.rs` — Added nav_series to TitleDetailTemplate
