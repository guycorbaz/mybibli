# Story 2.3: Browse Shelf Contents

Status: done

## Story

As a user,
I want to view the contents of a storage location sorted by title, author, or genre,
so that I can see what's on each shelf and verify the physical order matches the catalog.

## Acceptance Criteria (BDD)

### AC1: Location Contents Display

**Given** a location has volumes shelved at it,
**When** a user views the location detail page (`/location/{id}`),
**Then** a DataTable shows all volumes at that location with: media type icon, title (linked), author, genre, condition, and shelving status.

### AC2: Sorting

**Given** the location contents are displayed,
**When** the user clicks a column header (title, author, genre),
**Then** the list sorts by that column. Clicking again reverses direction.

### AC3: Empty Location

**Given** a location has no volumes,
**When** the detail page loads,
**Then** an empty state message is shown: "No volumes at this location."

### AC4: Breadcrumb Navigation

**Given** a location is viewed,
**When** the page loads,
**Then** the full breadcrumb path is displayed (e.g., "Maison → Salon → Bibliothèque 1 → Étagère 3") with each segment linked to its parent location.

### AC5: Volume Status Indicators

**Given** volumes are displayed in the table,
**When** the user looks at the status column,
**Then** shelved volumes show ✅, volumes on loan show 📘, volumes without location show —.

### AC6: Pagination

**Given** a location has more than 25 volumes,
**When** the user views the page,
**Then** pagination controls are shown (25 items per page).

## Explicit Scope Boundaries

**In scope:**
- Replace location_detail stub with real contents view
- DataTable with volumes (title, author, genre, condition, status)
- Sortable column headers (title, author, genre)
- Breadcrumb with linked segments
- Pagination (25/page)
- Empty state for locations without volumes
- Volume status indicators (shelved/on loan/not shelved)

**NOT in scope:**
- Dewey code sorting (no Dewey data yet — defer)
- Filtering by genre/state (future enhancement)
- Inline editing of volumes from this view
- Child location contents (show only direct volumes, not recursive)

## Tasks / Subtasks

- [ ] Task 1: VolumeModel query for location contents (AC: 1, 2, 6)
  - [ ] 1.1 Add `find_by_location(pool, location_id, sort, dir, page) -> Result<PaginatedList<VolumeWithTitle>>` to `src/models/volume.rs`. JOIN titles + genres, LEFT JOIN volume_states for condition, LEFT JOIN loans (WHERE returned_at IS NULL AND deleted_at IS NULL) for on-loan detection. **Sort whitelist:** `["title", "primary_contributor", "genre_name"]` with `validated_sort()` pattern from `src/models/title.rs`. Default: title ASC. Pagination: 25/page.
  - [ ] 1.2 Create `VolumeWithTitle` struct: volume_id, label, title_id, title_name, media_type, primary_contributor (subquery), genre_name, condition_name, is_on_loan (bool: `l.id IS NOT NULL`). **No location_path** — all volumes share the same location.
  - [ ] 1.3 Unit tests: sort validation, pagination calculation

- [ ] Task 2: Update location_detail handler (AC: 1, 2, 3, 4)
  - [ ] 2.1 Replace the stub in `src/routes/locations.rs` `location_detail()` — load volumes via `find_by_location()`, load child locations for sub-navigation
  - [ ] 2.2 Accept query params: `?sort=title&dir=asc&page=1`
  - [ ] 2.3 Add `LocationModel::get_path_segments(pool, id) -> Result<Vec<(u64, String)>>` — returns `[(id, "Maison"), (id, "Salon"), ...]` for linked breadcrumb. Reuse `get_path()` walk logic but return structured data instead of joined string. Build breadcrumb with `<a href="/location/{id}">` for each segment.
  - [ ] 2.4 Pass volumes, sort, pagination to template

- [ ] Task 3: Update location_detail template (AC: 1, 2, 3, 4, 5)
  - [ ] 3.1 Replace `templates/pages/location_detail.html` stub with DataTable: columns for media icon, title (linked to `/title/{id}`), author, genre, condition, status
  - [ ] 3.2 Sortable column headers: simple `<a href="/location/{id}?sort=title&dir=asc&page=1">` links (full-page navigation, not HTMX fragments — simpler for location detail which is a full page)
  - [ ] 3.3 Breadcrumb component: `<nav aria-label="Location path"><ol>` with `<a href="/location/{id}">` for each segment
  - [ ] 3.4 Empty state: "No volumes at this location."
  - [ ] 3.5 Pagination controls (reuse existing pattern from home search)
  - [ ] 3.6 Status badges: ✅ shelved, 📘 on loan, — not shelved

- [ ] Task 4: i18n keys (AC: all)
  - [ ] 4.1 Add to `locales/en.yml` under `location:`: contents_title, empty_volumes, col_title, col_author, col_genre, col_condition, col_status
  - [ ] 4.2 Add French translations
  - [ ] 4.3 `touch src/lib.rs` before build

- [ ] Task 5: E2E tests (AC: all)
  - [ ] 5.1 Test: Location with volumes shows DataTable
  - [ ] 5.2 Test: Empty location shows empty state
  - [ ] 5.3 Test: Breadcrumb shows full path
  - [ ] 5.4 Test: Sort by clicking column header

## Dev Notes

### What Already Exists (DO NOT recreate)

- `location_detail()` handler in `src/routes/locations.rs` — stub returning "coming soon"
- `LocationDetailTemplate` struct — needs update (add volumes, remove coming_soon)
- `templates/pages/location_detail.html` — needs complete rewrite (replace stub)
- `LocationModel::find_by_id()`, `get_path()` — work correctly
- `PaginatedList<T>` generic struct in `src/models/mod.rs` — reuse for pagination
- `DEFAULT_PAGE_SIZE = 25` — reuse
- Home page DataTable pattern — reuse sorting/pagination pattern from `src/routes/home.rs`

### Key Query Pattern

```sql
SELECT v.id, v.label,
       t.id as title_id, t.title, t.media_type,
       COALESCE(g.name, '') as genre_name,
       COALESCE(vs.name, '') as condition_name,
       (SELECT c.name FROM title_contributors tc
        JOIN contributors c ON tc.contributor_id = c.id
        JOIN contributor_roles cr ON tc.role_id = cr.id
        WHERE tc.title_id = t.id AND tc.deleted_at IS NULL AND c.deleted_at IS NULL AND cr.deleted_at IS NULL
        ORDER BY CASE WHEN cr.name = 'Auteur' THEN 0 ELSE 1 END, tc.id ASC
        LIMIT 1) as primary_contributor,
       (CASE WHEN l.id IS NOT NULL THEN 1 ELSE 0 END) as is_on_loan
FROM volumes v
JOIN titles t ON v.title_id = t.id AND t.deleted_at IS NULL
LEFT JOIN genres g ON t.genre_id = g.id AND g.deleted_at IS NULL
LEFT JOIN volume_states vs ON v.condition_state_id = vs.id AND vs.deleted_at IS NULL
LEFT JOIN loans l ON v.id = l.volume_id AND l.returned_at IS NULL AND l.deleted_at IS NULL
WHERE v.location_id = ? AND v.deleted_at IS NULL
ORDER BY {validated_sort} {validated_dir}
LIMIT 25 OFFSET ?
```

**Sort whitelist:** `["title", "primary_contributor", "genre_name"]`. Use `validated_sort()` pattern from title.rs. Map: title→t.title, primary_contributor→primary_contributor, genre_name→genre_name.

**Pagination:** Full-page links `/location/{id}?sort=title&dir=asc&page=2` (not HTMX fragments).

### MariaDB Type Gotchas (from CLAUDE.md)

- JSON → CAST(col AS CHAR)
- BIGINT UNSIGNED NULL → CAST(col AS SIGNED), read as Option<i64>
- Never CAST AS UNSIGNED in SELECT

### Breadcrumb Pattern

Build from `LocationModel::get_path()` — but that returns a flat string. For linked breadcrumbs, need to walk the parent chain and return `Vec<(u64, String)>` (id + name pairs).

Add `get_path_segments(pool, id) -> Vec<(u64, String)>` to LocationModel that returns the chain as structured data instead of a joined string.

### References

- [Source: _bmad-output/planning-artifacts/prd.md#FR24, #FR26]
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#UX-DR11, #UX-DR12]
- [Source: src/routes/home.rs — DataTable sorting/pagination pattern]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### Change Log

### File List
