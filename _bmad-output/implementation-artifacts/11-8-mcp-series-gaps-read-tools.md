# Story 11.8: MCP series and gaps read tools

Status: done

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

- [x] T0 — (#516) Move the search projection out of `src/models/title.rs` into `src/models/title_search.rs` (+ its unit tests), re-exported; no behaviour change. Own commit.
- [x] T1 — Models: shared "filled positions" SQL fragment in `models/series.rs`, `SeriesModel::list_summaries`; `TitleModel::list_incomplete_metadata` + `MetadataField` enum in `title_search.rs`.
- [x] T2 — Services: `services::series::{SeriesGaps, series_gaps, SeriesService::list_summaries, SeriesService::agent_series_detail}`; `CatalogSearch.no_cover_only`; `services::search::SearchService::list_incomplete_metadata` (thin pass-through).
- [x] T3 — Tools: `list_series.rs`, `get_series.rs`, `list_titles_without_cover.rs`, `list_titles_with_incomplete_metadata.rs`; shared media-type / genre helpers and title item; registry + tests.
- [x] T4 — Integration tests `tests/mcp_series_gaps_tools.rs`.
- [x] T5 — E2E `mcp-gaps-tools.spec.ts`; `mcp-read-tools.spec.ts` tool list updated.
- [x] T6 — Docs: `docs/route-role-matrix.md` § MCP lists the seven tools.
- [x] T7 — Gates (brief §5).

## Dev Notes

- AR27: `tools/*.rs` → `services/*` → `models/*`; DTOs and pure mapping only in tools.
- Gap rule = `list_with_gaps`; computing it in Rust from `find_by_series` rows gives the same number (same live filters, `position > 0`, distinct positions). Out-of-range positions (> total, legacy data only) count as held, as on the dashboard, so `gap_count` can then be smaller than the number of `missing_positions` — documented, unit-tested.
- Shared "filled positions" derived table: a `macro_rules!` returning the literal, spliced with `concat!` into `count_with_gaps`, `list_with_gaps` and `list_summaries` (sqlx takes `&'static str`; a `const` cannot be `concat!`-ed). `tests/dashboard_gaps.rs` is the regression net for the two dashboard queries.
- Incomplete predicates are fixed SQL fragments chosen from a Rust enum (never interpolated user text). Blank = `TRIM(col) = ''`.
- No migration, no setting, no route, no dependency, no locale key (agent-facing English, as in 11-7).
- Wire traps (11-6/11-7): `Host: localhost`, `Accept: application/json, text/event-stream`; `structuredContent` carries the JSON.
- E2E series creation / assignment: copy the flow of `specs/journeys/series.spec.ts` (`#assign-series`, `#assign-series-submit`, position input).
- Tests: `#[sqlx::test(migrations = "./migrations")]`, raw SQL seeding, `tests/common/mcp.rs`. E2E: `loginAs`, `specIsbn("MG", n)`, fresh request context for the agent, no `waitForTimeout`; full run on a freshly reset stack.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.8] — FR129.
- [Source: _bmad-output/implementation-artifacts/11-7-mcp-catalog-read-tools.md] — conventions; D-11-6-*, D-11-7-* in `epic-11-autonomous-decisions.md`.
- [Source: src/models/series.rs#list_with_gaps] — gap rule (story 9-6).

### Review Findings

Code review 2026-10-28 (bmad-code-review: Blind Hunter, Edge Case Hunter, Acceptance Auditor; second full pass after fixes found no Medium+ issue).

- [x] [Review][Patch] (Medium) `gap_count` disagreed with `missing_positions`, and `list_series gaps_only` hid a series that `get_series` reported gaps for, when a position sits above a closed series' total — reachable, not legacy-only (`update_series` checks the new total against the row count). Positions above the total now fill no slot in the shared SQL join and in `series_gaps`; dashboard fixed with it (D-11-8-8) [src/models/series.rs, src/services/series.rs]
- [x] [Review][Patch] (Low) Gaps predicate written three times — shared through `has_gaps_predicate!` [src/models/series.rs]
- [x] [Review][Patch] (Low) `missing` was case-sensitive while `media_type` is not — `MetadataField::parse` ignores ASCII case [src/models/title_search.rs]
- [x] [Review][Patch] (Low) AC5 only partly tested — soft-deleted junction row, soft-deleted role, empty publisher added; AC9 over-long query and unknown media type now also go through the router [tests/mcp_series_gaps_tools.rs]
- [x] [Review][Patch] (Low) E2E: no-op `waitForURL` after the assignment, nested `expect` inside `expect.poll`, title lists only shape-checked, fragile submit selector — fixed [tests/e2e/specs/journeys/mcp-gaps-tools.spec.ts]
- [x] [Review][Patch] (Low) Stale comments on `count_with_gaps` and `SeriesGaps::owned_positions` [src/models/series.rs, src/services/series.rs]
- [x] [Review][Defer] (Low) `/series` list page counts owned/gaps from assignment rows, and the `update_series` guard lets a total drop below the highest position — pre-existing → #519
- Dismissed (11): positions ≤ 0 listed in `positions` (AC2: every live row); "not found" wording (same as `get_title`); incomplete-metadata query cost; genre error lists all genres (pre-existing, 11-7); `expect` in the static schema builder; literal "25" in descriptions; `series_type` parse fallback (pre-existing; the column is an ENUM); E2E data left behind (shared-DB convention); SQL vs `has_cover` whitespace (PAD SPACE collation); `series sx` without `deleted_at` (outer `s` filtered); `PRIMARY_CONTRIBUTOR_SUBQUERY` copies in moved code.

### Senior Developer Review (AI)

- **Outcome:** approved after fixes (commits 8e5d27c, cb7cd87).
- **Gates (2026-10-28):** `cargo clippy --all-targets -- -D warnings` clean; `cargo test` with the test DB on 3307: 1730 passed / 0 failed across 65 binaries (+4: `dashboard_gaps::position_above_total_does_not_fill_a_slot`, three in `mcp_series_gaps_tools`); `cargo sqlx prepare --check --workspace -- --all-targets` clean; flake gate clean; `tsc --noEmit` clean; Playwright on a freshly reset stack, `CI=true --workers=2`: 362 passed, 2 skipped, 0 failed, 0 flaky.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

- Previous dev session killed by a kernel OOM (2026-10-28 15:23) during the gate phase; resumed from local commits 2caaeac..e618dfa plus uncommitted clippy clean-ups (`resolve_genre` takes `&ToolContext`; needless lifetime in the integration test), kept.

### Completion Notes List

- T0: search projection moved to `src/models/title_search.rs`, re-exported from `models::title` (no caller change); `title.rs` 1942 → 1501 lines. Closes #516.
- T1: `filled_positions_join!()` macro spliced with `concat!` into `count_with_gaps`, `list_with_gaps` and `list_summaries` (resume: `count_with_gaps` still had its own correlated subquery — moved onto the shared fragment; `tests/dashboard_gaps.rs` green). `TitleModel::list_incomplete_metadata` + `MetadataField` enum (fixed SQL predicates, never user text). `like_contains_pattern` moved to `utils.rs` (shared by contributors and series).
- T2: pure `series_gaps` / `gap_count` (cap 1000, open series, out-of-range positions — unit-tested), `SeriesService::{list_summaries, agent_series_detail}`, `CatalogSearch.no_cover_only`, `SearchService::list_incomplete_metadata`.
- T3: four tools (`list_series`, `get_series`, `list_titles_without_cover`, `list_titles_with_incomplete_metadata`); media-type / genre helpers and the title item shared with `search_titles`; registry test pins the seven tools.
- T4: `tests/mcp_series_gaps_tools.rs` (9 tests); shared fixtures moved to `tests/common/mcp.rs`.
- T5: `mcp-gaps-tools.spec.ts` (spec id `MG`); `MCP_TOOLS` constant in `helpers/mcp.ts` used by `mcp-read-tools.spec.ts`.
- T6: `docs/route-role-matrix.md` § MCP lists the seven tools.
- T7 gates (2026-10-28): `cargo clippy --all-targets -- -D warnings` clean; `cargo test` (lib + bins + all integration binaries, test DB on 3307) 1726 passed / 0 failed across 64 binaries (lib 1281; `mcp_series_gaps_tools` 9, `mcp_catalog_read_tools` 17, `dashboard_gaps` 16, `locale_parity` 3); `cargo sqlx prepare --check` clean; flake gate clean; `tsc --noEmit` clean; Playwright on a freshly reset stack, `CI=true --workers=2`: 361 passed, 2 skipped, 1 flaky (`shelving.spec.ts` "second shelve removes the earlier undo button", unrelated to this story, passed on retry).

### File List

- `docs/route-role-matrix.md`
- `src/mcp/tools/args.rs`
- `src/mcp/tools/get_series.rs` (new)
- `src/mcp/tools/list_series.rs` (new)
- `src/mcp/tools/list_titles_with_incomplete_metadata.rs` (new)
- `src/mcp/tools/list_titles_without_cover.rs` (new)
- `src/mcp/tools/mod.rs`
- `src/mcp/tools/search_contributors.rs`
- `src/mcp/tools/search_titles.rs`
- `src/models/contributor.rs`
- `src/models/mod.rs`
- `src/models/series.rs`
- `src/models/title.rs`
- `src/models/title_search.rs` (new)
- `src/services/search.rs`
- `src/services/series.rs`
- `src/utils.rs`
- `tests/common/mcp.rs`
- `tests/mcp_catalog_read_tools.rs`
- `tests/mcp_series_gaps_tools.rs` (new)
- `tests/dashboard_gaps.rs` (review regression test)
- `tests/e2e/helpers/mcp.ts`
- `tests/e2e/specs/journeys/mcp-gaps-tools.spec.ts` (new)
- `tests/e2e/specs/journeys/mcp-read-tools.spec.ts`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`

## Change Log

- 2026-10-28 — story created (create-story).
- 2026-10-28 — validated against the create-story checklist: SQL fragment sharing, dashboard regression net, E2E series flow, re-export keeps callers unchanged.
- 2026-10-28 — dev-story complete (T0–T7): four MCP tools, #516 split, gates green → review.
- 2026-10-28 — code review: 1 Medium + 5 Low fixed (gap rule above the total, D-11-8-8), 1 Low deferred to #519, second pass clean → done.
