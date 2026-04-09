# Story 5.5: BD Omnibus Multi-Position Volume

Status: done

## Story

As a librarian,
I want to register a BD omnibus as a volume covering multiple positions in a series,
so that my gap detection accurately reflects reality when I own an omnibus instead of individual issues.

## Acceptance Criteria

1. **Omnibus position range:** Given a title assigned to a series, when librarian marks it as "omnibus", then they can specify a position range (e.g., positions 1-3) instead of a single position
2. **Gap grid filled:** Given an omnibus volume covering positions [5,6,7] in a series, when `/series/{id}` renders the gap grid, then positions 5, 6, 7 all display as filled
3. **Omnibus click:** Given a filled square backed by an omnibus, when clicked, then it navigates to the omnibus volume's title detail
4. **Idempotent overlap:** Given a series where the same position is covered by both an individual title and an omnibus, when rendered, then both contribute to "filled" (idempotent, no error)
5. **Unit test:** Gap calculation with mixed individual + omnibus assignments
6. **E2E:** Create series -> add omnibus covering 3 positions -> verify grid filled

## Tasks / Subtasks

- [ ] Task 1: Extend assignment to support omnibus (AC: #1)
  - [ ] 1.1 Add `assign_omnibus(pool, title_id, series_id, start_position, end_position)` method to `TitleSeriesModel` — creates N rows in `title_series` (one per position in range), all with `is_omnibus = TRUE`. Uses existing UNIQUE constraint to prevent duplicates.
  - [ ] 1.2 Handle the soft-deleted row restoration pattern for each position in the range (same as single assign)
  - [ ] 1.3 Add position range validation: start >= 1, end >= start. For closed series: end <= total_volume_count. If start == end, delegate to existing single-position `assign_title()` (is_omnibus=FALSE).
  - [ ] 1.4 Add `SeriesService::assign_omnibus(pool, title_id, series_id, start, end)` wrapping the model method with validation
  - [ ] 1.5 Add `TitleSeriesModel::unassign_all_for_title_in_series(pool, title_id, series_id)` — soft-deletes ALL rows for a (title_id, series_id) pair. Used when removing an omnibus assignment from the title detail page.
- [ ] Task 2: Update gap detection for omnibus (AC: #2, #4, #5)
  - [ ] 2.1 The existing `build_position_grid()` already works — each omnibus position has its own row in `title_series`, so the grid fills correctly. No code change needed for gap detection itself.
  - [ ] 2.2 Verify that overlapping assignments (individual + omnibus on same position) work correctly — the query `find_by_series` returns all rows, `build_position_grid` uses `.find()` which returns the first match. This is idempotent (AC#4).
  - [ ] 2.3 Unit tests: gap calculation with omnibus [5,6,7] in a 10-position series; mixed individual + omnibus on same position
- [ ] Task 3: Update title detail assignment UI (AC: #1)
  - [ ] 3.1 Add "omnibus" checkbox and "end position" field to the assignment form in `templates/pages/title_detail.html`. Show end position only when omnibus is checked (JS toggle).
  - [ ] 3.2 Extend existing `POST /title/{id}/series` to accept optional `end_position` parameter. If end_position is present and > start, call `assign_omnibus()`. Otherwise call single `assign_title()`.
  - [ ] 3.3 Add i18n keys: `series.omnibus`, `series.start_position`, `series.end_position`, `series.omnibus_assigned`, `series.positions_range`
  - [ ] 3.4 **Modify `find_by_title()` to GROUP omnibus rows**: Instead of returning 3 separate rows for positions 5,6,7, return a single `TitleSeriesAssignment` with `position_start: i32, position_end: Option<i32>, is_omnibus: bool`. Group consecutive rows with same (title_id, series_id, is_omnibus=TRUE) into ranges.
  - [ ] 3.5 **Update title detail template**: Show grouped omnibus as "Positions 5-7" with a single remove button that calls `POST /title/{id}/series-remove?series_id={sid}` (removes all rows for that title+series pair via `unassign_all_for_title_in_series()`). Keep individual assignments showing "(#N)" with per-row remove as today.
- [ ] Task 4: Visual distinction for omnibus squares in gap grid (AC: #3)
  - [ ] 4.1 In `SeriesPositionInfo`, add `is_omnibus: bool` field
  - [ ] 4.2 Update `build_position_grid()` to set `is_omnibus` from the `title_series.is_omnibus` column
  - [ ] 4.3 Optionally add a subtle visual indicator on omnibus squares in `series_gap_grid.html` (e.g., a small "O" badge or different shade). All omnibus squares link to the same `/title/{id}`.
- [ ] Task 5: E2E test (AC: #6)
  - [ ] 5.1 Extend `tests/e2e/specs/journeys/series.spec.ts`: create closed series (total=8) → create title → assign as omnibus positions 3-5 → verify gap grid shows positions 3,4,5 filled and others missing
- [ ] Task 6: Verification
  - [ ] 6.1 `cargo clippy -- -D warnings` passes
  - [ ] 6.2 `cargo test` all green
  - [ ] 6.3 Full E2E suite passes

## Dev Notes

### Design Decision: Multiple Rows vs. New Table

The epic specifies "add `volume_series_positions` link table". However, the **existing schema already supports omnibus** without a new table:

- `title_series` has an `is_omnibus BOOLEAN NOT NULL DEFAULT FALSE` column (already in migration)
- The UNIQUE constraint `(title_id, series_id, position_number)` allows the same `(title_id, series_id)` with different `position_number` values
- For an omnibus covering positions [5,6,7], create 3 rows:
  ```sql
  INSERT INTO title_series (title_id, series_id, position_number, is_omnibus) VALUES
    (42, 7, 5, TRUE),
    (42, 7, 6, TRUE),
    (42, 7, 7, TRUE);
  ```
- The existing `build_position_grid()` already fills each position individually — **no change needed for gap detection**
- The gap grid template already links each filled square to `/title/{id}` — omnibus squares link to the same title, which is correct

**Advantages of this approach:**
- No new migration
- No new table to manage
- Existing gap detection works as-is
- Existing gap grid template works as-is (AC#2 already satisfied)
- AC#4 (idempotent overlap) works naturally — multiple rows for same position are handled by UNIQUE constraint

**What's actually new:**
- `assign_omnibus()` method that creates N rows with `is_omnibus = TRUE`
- UI extension: checkbox + end position field on title detail form
- Optional visual indicator for omnibus squares
- Position range validation

### Existing Code State (from Story 5-4)

**TitleSeriesModel** (`src/models/series.rs`):
- `assign(pool, title_id, series_id, position_number)` — single position, is_omnibus always FALSE
- `unassign(pool, id, title_id)` — soft-deletes one row with title_id verification
- `find_by_series(pool, series_id)` — returns all assignments with title info, ordered by position

**SeriesService** (`src/services/series.rs`):
- `assign_title()` — validates position >= 1, position <= total for closed series
- `get_series_positions()` — builds full grid for closed series, list for open
- `build_position_grid()` — iterates 1..=total, finds matching assignment per position

**SeriesPositionInfo** struct:
```rust
pub struct SeriesPositionInfo {
    pub position: i32,
    pub title_id: Option<u64>,
    pub title_name: Option<String>,
}
```
Needs: `is_omnibus: bool` field added.

**TitleSeriesRow** struct:
```rust
pub struct TitleSeriesRow {
    pub id: u64,
    pub title_id: u64,
    pub series_id: u64,
    pub position_number: i32,
    pub title_name: String,
    pub media_type: String,
}
```
Does NOT include `is_omnibus` — needs adding to the SELECT query and struct.

### Omnibus Assignment Flow

1. User opens title detail page → sees series section
2. Checks "Omnibus" checkbox → "End position" field appears
3. Selects series, enters start position (e.g., 3) and end position (e.g., 5)
4. Submits form → `assign_omnibus()` creates 3 rows with `is_omnibus = TRUE`
5. Navigates to series detail → gap grid shows positions 3,4,5 filled

### Edge Case: start == end

If start_position == end_position (e.g., 5 to 5), treat as a single-position assignment with `is_omnibus = FALSE`. Delegate to the existing `assign_title()` method. Only create omnibus rows (is_omnibus=TRUE) when end > start.

### Display Grouping for Omnibus on Title Detail

`find_by_title()` currently returns flat rows. For omnibus display, the service layer (or a post-processing step in the route handler) must group consecutive `is_omnibus=TRUE` rows with the same (title_id, series_id) into a single `TitleSeriesAssignment` entry:

```
// Before grouping (3 rows from DB):
{ series_id: 7, position: 5, is_omnibus: true }
{ series_id: 7, position: 6, is_omnibus: true }
{ series_id: 7, position: 7, is_omnibus: true }

// After grouping (1 entry for UI):
{ series_id: 7, position_start: 5, position_end: 7, is_omnibus: true }
```

The grouped entry shows "Positions 5-7" in the UI with a single remove button that calls `unassign_all_for_title_in_series(pool, title_id, series_id)`.

### Unassign Omnibus

When unassigning an omnibus title, ALL position rows for that (title_id, series_id) should be soft-deleted. Add a method:
```rust
pub async fn unassign_all_for_title_in_series(pool, title_id, series_id) -> Result<u64, AppError>
```
This soft-deletes all `title_series` rows where `title_id = ? AND series_id = ? AND deleted_at IS NULL`.

The UI on the title detail page should show omnibus assignments as "Positions 3-5" and have a single remove button that removes all positions.

### E2E Test Strategy

Extend existing `series.spec.ts`:
1. Create closed series (total=8)
2. Create title via scan + home search (same pattern as story 5-4)
3. Navigate to title detail → check "Omnibus" → fill start=3, end=5 → submit
4. Navigate to series detail → verify 3 filled cells at positions 3,4,5
5. Verify other positions (1,2,6,7,8) are missing

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Story 5.5 AC, FR40]
- [Source: migrations/20260329000000_initial_schema.sql — Lines 190-206 (title_series with is_omnibus)]
- [Source: src/models/series.rs — TitleSeriesModel, TitleSeriesRow]
- [Source: src/services/series.rs — build_position_grid, SeriesPositionInfo]
- [Source: templates/components/series_gap_grid.html — gap grid component]
- [Source: _bmad-output/implementation-artifacts/5-4-title-series-assignment-and-gap-detection.md — Dev Notes]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Design decision: used existing `title_series` table with `is_omnibus=TRUE` + multiple rows per position. No new migration needed.
- Added `assign_omnibus()` to TitleSeriesModel — creates N rows for positions start..=end
- Added `unassign_all_for_title_in_series()` for bulk omnibus removal
- Updated `find_by_title()` to GROUP consecutive omnibus rows into ranges (e.g., "#3-5")
- Updated `find_by_series()` and `TitleSeriesRow` to include `is_omnibus` field
- Updated `SeriesPositionInfo` with `is_omnibus` for gap grid rendering
- Extended `assign_to_series` form to accept optional `end_position` + `omnibus` checkbox
- Added `deserialize_optional_i32` as public function (reused from series routes)
- Title detail template shows grouped omnibus as "Positions 3-5" with bulk remove
- Template has separate remove forms: per-row for individual, by-series_id for omnibus
- Omnibus validation: start >= 1, end >= start, end <= total for closed series
- If start == end with omnibus checked, delegates to single-position assign
- 2 new unit tests: omnibus gap grid + overlap individual+omnibus (idempotent)
- E2E: omnibus test creates series (total=8), assigns positions 3-5, verifies gap grid

### File List

**Modified:**
- `src/models/series.rs` — Added `is_omnibus` to TitleSeriesRow, grouped TitleSeriesAssignment, assign_omnibus(), unassign_all_for_title_in_series()
- `src/services/series.rs` — Added assign_omnibus(), unassign_all_from_series(), is_omnibus in SeriesPositionInfo + build_position_grid, 2 unit tests
- `src/routes/series.rs` — Made deserialize_optional_i32 public
- `src/routes/titles.rs` — Extended AssignToSeriesForm with end_position/omnibus, added unassign_omnibus_from_series handler
- `src/routes/mod.rs` — Added /title/{id}/series-remove route
- `templates/pages/title_detail.html` — Omnibus checkbox, end position field, grouped display, separate remove forms
- `locales/en.yml` — Added series.omnibus, series.omnibus_assigned, fixed series.missing_volume
- `locales/fr.yml` — Same (French)
- `tests/e2e/specs/journeys/series.spec.ts` — Added omnibus E2E test, fixed home search approach (URL params vs form submit)
