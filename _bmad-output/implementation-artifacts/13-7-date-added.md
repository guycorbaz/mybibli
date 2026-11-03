# Story 13.7: Date added — pages, search and the agent

Status: ready-for-dev

GitHub issue: #586 (type:change-request). Covers: FR153. Epic 13; ships in v1.23.0.

## Story

As a librarian (and as the agent working for her),
I want to know when titles and volumes were added,
so that I can review what was entered since a given day.

## Evidence

On 2026-11-03 the librarian asked the MCP agent to check everything entered since Saturday 2026-10-31; no tool exposes a date, so it could not identify the volumes.

## Decision (project lead, 2026-11-03)

Date added + the technical `updated_at` (display only). No "modified by hand" date.

## Existing-code reality check (code map, 2026-11-03)

| Place | Today |
|---|---|
| every entity table | `created_at` / `updated_at` (`ON UPDATE CURRENT_TIMESTAMP`) — `TIMESTAMP` columns; in dynamic `sqlx::query()` read them with `CAST(col AS DATETIME)` (CLAUDE.md MariaDB gotcha 4) |
| `templates/pages/title_detail.html`, `volume_detail.html` | no date shown |
| `src/routes/home.rs` (browse: `q`, `filter`, `sort`, `dir`; indicator filters, single-active-filter contract l. 315) | sorts by title/author/date?… (check `sort` keys) — no "date added" key, no "since" filter |
| `src/models/title.rs::count_recent_cataloged` / `list_recent_cataloged` (l. 618–680) + `src/routes/home_indicators.rs` (`RECENT_ACTIVITY_DAYS = 7`) | count/list titles by `titles.created_at` only |
| `src/mcp/tools/search_titles.rs` | args `query`, `media_type`, `genre`, `page`; closed schema |
| `src/mcp/tools/get_title.rs` | title + volumes, no dates |
| MCP registry rules | closed schemas; `tests/common/mcp.rs` and `MCP_TOOLS` (`tests/e2e/helpers/mcp.ts`) stay in sync |

## Acceptance Criteria

1. **AC1 — pages.** Title page and volume page show "Added on <date>" and "Modified on <date>" (localized date format of the current locale; new i18n keys in en/fr/de/it).
2. **AC2 — "added" rule.** A title counts as *added in a period* when `titles.created_at` is in it **or** one of its live volumes' `created_at` is in it. One SQL predicate (an `EXISTS` on `volumes` + the title's own date), shared by the browse filter, the dashboard indicator and the MCP filter (Rule #1).
3. **AC3 — browse.** The home browse accepts a sort key "date added" (newest first by default; the title's latest addition date = greatest of its own and its volumes' `created_at`) and a filter "added since <date>" (a date input; the URL carries `added_since=YYYY-MM-DD`; composes with `q`/sort like the other browse params — respect the indicator single-active-filter contract).
4. **AC4 — dashboard.** "Recently catalogued" uses the AC2 rule (count and list), window unchanged (`RECENT_ACTIVITY_DAYS`, inclusive `>=`); its unit test keeps the 7-day lock.
5. **AC5 — MCP.** `get_title` returns `created_at` and `updated_at` (ISO 8601 UTC) for the title and `created_at` for each volume; `search_titles` results carry `created_at`, and the tool accepts `added_since` (`YYYY-MM-DD` or full ISO 8601; invalid → `InvalidArguments` with an English message) applying AC2; tool descriptions mention it; closed schema; registry test green; `tests/common/mcp.rs` and `MCP_TOOLS` unchanged in names.
6. **AC6 — tests.** Unit: the date-arg parser, the sort key. DB integration: AC2 rule (title created long ago + volume added today → included; deleted volume ignored), dashboard count, MCP `added_since`. E2E (specId `DA`): a title page shows "Added on"; browse filter "added since today" lists a title created by the spec; MCP `search_titles` with `added_since` returns it.

## Tasks / Subtasks

- [ ] T1 — shared SQL predicate / model functions (AC2)
- [ ] T2 — pages (AC1)
- [ ] T3 — browse sort + filter (AC3), dashboard (AC4)
- [ ] T4 — MCP (AC5)
- [ ] T5 — tests; `cargo sqlx prepare`; locale parity; gates; full suites once before the single push
