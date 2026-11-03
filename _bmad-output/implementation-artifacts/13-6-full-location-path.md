# Story 13.6: The full location of a volume in every list

Status: ready-for-dev

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

- [ ] T1 — `all_paths` + shared assembly (AC1)
- [ ] T2 — title detail service/template (AC2)
- [ ] T3 — other listings (AC3)
- [ ] T4 — tests; gates; full suites once before the single push
