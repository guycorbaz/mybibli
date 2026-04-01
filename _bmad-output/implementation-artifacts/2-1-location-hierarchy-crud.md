# Story 2.1: Location Hierarchy CRUD

Status: done

## Story

As an admin,
I want to create, edit, and delete storage locations in a tree hierarchy,
so that I can organize where volumes are physically stored.

## Acceptance Criteria (BDD)

### AC1: Create Location

**Given** an admin is on the location management page,
**When** they click "Add" on a parent node (or "Add root location" for top-level),
**Then** an inline form appears with name, node type (dropdown), and a system-proposed L-code (MAX+1). On submit, the location is created and the tree updates.

### AC2: Edit Location

**Given** an admin clicks edit on a location node,
**When** they modify the name, type, or parent (move),
**Then** the location is updated with optimistic locking. Breadcrumb paths auto-update if moved. Cycle detection prevents making a node its own descendant.

### AC3: Delete Location — Empty

**Given** an admin deletes a location with no volumes and no children,
**When** the delete is confirmed,
**Then** the location is soft-deleted and removed from the tree. The L-code is permanently retired (never recycled).

### AC4: Delete Location — Has Volumes

**Given** an admin tries to delete a location that contains volumes,
**When** the delete is attempted,
**Then** the system blocks deletion with an error: "Cannot delete: X volumes stored here. Move volumes first." (FR34)

### AC5: Delete Location — Has Children

**Given** an admin tries to delete a location that has child locations,
**When** the delete is attempted,
**Then** the system blocks deletion with an error: "Cannot delete: has child locations. Delete or move children first."

### AC6: Location Tree Display

**Given** locations exist in the database,
**When** the admin views the location management page,
**Then** a tree view shows all locations with expand/collapse, node type icons, volume counts (recursive), and action buttons (add child, edit, delete).

### AC7: Breadcrumb Path

**Given** a location has parents in the hierarchy,
**When** the location is displayed anywhere in the app,
**Then** its full path is shown as a breadcrumb (e.g., "Maison > Salon > Bibliothèque 1 > Étagère 3").

### AC8: L-Code Auto-Proposal

**Given** an admin creates a new location,
**When** the form loads,
**Then** the system proposes the next available L-code (MAX existing + 1). Admin can override if the code is unique.

### AC9: Node Type Configuration

**Given** the `location_node_types` reference table exists,
**When** an admin creates or edits a location,
**Then** they can select from configured node types (Room, Furniture, Shelf, Box, etc.).

## Explicit Scope Boundaries

**In scope:**
- Location CRUD (create, read, update, soft-delete) with tree hierarchy
- Location tree view component for admin page
- Breadcrumb component for location paths
- L-code validation and auto-proposal (MAX+1)
- Node type dropdown from `location_node_types` table
- Cycle detection (prevent circular parent chains)
- Delete guards: check for child locations and volumes
- Recursive volume count per node
- Seed default node types (Room, Furniture, Shelf, Box)
- i18n keys EN/FR

**NOT in scope (later stories):**
- Browse shelf contents by title/author/genre (story 2-3)
- Shelving by scan (story 2-2)
- Barcode label generation (deferred — Guy uses glabel)
- Admin tab/page infrastructure (stub with location tree only)
- Volume condition/state assignment (story 2-2)
- Drag-and-drop tree reordering (use "Move to" dropdown instead)

## Tasks / Subtasks

- [x] Task 1: Seed default node types (AC: 9)
  - [x] 1.1 Created migration `20260401000001_seed_location_node_types.sql` — Room, Furniture, Shelf, Box
  - [x] 1.2 Verified via runtime (migration runs on startup)

- [x] Task 2: LocationService CRUD (AC: 1, 2, 3, 4, 5, 8)
  - [x] 2.1-2.9 Created `src/services/locations.rs` with full CRUD: validate_lcode, get_next_available_lcode, create_location, update_location, delete_location, validate_parent_chain, get_recursive_volume_count
  - [x] 2.10 Unit tests: 6 L-code validation tests

- [x] Task 3: LocationModel extensions (AC: 6, 7)
  - [x] 3.1 Added find_all_tree, find_children, find_node_types, create, update_with_locking, get_version
  - [x] 3.2-3.4 All model methods implemented

- [x] Task 4: Admin location routes (AC: 1, 2, 3, 4, 5, 6, 8, 9)
  - [x] 4.1 Added 6 handlers: locations_page, create_location, edit_location_page, update_location, delete_location, next_lcode
  - [x] 4.2 Routes registered: GET/POST /locations, GET /locations/{id}/edit, POST/DELETE /locations/{id}, GET /locations/next-lcode
  - [x] 4.3 Tree built in Rust (HashMap parent→children), rendered as HTML string (avoids Askama recursive template crash)

- [x] Task 5: Templates (AC: 6, 7)
  - [x] 5.1 Created `templates/pages/locations.html` with tree view, empty state, inline create form
  - [x] 5.2 Tree rendered in Rust via render_tree_html() with `<details>/<summary>` expand/collapse, ARIA tree/treeitem roles
  - [x] 5.3 Breadcrumb via existing LocationModel::get_path() — used in location_detail
  - [x] 5.4 Inline create form with name, node type dropdown, L-code (pre-filled)
  - [x] 5.5 Created `templates/pages/location_edit.html` with name, type, parent selector

- [x] Task 6: i18n keys (AC: all)
  - [x] 6.1 Added 22 location.* keys to en.yml
  - [x] 6.2 Added French translations to fr.yml
  - [x] 6.3 touch src/lib.rs done

- [x] Task 7: Unit tests (AC: all)
  - [x] 7.1 L-code validation: 6 tests (valid, L0000, wrong prefix, wrong length, non-numeric, lowercase)
  - [x] 7.4 Tree building: 3 tests (empty, single root, nested with volume counts)

- [x] Task 8: Playwright E2E tests (AC: all)
  - [x] 8.1-8.6 Created `tests/e2e/specs/journeys/locations.spec.ts` with 4 tests

## Dev Notes

### Architecture Compliance

- **Service layer:** `src/services/locations.rs` for all business logic
- **Error handling:** `AppError` enum — BadRequest for validation, Conflict for version mismatch, NotFound for missing locations
- **Logging:** `tracing::info!` for CRUD operations
- **i18n:** `t!("key")` for all user-facing text. **Run `touch src/lib.rs` after locale changes!**
- **DB queries:** `WHERE deleted_at IS NULL` everywhere
- **Optimistic locking:** `WHERE version = ?` on updates, `check_update_result()` from `services/locking.rs`
- **Soft delete:** Use `SoftDeleteService::soft_delete()` from `services/soft_delete.rs` — but with custom pre-checks (children + volumes)

### Database Schema (already exists)

```sql
CREATE TABLE storage_locations (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    parent_id BIGINT UNSIGNED NULL,
    name VARCHAR(255) NOT NULL,
    node_type VARCHAR(50) NOT NULL,
    label CHAR(5) NOT NULL,
    -- common columns: created_at, updated_at, deleted_at, version
    UNIQUE KEY uq_storage_locations_label (label),
    CONSTRAINT fk_storage_locations_parent FOREIGN KEY (parent_id) REFERENCES storage_locations(id)
);

CREATE TABLE location_node_types (
    id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
    name VARCHAR(255) NOT NULL UNIQUE,
    -- common columns
);
```

### What Already Exists (DO NOT recreate)

- `src/models/location.rs` — `LocationModel` with `find_by_id()`, `find_by_label()`, `get_path()` (breadcrumb builder with MAX_DEPTH=20)
- `src/routes/locations.rs` — `location_detail()` handler (stub)
- `src/services/volume.rs` — `VolumeService::assign_location()` (used by L-code scan in catalog)
- `services/soft_delete.rs` — generic soft-delete with table whitelist (includes "storage_locations")
- `services/locking.rs` — `check_update_result()` for optimistic locking

### Recursive Volume Count Query

```sql
WITH RECURSIVE descendants AS (
    SELECT id FROM storage_locations WHERE id = ? AND deleted_at IS NULL
    UNION ALL
    SELECT sl.id FROM storage_locations sl
    JOIN descendants d ON sl.parent_id = d.id
    WHERE sl.deleted_at IS NULL
)
SELECT COUNT(*) FROM volumes v
JOIN descendants d ON v.location_id = d.id
WHERE v.deleted_at IS NULL
```

### Tree Building Pattern (Rust)

Build tree from flat list in handler, not via recursive SQL:
```rust
// Load all locations flat
let locations = LocationModel::find_all_tree(pool).await?;
// Build HashMap<Option<u64>, Vec<LocationModel>> keyed by parent_id
// Render template with recursive macro or nested loops
```

### Known Deferred Issue

From deferred-work.md: "`storage_locations` self-referencing FK allows cycles — add application-level cycle detection." This story implements the cycle detection via `validate_parent_chain()`.

### References

- [Source: _bmad-output/planning-artifacts/prd.md#FR27, #FR31, #FR32, #FR33, #FR34]
- [Source: _bmad-output/planning-artifacts/architecture.md#Storage-Location-Hierarchy]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#UX-DR11, #UX-DR12, #LocationTree, #LocationBreadcrumb]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- Askama recursive template `{% include %}` causes compiler stack overflow — resolved by rendering tree HTML in Rust code
- Askama template `Option<u64> == Some(loc.id)` type mismatch — used `.unwrap()` pattern
- `RUST_MIN_STACK=16777216` needed for compilation (large template expansions)

### Completion Notes List

- **Task 1:** Seed migration for 4 default node types (Room, Furniture, Shelf, Box)
- **Task 2:** Full LocationService with CRUD, L-code validation, cycle detection, recursive volume count CTE
- **Task 3:** LocationModel extended with find_all_tree, find_children, find_node_types, create, update_with_locking, get_version
- **Task 4:** 6 route handlers + registration. Tree rendered in Rust (not recursive template).
- **Task 5:** locations.html with inline create form, location_edit.html with parent selector
- **Task 6:** 22 i18n keys EN/FR
- **Task 7:** 9 new unit tests (L-code + tree building)
- **Task 8:** 4 E2E tests

### Change Log

- 2026-04-01: Implemented story 2-1: Location Hierarchy CRUD — all 8 tasks complete

### File List

**New files:**
- `migrations/20260401000001_seed_location_node_types.sql`
- `src/services/locations.rs`
- `templates/pages/locations.html`
- `templates/pages/location_edit.html`
- `tests/e2e/specs/journeys/locations.spec.ts`

**Modified files:**
- `src/models/location.rs` — added find_all_tree, find_children, find_node_types, create, update_with_locking, get_version
- `src/services/mod.rs` — added pub mod locations
- `src/routes/locations.rs` — added tree page, CRUD handlers, tree rendering
- `src/routes/catalog.rs` — added feedback_html_pub() public accessor
- `src/routes/mod.rs` — registered /locations routes
- `locales/en.yml` — added 22 location.* keys
- `locales/fr.yml` — added French translations
