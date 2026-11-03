# Story 13.7: Date added — pages, search and the agent

Status: done

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

- [x] T1 — shared SQL predicate / model functions (AC2)
- [x] T2 — pages (AC1)
- [x] T3 — browse sort + filter (AC3), dashboard (AC4)
- [x] T4 — MCP (AC5)
- [x] T5 — tests; `cargo sqlx prepare`; locale parity; gates; full suites once before the single push

## Completion Notes

**What was done**
- **AC2, the shared rule:** `src/models/date_added.rs` (new). It holds `added_since_predicate(bound)`, the single `(t.created_at >= b OR EXISTS live volume with created_at >= b)` fragment, and `LAST_ADDED_AT_SQL` (the later of the title's date and its newest live volume's). It also has `find_timestamps(DatedTable, id)`, `find_title_added` and `title_added_since`. The `EXISTS` uses the existing `idx_volumes_title (title_id)` index, so no migration was needed.
- **Search projection:** `SearchResult` gains `created_at` and `last_added_at`. The three copies of the projection SQL and the row mapping (`active_search`, `list_recent_active`, `list_recent_cataloged`) now share `search_projection()` and `search_result_from_row()` in `title_search.rs`. `active_search` takes `added_since`. A new sort key `date_added` sorts on `last_added_at` and defaults to `desc`.
- **AC1, pages:** new `templates/components/record_dates.html` and `utils::RecordDates`. They are included in the title page (full page only) and the volume page. Dates use `utils::format_date_localized`: en ISO, fr/it `dd/mm/yyyy`, de `dd.mm.yyyy`. New keys `record_dates.*`, `browse.added_since*` and `search.col.added` are in all four locales.
- **AC3, browse:** `src/routes/home_added_since.rs` (new) handles parsing, the indicator override and the query suffix. In the home page, a GET form with a date input sits next to the sort control; the URL carries `added_since=YYYY-MM-DD`. A new "Added" column (sortable, newest first on the first click) appears in the browse table, with the date cell in the shared row partial, so Recent additions gets it too. There are also `date_added` options in the sort select. Every browse link (chips, headers, pagination, HTMX fragment pagination, sort select JS) keeps the date. The indicator filter overrides the date, as it does `q` and `sort`.
- **AC4:** `count_recent_cataloged` / `list_recent_cataloged` use the shared predicate. The list is ordered by `last_added_at DESC`. The window and the 7-day lock test are unchanged.
- **AC5, MCP:** `search_titles` accepts `added_since` (`args::added_since`: `YYYY-MM-DD` = 00:00 UTC, or RFC 3339 / naive ISO = UTC; anything else is `InvalidArguments` in English). The filter also applies to exact ISBN / V-code hits. Items carry `created_at` and `last_added_at` (RFC 3339 UTC, reusing `proposal_history::rfc3339_utc`). `get_title` returns the title's `created_at` / `updated_at` and each volume's `created_at`. Descriptions and schema were updated; the schema stays closed. No tool names changed.

**Deviations / decisions**
- The dates were added to the shared `TitleDetail` DTO, so `/api/v1/titles/{id}` gains them too (additive). `list_titles_without_cover` items also gain the two dates, since they share `TitleItem`.
- `search_titles` items also carry `last_added_at`. Without it, an agent cannot see why an old title matches `added_since` (a new volume).
- Saved searches deliberately do not store `added_since`: an absolute date goes stale, and the table has no column for it (documented in `home_added_since.rs`).
- The browse table date is localized like the pages (review L4: the row partial takes `row_lang`).
- Days are UTC days. A volume entered between 00:00 and 01:00/02:00 Swiss time counts for the previous UTC day.
- In the web search, an exact V-code / ISBN hit ignores the date filter, consistent with the genre filter there. In MCP the filter applies, consistent with its other filters.
- The search field's `hx-include` is now scoped to `#filter-tags [name=…]` plus `#browse-added-since`. The new form's hidden inputs share the names `sort`/`dir`/`filter`, which would otherwise send duplicate params (#367 failure mode). A small `browse-mode.js` hook refreshes the form's hidden params from the URL on submit.
- No `sqlx::query!` macro changed, so `.sqlx/` is unchanged.

**Files over 2000 lines (all were already over before this story):** `src/routes/home.rs` 2582 (+18), `src/routes/titles.rs` 2768 (+15), `src/routes/catalog.rs` 3212 (+12). The new logic went into new modules.

**Tests:** unit tests (utils date helpers, `date_added`, `home_added_since` including a render test, sort key / dir in `title_search`, `args::added_since`, `search_titles` args, volume page dates). DB tests in `tests/date_added.rs` (7). The E2E spec is `tests/e2e/specs/journeys/date-added.spec.ts` (specId DA). `series-selector-recent-table.spec.ts` was updated: the table now has 6 header columns.

### Dev Notes (review)

- **UTC day boundaries (L1, deferred to #602):** "added since <date>" starts at 00:00 UTC, and the page dates are UTC calendar days. Time-zone handling for the instance is tracked in #602.
- **Web vs MCP exact hits (L5, dismissed):** in the web search an exact ISBN / V-code hit ignores `added_since`, as it already ignores the genre filter. The MCP `search_titles` applies all its filters to exact hits, `added_since` included. This is intentional.

### Review Findings

Code review, round 1 of a7d9298: no Medium or higher findings, 6 Low. A single round.

- [x] [Review][Defer] L1 — UTC day boundaries. Kept UTC; see Dev Notes; tracked in #602.
- [x] [Review][Patch] L2 — `list_recent_active` (home "Recent additions") now orders by `last_added_at DESC, t.id DESC`, the date its table shows. DB test `recent_additions_rank_by_latest_addition`.
- [x] [Review][Patch] L3 — the "added since" form leaves out an indicator filter: the date replaces the indicator view (single-active-filter contract). `AddedSinceControl::form_filter`, `INDICATOR_FILTER_NAMES` (locked against the parser), and the `browse-mode.js` submit hook skips indicator names. Route test `the_form_leaves_an_indicator_filter_out`.
- [x] [Review][Patch] L4 — the browse-table date uses `format_date_localized` (`SearchResult::added_date(locale)`, `row_lang` in the row partial).
- [x] [Review][Dismiss] L5 — web exact hits ignore `added_since`, MCP applies it; see Dev Notes.
- [x] [Review][Patch] L6 — the save-search form shows "The date filter is not saved with the search." while a date filter is active (`saved_searches.date_not_saved`, 4 locales). Render test in `routes::saved_searches`.
