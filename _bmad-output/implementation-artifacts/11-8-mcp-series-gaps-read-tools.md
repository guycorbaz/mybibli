# Story 11.8: MCP series and gaps read tools

Status: ready-for-dev

GitHub issue: part of #493 (MCP server — do NOT close it here); resolves #516 (split `src/models/title.rs`). Covers: FR129. Epic 11 — eighth story; builds on 11-6 (`/api/mcp`, `McpTool` trait) and 11-7 (`search_titles`, shared `tools/args.rs`, `ListPage`, `tests/common/mcp.rs`, `helpers/mcp.ts`). 11-10 adds expected tomes to `get_series`.

## Story

As an AI agent,
I want to see series, their gaps and the incomplete titles,
so that I know what to look for on the web.

## Existing-code reality check (code map, 2026-10-28, `main` 870609a)

| Place | Today | Consequence |
|---|---|---|
| `src/models/title.rs` | 1942 lines (#516); search projection (`SearchResult`, sort whitelist, `active_search`, `list_recent_active`) at lines ~1083–1423 | T0: move that block and its unit tests to `src/models/title_search.rs` (behaviour-preserving, own commit, `pub use` keeps `models::title::SearchResult` paths valid); the new incomplete-metadata query lives there |
| `src/models/title.rs::active_search` | `no_cover_only` filter (CR #355: `cover_image_url IS NULL OR ''`, codes not required), `media_type`, `genre_id`, order `title, id` | `list_titles_without_cover` reuses it through `CatalogSearch` (new `no_cover_only` field) — no new SQL |
| `src/models/series.rs` | `SeriesModel::{active_find_by_id, active_list(page)}`; `count_with_gaps` / `list_with_gaps(limit)` — closed series, `total > 0`, filled = `COUNT(DISTINCT position_number)` over live `title_series` × live `titles`, `position_number > 0`; gap = `total - filled`; `TitleSeriesModel::find_by_series` (live rows, ordered by position, title name, media type, cover, primary contributor) | new `SeriesModel::list_summaries(pool, name_query, gaps_only, page)` reusing the same "filled" derived table (one shared SQL fragment for the three queries, Rule #1) |
| `src/services/series.rs` | `get_series_positions` builds a full 1..total grid (web page) | new pure `series_gaps(series, rows)` + `SeriesService::{list_summaries, agent_series_detail}`; the web grid is untouched |
| `services::series` assign rules | a position must be ≥ 1 and ≤ total for a closed series | positions > total only arise from legacy data; gap formula stays the dashboard's |
| `series.total_volume_count` | `INT NULL`, no upper bound | `missing_positions` capped (1000) with a `missing_positions_truncated` flag |
| `src/mcp/tools/search_titles.rs` | private `media_type` parsing and genre resolution, private `Item` DTO | extract `media_type` / genre helpers into `tools/args.rs` / `tools/mod.rs` and share the title `Item` (Rule #1) |
| `src/mcp/tools/mod.rs` | registry test pins `{get_title, search_contributors, search_titles}` | pins the seven tools |
| `tests/e2e/specs/journeys/mcp-read-tools.spec.ts` | asserts the exact `tools/list` set | updated to the seven names |

## Acceptance Criteria

1. **AC1 — `list_series`.** Input `{ query?: string (≤ 200 chars, name substring, case-insensitive, LIKE wildcards escaped), gaps_only?: boolean, page?: 1..=10000 }`. Output `ListPage` of `{ id, name, series_type: "open"|"closed", total_volume_count (null for open), owned_positions, gap_count (null for an open series or a closed one without total) }` sorted by name then id. `owned_positions` and `gap_count` use exactly the `list_with_gaps` rule (closed, `total > 0`, distinct positions > 0 held by live titles through live assignments; `gap_count = max(total - owned, 0)`); `gaps_only = true` keeps the series with `gap_count > 0`.
2. **AC2 — `get_series`.** Input `{ id: integer ≥ 1 }`. Output `{ id, name, description, series_type, total_volume_count, owned_positions, gap_count, missing_positions: [int], missing_positions_truncated: bool, positions: [{ position, title_id, title, media_type, primary_contributor, is_omnibus, has_cover }] }`. `positions` lists every live assignment row (an omnibus contributes one row per covered position, `is_omnibus: true`) ordered by position then title id. `missing_positions` = the positions 1..=total of a closed series not held (at most 1000 listed, ascending; the flag says when the list was cut); `[]` for an open series. Unknown or soft-deleted id → `isError` "not found". No expected tomes (11-10).
3. **AC3 — `list_titles_without_cover`.** Input `{ media_type?, genre?, page? }` (same validation as `search_titles`). Output: the `search_titles` item shape, titles whose `cover_image_url` is NULL or empty (same predicate as the web "no cover" filter), sorted by title then id.
4. **AC4 — `list_titles_with_incomplete_metadata`.** Input `{ missing?: "description"|"publisher"|"contributor"|"publication_year", media_type?, genre?, page? }`. A title is incomplete when its description is NULL/blank, its publisher is NULL/blank, it has no live contributor (live junction row, live contributor, live role — the `get_title` rule) or its `publication_date` is NULL. Without `missing`, every incomplete title; with it, only the titles missing that field. Output `ListPage` of `{ id, title, subtitle, media_type, genre_name, isbn, primary_contributor, publication_date, has_cover, missing: [field…] }` (fields in the fixed order above), sorted by title then id.
5. **AC5 — soft delete.** Soft-deleted series, titles, assignments, contributors, junction rows and roles never appear or count.
6. **AC6 — pagination.** Fixed page size 25, `page` 1..=10000 (11-7 rules); a page past the end returns `items: []` with true totals.
7. **AC7 — thin adapters (AR27).** No SQL under `src/mcp/` (`no_sql_under_src_mcp` stays green); tools call `services/`.
8. **AC8 — self-describing.** Title, description (> 200 chars, states read-only, paging, output, how gaps are computed / what "incomplete" means), `description` on every schema property, `additionalProperties: false`; registry test pins the seven tools.
9. **AC9 — invalid arguments** (unknown property, wrong type, `page = 0`, `id = 0`, unknown `missing` value, unknown media type / genre, over-long string) → `isError: true` with an actionable message.
10. **AC10 — tests.** Integration (`tests/mcp_series_gaps_tools.rs`, full router, isolated DB): a closed series with gaps (+ an omnibus, a soft-deleted title, a soft-deleted assignment, a position held twice), an open series, a soft-deleted series; `list_series` (counts, `gaps_only`, query, paging), `get_series` (positions, gaps, open series, unknown / deleted id), `list_titles_without_cover` (cover / empty string / NULL, soft-deleted excluded, media-type filter), `list_titles_with_incomplete_metadata` (each field, filter, complete title absent, soft-deleted contributor counts as missing). Unit tests: the gap computation (cap, open series, out-of-range positions), argument parsing, the `missing` enum. E2E `tests/e2e/specs/journeys/mcp-gaps-tools.spec.ts` (spec id `MG`): `loginAs(admin)`, create a closed series of 3 through the UI, scan a title and assign it at position 2, create a `propose` key, `tools/list` has the seven tools, `get_series` reports missing `[1, 3]`, `list_series` with `gaps_only` lists it, `list_titles_with_incomplete_metadata` / `list_titles_without_cover` answer, key revoked in `finally`.

## Tasks / Subtasks

- [ ] T0 — (#516) Move the search projection out of `src/models/title.rs` into `src/models/title_search.rs` (+ its unit tests), re-exported; no behaviour change. Own commit.
- [ ] T1 — Models: shared "filled positions" SQL fragment in `models/series.rs`, `SeriesModel::list_summaries`; `TitleModel::list_incomplete_metadata` + `MetadataField` enum in `title_search.rs`.
- [ ] T2 — Services: `services::series::{SeriesGaps, series_gaps, SeriesService::list_summaries, SeriesService::agent_series_detail}`; `CatalogSearch.no_cover_only`; `services::search::SearchService::list_incomplete_metadata` (thin pass-through).
- [ ] T3 — Tools: `list_series.rs`, `get_series.rs`, `list_titles_without_cover.rs`, `list_titles_with_incomplete_metadata.rs`; shared media-type / genre helpers and title item; registry + tests.
- [ ] T4 — Integration tests `tests/mcp_series_gaps_tools.rs`.
- [ ] T5 — E2E `mcp-gaps-tools.spec.ts`; `mcp-read-tools.spec.ts` tool list updated.
- [ ] T6 — Docs: `docs/route-role-matrix.md` § MCP lists the seven tools.
- [ ] T7 — Gates (brief §5).

## Dev Notes

- AR27: `tools/*.rs` → `services/*` → `models/*`; DTOs and pure mapping only in tools.
- Gap rule = `list_with_gaps`; computing it in Rust from `find_by_series` rows gives the same number (same live filters, `position > 0`, distinct positions). Out-of-range positions (> total, legacy data only) count as held, as on the dashboard, so `gap_count` can then be smaller than the number of `missing_positions` — documented, unit-tested.
- Incomplete predicates are fixed SQL fragments chosen from a Rust enum (never interpolated user text). Blank = `TRIM(col) = ''`.
- No migration, no setting, no route, no dependency, no locale key (agent-facing English, as in 11-7).
- Wire traps (11-6/11-7): `Host: localhost`, `Accept: application/json, text/event-stream`; `structuredContent` carries the JSON.
- Tests: `#[sqlx::test(migrations = "./migrations")]`, raw SQL seeding, `tests/common/mcp.rs`. E2E: `loginAs`, `specIsbn("MG", n)`, fresh request context for the agent, no `waitForTimeout`; full run on a freshly reset stack.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.8] — FR129.
- [Source: _bmad-output/implementation-artifacts/11-7-mcp-catalog-read-tools.md] — conventions; D-11-6-*, D-11-7-* in `epic-11-autonomous-decisions.md`.
- [Source: src/models/series.rs#list_with_gaps] — gap rule (story 9-6).

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-10-28 — story created (create-story).
