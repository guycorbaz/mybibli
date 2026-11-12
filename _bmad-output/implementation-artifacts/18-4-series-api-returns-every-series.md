# Story 18.4: The series API returns every series

Status: done

GitHub issue: #599 (closes). Covers: fix. Epic 18; ships in v1.28.0. Sprint-status key: `18-4-series-api-returns-every-series`.

## Story

As a script or tool reading the API,
I want `GET /api/v1/series` to return all the series,
so that nothing past the 25th is silently missing.

## Context

`src/routes/api_v1.rs::list_series_endpoint` answers with `SeriesModel::active_list(pool, 1)` — page 1 (25 rows, `DEFAULT_PAGE_SIZE`) of the paginated list behind `/series` — with a comment "v1: return up to one page … Future patch". A library with more than 25 series loses every series after the 25th by name. The sibling reference-data endpoints (`/api/v1/genres` → `GenreModel::list_active`, `/api/v1/locations` → `LocationModel::find_all_tree`) already return complete arrays; the manual (ch. 11, "GET /api/v1/genres, /locations, /series") describes the three as reference data.

## Decisions

- **D1 — all at once, no pagination.** Same contract as genres / locations: a JSON array of every live series, ordered by name then id. Single-tenant library, reference data of a few hundred rows at most; adding a `page` parameter would change the response shape for existing scripts and diverge from the sibling endpoints. Response fields unchanged (`id`, `name`, `description`, `series_type`, `total_volume_count`).
- **D2 — a dedicated model read.** `SeriesModel::list_active_all(pool)` — same columns and row mapping as `active_list` (`row_to_series`), `deleted_at IS NULL`, `ORDER BY name ASC, id ASC`, no `LIMIT`. `active_list` (the paginated `/series` page) is untouched.
- **D3 — docs.** The manual API section (EN + FR) states that the three reference endpoints return every live row, unpaginated; the PDF rebuild is done once in 18-7.

## Acceptance Criteria

1. **AC1.** With more than 25 live series, `GET /api/v1/series` returns all of them, ordered by name; soft-deleted series are absent.
2. **AC2.** The response fields and the array shape are unchanged; auth unchanged (`ApiKeyAuth`, any scope).
3. **AC3.** DB integration test: 30 live series + 1 deleted → 30 returned, in name order, deleted one absent; plus a 401 without key.
4. **AC4.** E2E: the spec creates 27 series under a per-run unique prefix (Rule #23), an admin creates a read key, `GET /api/v1/series` with it lists all 27; the key is revoked at the end.
5. **AC5.** Manual EN + FR updated (D3).

## Tasks / Subtasks

- [x] T1 — `SeriesModel::list_active_all` + endpoint (AC1, AC2)
- [x] T2 — `tests/api_v1_series.rs` (AC3)
- [x] T3 — E2E `tests/e2e/specs/journeys/series-api.spec.ts` (AC4)
- [x] T4 — manual ch. 11 EN + FR (AC5)
- [x] T5 — gates

## Dev Notes

- Pattern for > 25 series in E2E: `series-selector-recent-table.spec.ts` (`createSeries` via `page.request.post("/series")` with the CSRF token, `zzz-` prefix so a parallel spec's paginated `/series` list keeps its rows on page 1). API key: `helpers/api-keys.ts::createApiKey(page, "read")` / `revokeApiKey`.
- Integration harness: `tests/api_v1_auth.rs` (`state_with_pool`, `seed_key`, router `oneshot`).
- No new SQL macro (`sqlx::query` dynamic, like `active_list`) → no `.sqlx` change.
- Gates: lighter test build if the station is loaded (`CARGO_PROFILE_{DEV,TEST}_DEBUG=line-tables-only`); E2E on 18080; one push after the 18-3 merge (done).

### References

- Issue #599; `src/routes/api_v1.rs:235-268`; `src/models/series.rs:132-178`; `docs/manual/{en,fr}/11-api-integrations.tex`

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (`claude-opus-5-5`)

### Completion Notes List

- **T1** — `SeriesModel::list_active_all` (same columns and `row_to_series` as `active_list`, `deleted_at IS NULL`, `ORDER BY name ASC, id ASC`, no limit); `list_series_endpoint` uses it. `active_list` untouched. Dynamic `sqlx::query` → no `.sqlx` change (`prepare --check` green).
- **T2** — `tests/api_v1_series.rs` (shared `common::mcp::{router, seed_key}`, DRY): 30 live series inserted in reverse name order + one soft-deleted → the 30, in name order, fields unchanged; no key → 401 `missing_api_key`. **Regression proven:** with the old endpoint the first test fails with exactly 25 names ("Série 01" … "Série 25").
- **T3** — E2E `series-api.spec.ts`: 27 series under a per-run `zzz-api-<base36 time>` prefix, read key created through the UI, `GET /api/v1/series` lists the spec's 27 in order, key revoked in `finally`. First named `api-series.spec.ts`: it then ran before `classification-fieldset`, `dewey-code` and `empty-states`, which assume no series exists (#528 — the last title-page submit button becomes "Add to series"; the empty `/series` list), and broke 5 tests. Renamed to sort after them (the existing D-11-11-6 convention, comment in the file); #528 itself stays open.
- **T4** — manual ch. 11 EN + FR: the three reference endpoints return every live row, unpaginated, series by name; "since 1.28.0" for `/series`. PDFs rebuilt in 18-7.
- **T5 — Gates.** clippy `--all-targets -D warnings` clean; `sqlx prepare --check` green; full `cargo test` with DB: 2729 passed, 0 failed; E2E on a fresh stack, `CI=true --workers=2`: 477 passed, 2 skipped, 0 failed, 0 flaky.

### File List

- `src/models/series.rs` (+ `series_select!` column head shared by its four full-row reads), `src/routes/api_v1.rs`
- `tests/api_v1_series.rs` (new), `tests/e2e/specs/journeys/series-api.spec.ts` (new)
- `docs/manual/en/11-api-integrations.tex`, `docs/manual/fr/11-api-integrations.tex`
- `_bmad-output/implementation-artifacts/18-4-series-api-returns-every-series.md`, `sprint-status.yaml`

### Review Findings

Code review 2026-11-12 (one reviewer, three passes: adversarial, edge cases, acceptance). No High, no Medium.

- [x] [Review][Patch] Column list copied a third time (Low, Rule #1) — `series_select!()` macro (a literal, for `concat!`) heads the four full-row reads of `series` in `models/series.rs`.
- [x] [Review][Patch] Library-wide length assertion in the E2E (Low, Rule #23 spirit) — removed; `toEqual(names)` on the spec's own rows carries the regression.
- [x] [Review][Patch] `id` tie-break not exercised (Low) — `namesakes_are_ordered_by_id` (no UNIQUE on `series.name`).
- [x] [Review][Dismiss] Story record out of date — it was being written while the review ran; T3 names `series-api.spec.ts`, the rename is explained.

After the patches: clippy clean; every series integration test file (`api_v1_series`, `mcp_series_*`, `proposal_accept_series`, `proposal_series_update`, `series_delete_modal`, `series_list_all_names`) green, `--lib series` 110 passed; `tsc --noEmit` green. The E2E change only removes an assertion. Review clean.
