# Story 13.5: Every series in the selector, Recent additions in table mode

Status: ready-for-dev

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

- [ ] T1 — model + route change (#576)
- [ ] T2 — template + JS (#577)
- [ ] T3 — tests; gates; full suites once before the single push
