# Story 15.6: Value coverage

Status: review

GitHub issue: #564 part 2 (type:change-request). Covers: FR166. Epic 15; ships in v1.25.0.

## Story

As a librarian,
I want to see how much of my collection the value total covers,
so that I know how far to trust it.

## Decisions (project lead, 2026-11-04 — #564)

Coverage on `/stats/value` is part of the base (with 15-5); estimate history, ranges and series premium come later.

## Design decisions (dev, 2026-11-05 — recorded for the review)

- **D1 — "valued" = a live volume of a live title with `current_value IS NOT NULL`** (the purchase price does not count: the page totals current values). Shares are of live volumes of live titles.
- **D2 — "older than one year" = `current_value_updated_at` before now − 365 days, or unknown** (a value without date counts as old: its age cannot be trusted). The bound is computed in Rust (UTC instant), never `CURDATE` / `DATEDIFF` (story 15-1 rule). Oldest / newest dates are shown in the instance zone.
- **D3 — breakdowns: the existing genre and series ones**, each row gaining its coverage (volumes, valued, share, oldest, newest, old count). A coverage table is independent of currency (coverage counts volumes, not money). Genres and series with live volumes appear even when none is valued (that is what coverage is for); series rows keep the CR #243 rule (a volume in N series counts in each).
- **D4 — the page shows the coverage even when no volume is valued** (0 % of N); the empty state stays for a library without any live volume.
- **D5 — agent read tool `list_volumes_to_value`** (read-only): live volumes ordered **never valued first, then oldest estimate first** (undated before dated, as D2), ties by title then V-code; optional `series_id` and `genre` filters; 25 per page. Each item: volume id and label, title id and title, condition, edition note, current value (`{amount, currency, updated_at}` or null), purchase price (or null). Volumes with a pending value proposal are left out (the agent cannot file another, 15-5 D4). No SQL under `src/mcp/` — the query lives in `models/value_coverage.rs`.

## Starting point (code map, 2026-11-05)

- Page: `src/routes/stats.rs` (187 lines), `templates/pages/stats_value.html`, model queries `VolumeModel::value_totals_by_currency` / `value_by_genre` / `value_by_series` (`src/models/volume.rs` 1345 lines → new queries in `src/models/value_coverage.rs`).
- Locales `stats.value.*` (en/fr/de/it).
- MCP list tools: `src/mcp/tools/list_titles_without_cover.rs` pattern (`ListPage`, `page_property`, `args::genre`, `resolve_genre`); amounts as in `services::title_detail::{AmountDoc, ValueDoc}` (15-5).
- Time: `AppState::time_zone()`, `utils::local_time::format_local_date`.

## Acceptance Criteria

1. **AC1 — overall coverage.** `/stats/value` shows, above the totals: live volumes, valued volumes, share (%), oldest and newest estimate dates, and the count of estimates older than one year (D2) — with an explanation sentence.
2. **AC2 — per breakdown.** A coverage table per genre and per series (D3) with the same columns; sorted by share ascending (least covered first), then name.
3. **AC3 — empty / partial states (D4).** No live volume → the existing empty state; volumes but none valued → coverage rows at 0 %, no totals tables.
4. **AC4 — agent tool `list_volumes_to_value` (D5).** Registered in `registry()`, `tests/common/mcp.rs` `ALL_TOOLS`, `tests/e2e/helpers/mcp.ts`; closed schema, read-only; description says the order and points to `propose_volume_value`; `docs/route-role-matrix.md` updated; `propose_volume_value` / `get_title` descriptions mention it.
5. **AC5 — tests.** Unit: share formatting and the old-estimate bound (pure). DB integration (`tests/value_coverage.rs`): counts overall / per genre / per series with deleted volumes and titles excluded, undated values counted as old, the tool order, filters, and the pending-proposal exclusion. E2E (`tests/e2e/specs/journeys/value-coverage.spec.ts`, own spec id): the librarian (real `loginAs`) sees the coverage change after valuing a volume; axe passes on `/stats/value`. Full suites green; flake gate.
6. **AC6 — i18n and docs.** New keys in en/fr/de/it; CLAUDE.md note (coverage rule + tool). Manual chapters are story 15-8.

## Tasks

- [x] `models/value_coverage.rs`: overall, per genre, per series coverage; volumes to value (page).
- [x] `/stats/value` coverage section + template + locales.
- [x] MCP tool `list_volumes_to_value` + registries + docs.
- [x] Tests (unit, DB, E2E with axe).
- [x] CLAUDE.md.

## Dev Agent Record

### Completion Notes

- All ACs implemented; coverage queries in the new `models/value_coverage.rs`; the agent tool reaches it through `services::volume_value_proposals::volumes_to_value` (no SQL under `src/mcp/`).
- Bug found by the DB test and fixed: `VolumeModel::value_totals_by_currency` counted volumes of soft-deleted titles (the page intro says they are excluded) — both sums now join live titles.
- axe caught an empty header on the overall coverage table (fixed: the column is labelled).
- Tests: 3 unit (share, old bound, order) + 1 tool unit, 3 DB integration (`tests/value_coverage.rs`), E2E `value-coverage.spec.ts` (spec id VC, V7500–V7999) with axe; targeted E2E 4/4.

### File List

- src/models/value_coverage.rs (new), src/models/mod.rs, src/models/volume.rs
- src/routes/stats.rs, templates/pages/stats_value.html, templates/fragments/stats_value_coverage_rows.html (new)
- src/mcp/tools/list_volumes_to_value.rs (new), mod.rs, propose_volume_value.rs
- src/services/volume_value_proposals.rs, src/services/title_detail.rs
- locales/{en,fr,de,it}.yml
- tests/value_coverage.rs (new), tests/common/mcp.rs, tests/e2e/helpers/mcp.ts, tests/e2e/specs/journeys/value-coverage.spec.ts (new)
- docs/route-role-matrix.md, CLAUDE.md
