# Story 13.5: Every series in the selector, Recent additions in table mode

Status: done

GitHub issues: #576 (type:bug, medium), #577 (type:bug, low). Covers: bug fixes. Epic 13; ships in v1.23.0.

## Story

As a librarian,
I want to choose any series from a title page, and see Recent additions in the mode I chose,
so that the 26th series is reachable and the home page is consistent.

## Existing-code reality check (from the issues, 2026-11-01; re-verify line numbers)

| Place | Today |
|---|---|
| `src/routes/titles.rs:133` | series options = `SeriesModel::active_list(pool, 1).await?.items` — page 1 of a paginated list (`DEFAULT_PAGE_SIZE = 25`, `ORDER BY name`) |
| `templates/pages/title_detail.html` ~l. 245 (`#assign-series`) | plain `<select>` |
| `templates/pages/home.html` ~l. 655 (`#recent-additions`) | hard-coded `<div class="browse-list">` + `components/title_card.html` |
| `static/js/browse-mode.js` | only handles `#browse-results` (l. 12) |
| `static/js/home-recent-additions.js` | the Recent additions fold |

## Acceptance Criteria

1. **AC1 — all series.** A new `SeriesModel::list_all_names(pool) -> Vec<(u64, String)>` (live series, ordered by name, no pagination) feeds the selector. If the production count makes a `<select>` unwieldy (> ~200), keep the `<select>` anyway in this story and note it — a typeahead is a later enhancement.
2. **AC2 — Recent additions follows the mode.** With the browse mode set to table, Recent additions renders as a table with the browse table's columns and markup (reuse the browse table template/fragment — Rule #1); grid/list modes as today; switching the toggle re-renders Recent additions live like `#browse-results`; the persisted mode applies on page load.
3. **AC3 — CSP / a11y.** No inline script or style; axe-core passes on the home page in table mode.
4. **AC4 — tests.** Unit: the model query (DB test with 30 series, deleted ones excluded). E2E: a title page offers a series whose name sorts after the 25th (the spec creates 26+ uniquely-prefixed series — mind #528: do not assume an empty series table); home page, table mode, Recent additions is a table and switches back to cards.

## Tasks / Subtasks

- [x] T1 — model + route change (#576)
- [x] T2 — template + JS (#577)
- [x] T3 — tests; gates; full suites once before the single push

## Completion Notes

**#576 — series selector.** `SeriesModel::list_all_names(pool) -> Vec<(u64, String)>` (`src/models/series.rs`): live series, `ORDER BY name, id`, no pagination (dynamic `query_as`, no `.sqlx` change). `src/routes/titles.rs` feeds `#assign-series` from it; the query moved into the full-page branch (the HTMX header refresh never renders the selector). `templates/pages/title_detail.html` destructures the tuple. `grep active_list(`: the only other page-1 consumer is `GET /api/v1/series` (`src/routes/api_v1.rs:249`, returns at most 25 series — a JSON API, not a selector; left as is, flagged here for a follow-up issue). No other series `<select>` exists. Production series count not measured; the plain `<select>` is kept per AC1.

**#577 — Recent additions follows the browse mode.** The browse toggle has two modes: *list* (= the browse table, CR #250) and *grid* (cards); "table mode" in the issue is the list button. New partial `templates/components/browse_table_row.html` is the single source of the list-mode `<tr>`: used by `#browse-results`, by `#recent-additions`, and by the HTMX search fragment (`src/routes/home_search_fragment.rs` now renders it through `BrowseTableRowTemplate` instead of a hand-built string — Rule #1). `#recent-additions` now emits `#recent-additions-results` (`.browse-list` + `data-browse-mode-container`) with the browse table (plain `scope="col"` headers — sortable links would re-sort `#browse-results`) and the `.browse-cards` grid; `static/css/browse.css` already shows one of them per class. `static/js/browse-mode.js` applies the mode to every `[data-browse-mode-container]` (both containers), so toggling is live and the persisted mode applies on load. Deviation: "grid/list as today" — in list mode Recent additions now shows the table (it previously showed a horizontal card stack that no longer exists anywhere else); grid mode shows the card grid. `#recent-cataloged-list` (indicator filter) is unchanged.

**Tests.** DB: `tests/series_list_all_names.rs` (30 series + 2 deleted; empty case). Unit: `home_search_fragment::test_render_search_table_row_uses_shared_partial`, `home_fold_tests::{recent_additions_renders_table_and_cards_under_a_mode_container, browse_results_is_a_browse_mode_container}`. E2E: new `tests/e2e/specs/journeys/series-selector-recent-table.spec.ts` (27 `zzz-RT-<run>-NN` series — the `zzz-` head keeps parallel specs' series on page 1 of `/series`, which `series.spec.ts` relies on; Recent additions table → grid live → reload persists → table, axe in table mode); `home.spec.ts` Recent additions test now clicks the table row link (cards are hidden in the default mode).

**Gates run.** `cargo clippy --all-targets -D warnings` clean; `cargo test --lib -- routes::home routes::titles templates_audit models::series` 182 passed; `cargo test --test series_list_all_names` 2 passed; `npx tsc --noEmit`, flake gate, home-page selector gate OK; Playwright (fresh stack, `--workers=2`): new spec + home, browse-toggle, home-search, series, series-epic11-smoke, mcp-gaps-tools, accessibility-full, csp-headers, dewey-code — 72 passed, 1 skipped. No locale change.

**Files over 2000 lines.** `src/routes/titles.rs` 2753 (was 2750; +3 lines, no new logic added there).

### Review Findings

Code review round 1 of 3d9f7fe — single round: no Medium+ (4 Low).

- [x] [Review][Defer] L1 — `GET /api/v1/series` still returns page 1 only (`src/routes/api_v1.rs`) → issue #599.
- [x] [Review][Dismiss] L2 — on mobile, list mode shows the 6-column table, like `#browse-results` (consistent).
- [x] [Review][Dismiss] L3 — brief table flash in grid mode when the fold is remembered open — pre-existing for `#browse-results`; the fold is closed by default.
- [x] [Review][Patch] L4 — `render_search_table_row` and `render_search_row` (`src/routes/home_search_fragment.rs`) log the render error with `tracing::warn!` before falling back to an empty string.
