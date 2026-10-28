# Story 11.7: MCP catalog read tools

Status: ready-for-dev

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

- [ ] T1 — Model (AC1, AC3, AC5): `TitleModel::active_search` gains `media_type: Option<&str>` (`t.media_type = ?`, bound); `ContributorModel::search_paged` (count + page query, `deleted_at IS NULL` on contributors, junction and titles).
- [ ] T2 — Services (AC1–AC3, AC6): `services::search::{CatalogSearch, SearchService::search_catalog}`; `services::contributor::ContributorService::search`; `services::title_detail::{AgentTitleDetail, find_agent_title_detail}` (flattened `TitleDetail` + `has_cover` + `manually_edited_fields`). Unit tests where pure.
- [ ] T3 — Tools (AC1–AC3, AC7, AC8): `src/mcp/tools/search_titles.rs`, `search_contributors.rs`; shared argument helpers (`page`, `query`) in `src/mcp/tools/args.rs`; `get_title` switches to the enriched document; registry + its test updated; no-SQL walk test in `src/mcp/mod.rs`.
- [ ] T4 — Integration tests (AC9): extract `tests/common/mcp.rs` from `tests/mcp_endpoint.rs`; new `tests/mcp_catalog_read_tools.rs`.
- [ ] T5 — E2E (AC9): `tests/e2e/helpers/mcp.ts` (extracted `mcpPost`, `mcpCall`), reused by `mcp-endpoint.spec.ts`; new `mcp-read-tools.spec.ts` (spec id `MR`).
- [ ] T6 — Docs: `docs/route-role-matrix.md` § MCP lists the three tools. User manual = story 11-24.
- [ ] T7 — Gates: clippy `--all-targets -D warnings`, full `cargo test -j 1` (mem-guard), `cargo sqlx prepare --check` (dynamic queries only expected — no `.sqlx` change), locale parity (no keys: tool texts are agent-facing English, not UI), flake grep, `tsc --noEmit`, full E2E `--workers=2`.

## Dev Notes

### Architecture (AR27)

`tools/*.rs` → `services/*` → `models/*`. The tool files may define small `Serialize` DTOs and map service rows into them (pure mapping, no I/O besides the service call). `ToolError::from(AppError)` already logs and hides internals.

### Search semantics

- `search_catalog` order: trimmed query empty → `active_search("", filters)` (browse). Otherwise `detect_code`: `VCode` → `VolumeModel::find_by_label_with_title` (both live) → one-item list if filters match, else fall through to text; `Isbn` → `TitleModel::find_by_isbn` → same; `LCode` / `Text` → `active_search`. Exact hits are returned as a one-item page (`total_items = 1`) on page 1 and as an empty page with the same totals on page > 1.
- Filters on an exact hit compare the `TitleModel` (`media_type`, `genre_id`).
- `active_search` FULLTEXT needs ≥ 3 characters after stripping operators; shorter queries use LIKE — unchanged.
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

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-10-28 — story created (create-story).
