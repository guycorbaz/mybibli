# Story 11.7: MCP catalog read tools

Status: review

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR128, AR27. Epic 11 — seventh story; builds on 11-6 (`/api/mcp`, `McpTool` trait, `get_title`). 11-8 adds the series / gaps / incomplete-metadata tools on the same pattern.

## Story

As an AI agent,
I want to search and read the catalog,
so that I can identify what I could propose.

## Existing-code reality check (code map, 2026-10-28, `main` 7d929c1)

| Place | Today | Consequence |
|---|---|---|
| `src/mcp/tool.rs` | `McpTool` trait (name, title, description, input_schema, read_only, call → `serde_json::Value`), `ToolError { InvalidArguments, NotFound, Internal }`, `ToolRegistry::with` | new tools implement it; nothing rmcp-specific to touch |
| `src/mcp/tools/mod.rs` | `registry()` registers `get_title`; unit test checks snake_case names, title, description > 40 chars, object schema, read-only | register `search_titles`, `search_contributors`; the test now also pins the exact tool set |
| `src/mcp/tools/get_title.rs` | adapter over `services::title_detail::find_title_detail`; `parse_args` rejects non-objects (serde reads `[1]` as a struct) | reference pattern for argument parsing; enriched document (AC2) |
| `src/services/title_detail.rs` | `TitleDetail` DTO shared with `GET /api/v1/titles/{id}` | MCP-only enrichment wraps it with `#[serde(flatten)]` so `/api/v1` JSON stays byte-identical (D-11-7-2) |
| `src/services/search.rs` | `detect_code` (V-code, L-code, ISBN-10→13, ISBN-13, text); `SearchService::search` (9 params, used by `home.rs` and `api_v1.rs`); empty query + no filter → empty list; L-code → `Redirect` | new `SearchService::search_catalog(pool, &CatalogSearch)` for the agent — reuses `detect_code` and `enrich_title`, never redirects, browses on an empty query; the two existing callers are untouched |
| `src/models/title.rs::active_search` | FULLTEXT/LIKE on title/subtitle/description + contributor name, genre / volume-state / no-volume / no-cover filters, fixed page size `DEFAULT_PAGE_SIZE = 25`, sort whitelist; no media-type filter; `(page-1) * 25` in `u32` | gains `media_type: Option<&str>` (bound, not interpolated); its only caller `fulltext_search` passes `None` |
| `src/models/contributor.rs` | `search_by_name(query, limit)` (LIKE, no count, no offset) used by autocomplete | new `search_paged(pool, query, page)` → `PaginatedList<ContributorSearchRow>` with live title count; autocomplete untouched |
| `src/services/contributor.rs` | `ContributorService` (find_or_create, add/remove, update, delete) | new `ContributorService::search(pool, query, page)` |
| `src/models/media_type.rs` | `MediaType` enum + `FromStr` (book, bd, cd, dvd, magazine, report) | argument validation for `media_type` |
| `src/models/genre.rs` | `GenreModel::list_active` | resolve the `genre` argument by name (D-11-7-5) |
| `tests/mcp_endpoint.rs` | 637 lines; private helpers `state_with_pool`, `router`, `seed_key`, `seed_title`, `Call`, `send`, `send_json`, `legacy_call`, `modern_call` | move the helpers into `tests/common/mcp.rs` and reuse them from both files (Rule #1, D-11-7-7) |
| `tests/e2e/specs/journeys/mcp-endpoint.spec.ts` | private `mcpPost` + header constants | extract to `tests/e2e/helpers/mcp.ts` and reuse |

## Acceptance Criteria

1. **AC1 — `search_titles` (FR128).** Input `{ query?: string (≤ 200 chars), media_type?: "book"|"bd"|"cd"|"dvd"|"magazine"|"report", genre?: string, page?: integer 1..=10000 }`, `additionalProperties: false`. `query` is free text (title, subtitle, description, contributor name), an ISBN-10 / ISBN-13 (exact lookup, ISBN-10 converted as in 11-1) or a volume label `V0042` (the title owning that volume); an L-code or an unmatched code falls through to text search. An empty / absent query with no filter browses the whole catalog. `media_type` and `genre` (genre name, case-insensitive; unknown → `isError` listing the valid names) also apply to an exact ISBN / volume-label hit. Output: `{ items: [{ id, title, subtitle, media_type, genre_name, primary_contributor, volume_count, has_cover, publication_date (YYYY-MM-DD | null), dewey_code }], page, page_size, total_items, total_pages }`, sorted by title.
2. **AC2 — `get_title` enriched.** The 11-6 document (bibliographic fields, contributors with `role`, volumes with location and condition, series with `position` / `is_omnibus` / `end_position`) plus `has_cover: bool` (true iff `cover_image_url` is non-empty) and `manually_edited_fields: [string]` (from `titles.manually_edited_fields`, `[]` when NULL). `GET /api/v1/titles/{id}` is unchanged.
3. **AC3 — `search_contributors`.** Input `{ query?: string (≤ 200 chars), page?: integer 1..=10000 }`. Case-insensitive substring match on the name (LIKE wildcards escaped); empty query lists all. Output `{ items: [{ id, name, title_count }], page, page_size, total_items, total_pages }`, sorted by name; `title_count` counts live titles through live junction rows.
4. **AC4 — soft delete.** No tool ever returns a soft-deleted title, volume, contributor, junction row or series assignment; a soft-deleted title is "not found" for `get_title`, absent from `search_titles` (text, ISBN and volume-label paths); a soft-deleted contributor is absent from `search_contributors` and from `get_title`.
5. **AC5 — pagination capped server-side.** The page size is fixed at 25 (`DEFAULT_PAGE_SIZE`); no argument can raise it (`page_size` / `limit` are rejected by `additionalProperties: false`). `page` outside 1..=10000 → `isError` (no `u32` overflow); a page past the end returns `items: []` with the true totals.
6. **AC6 — thin adapters (AR27).** Tools parse arguments, call `services/` (`search.rs`, `title_detail.rs`, `contributor.rs`) and map to JSON. No SQL in `src/mcp/`: a unit test walks `src/mcp/` and fails on `sqlx` or SQL keywords in string literals (`SELECT `, `FROM `).
7. **AC7 — self-describing tools.** Each tool has a title, a description stating what it searches, how to page, what it returns and that it is read-only, and an input schema with a `description` on every property; the registry test pins the exact tool set `{get_title, search_contributors, search_titles}`.
8. **AC8 — invalid arguments.** Wrong types, unknown properties, non-object arguments, an unknown media type, an unknown genre, an over-long query, `page = 0` → tool result `isError: true` with a plain message the agent can act on (never a protocol error, never a 500).
9. **AC9 — tests.** Integration tests (`tests/mcp_catalog_read_tools.rs`, full router, isolated DB) cover each tool over MCP: text / ISBN-10 / ISBN-13 / V-code / media-type / genre / browse / pagination (30 titles → 25 + 5, page past the end) for `search_titles`; enriched `get_title` (contributors+roles, volumes, series+position, cover flag, manually edited fields) plus unknown and soft-deleted ids; `search_contributors` (match, title count, pagination, soft-deleted contributor excluded); soft-deleted rows on every path; invalid arguments. Unit tests cover argument parsing and the no-SQL walk. E2E: `tests/e2e/specs/journeys/mcp-read-tools.spec.ts` — `loginAs(admin)`, scan a title, create a `propose` key, `tools/list` lists the three tools, `search_titles` finds the scanned ISBN, `get_title` returns `has_cover` / `manually_edited_fields`, `search_contributors` answers, key revoked in `finally`.

## Tasks / Subtasks

- [x] T1 — Model (AC1, AC3, AC5): `TitleModel::active_search` gains `media_type: Option<&str>` (`t.media_type = ?`, bound); `ContributorModel::search_paged` (count + page query, `deleted_at IS NULL` on contributors, junction and titles).
- [x] T2 — Services (AC1–AC3, AC6): `services::search::{CatalogSearch, SearchService::search_catalog}`; `services::contributor::ContributorService::search`; `services::title_detail::{AgentTitleDetail, find_agent_title_detail}` (flattened `TitleDetail` + `has_cover` + `manually_edited_fields`). Unit tests where pure.
- [x] T3 — Tools (AC1–AC3, AC7, AC8): `src/mcp/tools/search_titles.rs`, `search_contributors.rs`; shared argument helpers (`page`, `query`) in `src/mcp/tools/args.rs`; `get_title` switches to the enriched document; registry + its test updated; no-SQL walk test in `src/mcp/mod.rs`.
- [x] T4 — Integration tests (AC9): extract `tests/common/mcp.rs` from `tests/mcp_endpoint.rs`; new `tests/mcp_catalog_read_tools.rs`.
- [x] T5 — E2E (AC9): `tests/e2e/helpers/mcp.ts` (extracted `mcpPost`, `mcpCall`), reused by `mcp-endpoint.spec.ts`; new `mcp-read-tools.spec.ts` (spec id `MR`).
- [x] T6 — Docs: `docs/route-role-matrix.md` § MCP lists the three tools. User manual = story 11-24.
- [x] T7 — Gates: clippy `--all-targets -D warnings`, full `cargo test -j 1` (mem-guard), `cargo sqlx prepare --check` (dynamic queries only expected — no `.sqlx` change), locale parity (no keys: tool texts are agent-facing English, not UI), flake grep, `tsc --noEmit`, full E2E `--workers=2`.

## Dev Notes

### Architecture (AR27)

`tools/*.rs` → `services/*` → `models/*`. The tool files may define small `Serialize` DTOs and map service rows into them (pure mapping, no I/O besides the service call). `ToolError::from(AppError)` already logs and hides internals.

### Search semantics

- `search_catalog` order: trimmed query empty → `active_search("", filters)` (browse). Otherwise `detect_code`: `VCode` → `VolumeModel::find_by_label_with_title` (both live) → one-item list if filters match, else fall through to text; `Isbn` → `TitleModel::find_by_isbn` → same; `LCode` / `Text` → `active_search`. Exact hits are returned as a one-item page (`total_items = 1`) on page 1 and as an empty page with the same totals on page > 1.
- Filters on an exact hit compare the `TitleModel` (`media_type`, `genre_id`).
- `active_search` FULLTEXT needs ≥ 3 characters after stripping operators; shorter queries use LIKE — unchanged.
- Verified live-only lookups: `VolumeModel::find_by_label` and `TitleModel::{find_by_id, find_by_isbn}` filter `deleted_at IS NULL`, so a soft-deleted volume label or title ISBN falls through to text search and finds nothing.
- `has_cover` uses the same predicate as the `no_cover_only` filter (CR #355): NULL or empty string = no cover.
- `title_count` = `COUNT(DISTINCT t.id)` over `LEFT JOIN title_contributors tc (tc.deleted_at IS NULL) JOIN titles t (t.deleted_at IS NULL)` — a contributor with two roles on one title counts it once; a contributor without live titles is still listed with 0. Name matching relies on the column's case-insensitive collation; LIKE wildcards are escaped as in `search_by_name`.
- Genre names are compared with `to_lowercase()` in Rust over `GenreModel::list_active` (accents preserved).
- FULLTEXT (`ft_titles_search`) sees committed rows only — `#[sqlx::test]` inserts commit, so text search works in tests (as in `tests/search_filter_browse.rs`).
- Page cap: `MAX_PAGE = 10_000` (250 000 rows — far above a household library); prevents `(page - 1) * 25` overflowing `u32`.

### Wire details (from 11-6)

- Every `oneshot` request needs `Host: localhost`; POST with `Accept: application/json, text/event-stream`.
- `tools/call` results: `structuredContent` = the tool JSON, `content[0].text` = the same JSON as text; tool failures → `isError: true`.

### Testing standards

`#[sqlx::test(migrations = "./migrations")]`; keys seeded as in 11-6; titles/contributors/volumes seeded with raw SQL inside the test DB (test-only, as in `tests/mcp_endpoint.rs`). E2E: `loginAs`, `specIsbn("MR", n)`, fresh request context for the agent, no `waitForTimeout`. Heavy commands under `mem-guard`; `cargo test -j 1`.

### Regression map

`tests/mcp_endpoint.rs` (helpers moved), `tests/api_v1_*.rs` (title detail unchanged), `tests/search_*.rs` and `home.rs` search (active_search signature), contributor autocomplete (untouched), `mcp-endpoint.spec.ts` (helper extraction).

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.7] — ACs; FR128, AR27.
- [Source: _bmad-output/implementation-artifacts/11-6-mcp-endpoint-rmcp-spike.md] — trait, transport, wire traps.
- [Source: _bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md] — D-11-6-*, D-11-7-*.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Review Findings

Round 1 (2026-10-28, Blind Hunter + Edge Case Hunter + Acceptance Auditor, full mode): 0 High, 10 Medium, ~20 Low.

- [x] [Review][Patch] (Medium) An existing ISBN / volume label that failed the filters fell through to a text search on the digits — now an empty page (D-11-7-9) [src/services/search.rs]
- [x] [Review][Patch] (Medium) Duplicate ISBNs hidden by `find_by_isbn … LIMIT 1` — `TitleModel::list_by_isbn`, all live titles returned and paged [src/models/title.rs, src/services/search.rs]
- [x] [Review][Patch] (Medium) Hyphenated / spaced ISBNs read as text — `compact_code` strips separators when the result is ISBN-shaped
- [x] [Review][Patch] (Medium) No tie-breaker in the title order: paging could repeat or skip equal titles — `ORDER BY …, t.id ASC`
- [x] [Review][Patch] (Medium) Media-type list hard-coded three times — `MediaType::ALL` feeds the schema enum and the error message
- [x] [Review][Patch] (Medium) `genre` / `media_type` unbounded and echoed back — capped at 200 characters; empty-genre-list message
- [x] [Review][Patch] (Medium) No-SQL walk test weak (`//` split cut URLs, case/whitespace gaps) — whole comment lines only, keyword + any whitespace, lower-case `select … from`, sqlx API names; self-test of the detector
- [x] [Review][Patch] (Medium) Non-object `arguments` never reach the tool over the wire — pinned as a JSON-RPC error (D-11-7-8)
- [x] [Review][Patch] (Medium→Low) E2E loop variable shadowed the `page` fixture — renamed (the author is a single exact-name row, so the 10-page bound is ample)
- [x] [Review][Patch] (Low) Genre matching accent-sensitive unlike the UI — DB collation (D-11-7-10); query bound applied before trimming like `maxLength`; tests for soft-deleted series / series links / primary contributor, upper-case media type over the wire, V-code with mismatching filter, equal-title paging, duplicate ISBNs
- [x] [Review][Defer] (Low) `src/models/title.rs` at 1942 lines → #516
- [x] [Review][Defer] (Low, pre-existing) `enrich_title` hides lookup errors behind defaults → #517
- Dismissed: `page: 2.0` (schema says integer; clients send integers); `has_cover` not checking the file on disk (documented predicate, same as the UI filter); malformed `manually_edited_fields` read as `[]` (pre-existing, logged at WARN); count/page not in one snapshot (household scale, same as the web lists); LIKE escaping under `NO_BACKSLASH_ESCAPES` (mode not used); `page_size` constant in the response (same constant as the query); FULLTEXT short tokens (unchanged web behaviour); description-length assertions (deliberate AC7 guard).

### Debug Log References

- `cargo fmt -- <files>` formatted the whole crate (main is not fmt-clean); unrelated files were restored and only this story's hunks kept.
- A targeted E2E run before the full suite left the scanned ISBNs in the DB; the full run then re-scanned existing titles (no skeleton) — the full suite was rerun on a freshly reset stack.

### Completion Notes List

- `search_titles` (text / ISBN-10/13 / volume label, `media_type`, `genre` by name, browse on empty query, 25 per page, page 1..=10000) over the new `SearchService::search_catalog`; `TitleModel::active_search` gained a bound `media_type` filter (web callers pass `None`).
- `search_contributors` over `ContributorService::search` → `ContributorModel::search_paged` (live title count, LIKE metacharacters escaped by a shared `like_contains_pattern`).
- `get_title` now returns `AgentTitleDetail` (flattened `TitleDetail` + `has_cover` + `manually_edited_fields`); `/api/v1` unchanged (D-11-7-2).
- Shared tool plumbing: `tools/args.rs` (object parsing, page and query bounds), `ListPage<T>` and `to_json` in `tools/mod.rs`; registry test pins the exact tool set, a description on every schema property and `additionalProperties: false`; `mcp::tests::no_sql_under_src_mcp` enforces AR27.
- Tests: 11 integration tests (`tests/mcp_catalog_read_tools.rs`), MCP helpers moved to `tests/common/mcp.rs`; unit tests for args, genre matching, exact-hit filters, `has_cover`, LIKE escaping, list page shape. Full `cargo test -j 1`: 1696 passed, 0 failed. E2E `mcp-read-tools.spec.ts` (+ `helpers/mcp.ts`, reused by the 11-6 spec); full suite `--workers=2`: 360 passed, 2 skipped, 1 known flaky (shelving.spec.ts:193, #499) passed on retry.
- No migration, no new setting, no route, no dependency, no locale key.

### File List

- src/mcp/mod.rs, src/mcp/tools/mod.rs, src/mcp/tools/get_title.rs
- src/mcp/tools/args.rs, src/mcp/tools/search_titles.rs, src/mcp/tools/search_contributors.rs (new)
- src/services/search.rs, src/services/title_detail.rs, src/services/contributor.rs
- src/models/title.rs, src/models/contributor.rs, src/models/genre.rs, src/models/media_type.rs
- tests/common/mod.rs, tests/common/mcp.rs, tests/mcp_catalog_read_tools.rs (new); tests/mcp_endpoint.rs
- tests/e2e/helpers/mcp.ts, tests/e2e/specs/journeys/mcp-read-tools.spec.ts (new); tests/e2e/specs/journeys/mcp-endpoint.spec.ts
- docs/route-role-matrix.md
- _bmad-output/implementation-artifacts/11-7-mcp-catalog-read-tools.md, epic-11-autonomous-decisions.md, sprint-status.yaml

## Change Log

- 2026-10-28 — story created (create-story).
- 2026-10-28 — validated against the create-story checklist: live-only lookups verified, cover predicate, title-count join, FULLTEXT commit note added.
- 2026-10-28 — implemented (dev-story); status → review.
