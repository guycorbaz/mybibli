# Story 13.6: The full location of a volume in every list

Status: review

GitHub issue: #582 (type:change-request). Covers: FR160. Epic 13; ships in v1.23.0.

## Story

As a librarian looking for a volume,
I want its location shown as a full path,
so that "Shelf 3" is never ambiguous.

## Existing-code reality check (code map, 2026-11-03)

| Place | Today |
|---|---|
| `src/models/volume.rs` ~l. 407–442 (volumes of a title) | joins `storage_locations` for `sl.name AS location_name, sl.label AS location_label` — the leaf only |
| `src/services/title_detail.rs:62/109` | carries `location_name` to the template |
| `templates/pages/title_detail.html:317` (table) and `:365` (cards) | render `location_name` + `(L-code)` |
| `src/models/location.rs::get_path` (l. 174) / `get_path_segments` (l. 368) | walk up the parents, **one query per level** — fine for one volume, not for a list |
| `templates/pages/volume_detail.html:79` | already shows the full `location_path` |

Other volume listings to check at story start (grep `storage_locations` joins and `location` columns in templates): loans list, audit list (`templates/pages/audit_list.html`), location contents, borrower loans, search/browse if they show a location, MCP `get_title` volumes.

## Acceptance Criteria

1. **AC1 — one path builder, no N+1.** A `LocationModel::all_paths(pool) -> HashMap<u64, String>` loads the live `storage_locations` (id, parent_id, name) in **one** query and builds every path in memory with the same separator and depth guard as `get_path` (Rule #1: `get_path` and the new function share the in-memory assembly; a cycle or a missing parent stops the walk as today).
2. **AC2 — title page.** The volume table and cards show the full path (L-code kept), e.g. `Maison › Salon › Bibliothèque › Étagère 3 (L0042)`.
3. **AC3 — other listings.** Every other place listing volumes with a location shows the full path the same way; list them in the completion notes.
4. **AC4 — readability.** A long path wraps on narrow screens (no horizontal scroll), keeps a `title` attribute or equivalent only if truncated; axe-core passes.
5. **AC5 — tests.** Unit: path assembly (nested, root, missing parent, cycle guard). DB integration: `all_paths` on a 3-level tree, deleted node excluded. E2E (specId `LP`): a volume shelved on a 3-level location shows the full path on its title page.

## Tasks / Subtasks

- [x] T1 — `all_paths` + shared assembly (AC1)
- [x] T2 — title detail service/template (AC2)
- [x] T3 — other listings (AC3)
- [x] T4 — tests; gates; full suites once before the single push

## Completion Notes

**T1 — one path builder (AC1).** `src/models/location.rs` gains the pure in-memory walk `assemble_path(&PathNodes, id)` (same `" → "` separator, now `PATH_SEPARATOR`; same 20-level `PATH_MAX_DEPTH` cycle guard with its warning; stops at a root, at a missing/soft-deleted parent, empty for an unknown id), `join_path`, `paths_for_all`, and the loader `load_path_nodes` (ONE query on live `storage_locations`). `LocationModel::all_paths(pool) -> HashMap<u64, String>` is new; `get_path` and `get_path_segments` now run on the same loader + assembly (one query instead of one per level), so there is a single implementation (Rule #1). Every existing caller (scan feedback, loan return, volume detail, location breadcrumb) keeps its signature.

**T2 — title page (AC2).** `VolumeModel::find_by_title` also selects the location id and fills a new `VolumeWithLocation::location_path` from one `all_paths` call (skipped when no volume is shelved). `templates/pages/title_detail.html` table and cards render the path + `(L-code)`; `break-words` (+ `min-w-0` on the card `dd`) lets a long path wrap. Nothing is truncated, so no `title` attribute.

**T3 — other listings (AC3).**
- Audit list (`src/routes/audit.rs`, `templates/pages/audit_list.html`): already showed the full path but walked the parents per row (N+1) — now one `all_paths` lookup; `break-words` added.
- Label drill-down volumes (`src/models/label.rs::list_volumes_for`, `templates/pages/label_detail.html`): showed the leaf only — now the full path; `break-words` added.
- `/api/v1/titles/{id}` and MCP `get_title` (`src/services/title_detail.rs`): new additive field `location_path` on each volume; `location_name` (leaf) and `location_label` unchanged so the API contract does not break.
- Volume detail: already showed the full path (now built by the shared assembly).
- Location contents (`/location/{id}`): lists the volumes of ONE location only (`WHERE v.location_id = ?`, no descendants) under a full breadcrumb — a per-row path would repeat the breadcrumb; left unchanged on purpose.
- Loans list, borrower loans, search/browse: no location column — nothing to change.
- Audit list and label drill-down did not show an L-code before and still do not (their queries carry no label; out of scope for a path change).

**T4 — tests.** Unit (location.rs): nested, root, missing parent, unknown id, cycle bound, `paths_for_all`. DB: `all_paths_builds_a_three_level_tree_and_skips_deleted_nodes` (also checks `get_path` / `get_path_segments` agree), `find_by_title_carries_the_full_location_path` + path assertions in the existing `find_by_title` tests, `list_volumes_for_carries_the_full_location_path`, `location_path` assertion in `tests/mcp_catalog_read_tools.rs`. E2E: `tests/e2e/specs/journeys/location-full-path.spec.ts` (3-level tree, shelve, full path + L-code in the table, mobile card wraps without horizontal scroll, axe on both); new helper `createChildLocation` in `tests/e2e/helpers/locations.ts`.

**Deviations.** E2E specId is `FP`, not `LP`: `LP` is already used by `labels-page.spec.ts`. Codes randomised in unused bands L6000–L6999 / V7000–V7999. Separator stays `" → "` (AC1 "same separator as `get_path`"), not the `›` of the AC2 example.

**File sizes.** No touched file above 2000 lines (largest: `src/models/volume.rs`, 1343). `src/routes/titles.rs` (2753, pre-existing) was not touched.
