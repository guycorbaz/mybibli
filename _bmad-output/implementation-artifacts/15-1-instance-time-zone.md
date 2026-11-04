# Story 15.1: Instance time zone

Status: review

GitHub issue: #602 (type:change-request). Covers: FR161. Epic 15; ships in v1.25.0.

## Story

As a librarian in Switzerland,
I want dates shown and days cut in my local time,
so that a book entered at half past midnight is dated that day.

## Decision (project lead, 2026-11-04)

An **Admin → System** setting (not an env var), default `Europe/Zurich`, effective without restart. Storage, HTTP API and MCP stay UTC.

## Code map (starting points, 2026-11-04 — verify and complete)

- DB session is UTC (`src/db.rs`). Settings K/V: `src/config.rs` (`AppSettings`, `load_from_db` validation), Admin → System forms in `src/routes/admin_system.rs` (`fetch_setting_rows` whitelist ~l. 345, save + `reload_settings_cache`), `services::admin_system::{save_setting, reload_settings_cache}`; `PageSettings` / `AppState::page_settings()` (story 14-2) is the per-page read of settings.
- Date-added rule (story 13-7): `models::date_added`, `routes/home_added_since.rs` (`added_since=YYYY-MM-DD` → today cut in UTC), browse "Added" column, title/volume pages (`services/title_detail.rs`), Recent cataloged indicator (`RECENT_ACTIVITY_DAYS`).
- Other user-visible dates/times: loans (`routes/loans.rs`, due/overdue — `loan::count_overdue` uses DATEDIFF), wish list (`routes/wishlist.rs`, `models/wishlist.rs`, `services/wishlist_pdf.rs` — uses `Local::now()`), proposals (`services/proposal_review.rs`, `proposal_history.rs`), admin audit / health / trash / API keys (`routes/admin*.rs`, `models/admin_audit.rs`), catalog feedback, title detail. Grep every `.format(` / `NaiveDateTime` / `DATE(` reaching a template.
- The API (`routes/api_v1.rs`) and MCP (`src/mcp/`) keep UTC ISO 8601; MCP `added_since` accepts an offset.

## Acceptance Criteria

1. **AC1 — setting.** New K/V row `time_zone` (IANA name, validated with `chrono-tz`; invalid → keep previous + warn on load, form error on save), default `Europe/Zurich` (migration seeds the row), shown in Admin → System (own small form or within an existing one), whitelisted in `fetch_setting_rows`, optimistic locking, cache reload without restart. Error copy per `docs/error-message-style.md`; i18n four locales.
2. **AC2 — one conversion helper.** A single utility (e.g. `utils::local_time` module) converts a UTC timestamp to the instance zone for display and a local calendar day to its UTC `[start, end)` bounds (DST-safe); no per-site arithmetic (Rule #1).
3. **AC3 — display.** Every date/time shown in the web interface uses the instance zone (list them in the completion notes).
4. **AC4 — day bounds.** Web date inputs and day-based filters (`added_since`, Recent cataloged window if day-based, overdue / days-overdue, wish list dates) use local-day bounds converted to UTC; `Local::now()` is gone from the codebase (server TZ must not matter).
5. **AC5 — API/MCP unchanged.** Responses stay UTC ISO 8601; MCP date inputs accept a date or an RFC 3339 timestamp with offset (document the semantic of a bare date: UTC day, unchanged).
6. **AC6 — tests.** Unit: conversion around midnight and both DST transitions (Europe/Zurich), invalid zone. DB integration: a title created at 23:30 UTC on day D is "added since D+1" in Zurich. E2E: change the zone in Admin → System and see a date move; the default shows local dates. Full suites green.

## Tasks / Subtasks

- [x] AC1 — `time_zone` K/V row (migration `20261104120000_settings_time_zone.sql`, default `Europe/Zurich`), `AppSettings::time_zone` (validated with `chrono-tz` on load), `AppState::time_zone()` / `local_clock()`, Admin → System form (`routes/admin_system_time_zone.rs`, template `fragments/admin_system_time_zone_form.html`, route `POST /admin/system/time-zone`), whitelisted in `fetch_setting_rows`, optimistic lock via `save_setting`, cache reload via `reload_settings_cache`, i18n in four locales.
- [x] AC2 — `src/utils/local_time.rs`: the only conversion point.
- [x] AC3 — every web date shown converted (list below).
- [x] AC4 — day bounds: `added_since`, overdue count/list, day counts of loans; `Local::now()` removed.
- [x] AC5 — API/MCP unchanged (UTC); MCP `added_since` documented (bare date = UTC day, offset accepted).
- [x] AC6 — unit, DB integration and E2E tests; full suites green.

## Dev Agent Record

### Completion Notes

**Design (AC2).** `crate::utils::local_time` (`src/utils/local_time.rs`):
`DEFAULT_TIME_ZONE` (Europe/Zurich), `parse_time_zone` (exact IANA name),
`time_zone_names` (datalist), `to_local`, `local_date`, `format_local`,
`format_local_date`, `format_local_date_time` (`YYYY-MM-DD HH:MM`),
`day_start_utc` (first instant of a local day; on a day whose midnight
falls in a DST gap, the first local minute after it; on a repeated
midnight the earlier instant), `day_bounds_utc` (`[start, end)`, 23 h /
25 h on DST days), and `LocalClock { tz, now_utc }` (`now(tz)`,
`at(tz, now)`, `today()`, `days_since(utc)`, `start_of_day_days_ago(n)`).
The zone is read per request through `AppState::time_zone()` /
`AppState::local_clock()` (lock-and-copy, `Tz` is `Copy`), so an
Admin → System save applies to the next page without restart. No call
site adds an offset itself; the SQL compares UTC instants only (MariaDB
time-zone tables are not assumed).

**AC1.** Form placed in the Language section (renamed "Language and time
zone" in the four locales): text input with a `<datalist>` of all IANA
names; invalid → 400 + terse field error `error.system.time_zone_invalid`
(form keeps the typed value); success → `success.system.time_zone_saved`.

**AC3 — date sites converted (web):**
1. Title page / volume page "Added on / Modified on" (`utils::RecordDates::new(.., tz)`).
2. Browse "Added" column (home page table, HTMX search fragment) (`SearchResult::added_date(locale, tz)`).
3. `/loans` list and scan-to-find card — loan date (`LoanWithDetails::loaned_on`) and day count.
4. Borrower page — loan dates and day counts.
5. Home dashboard — overdue list and recent-returns list day counts.
6. Volume page loan-status badge date (librarian and anonymous variants).
7. Admin → Reference data loanable-warning modal — sample loan dates.
8. Wish list page + wish list detail — "Added" (`WishlistItem::created_on(tz)`).
9. Wish list print page "generated at"; PDF export "generated on" and file name date.
10. Proposals list "filed", proposal detail "filed" / "decided", line "edited by … on …", removed line "… on …" (`display_utc` removed — dates no longer suffixed "UTC").
11. Admin → Trash "deleted at".
12. Admin → Users "created" and "last login".
13. Admin → API keys "created" and "last used".
14. Admin → Health provider "last checked" (was "… UTC").

**AC4 — day bounds.** `added_since=YYYY-MM-DD` → `day_start_utc(date, tz)`
(`home_added_since::lower_bound`). Overdue: `DATEDIFF(NOW(), loaned_at) > n`
replaced by `loaned_at < clock.start_of_day_days_ago(n)` (count and list);
`duration_days` computed in Rust (`LocalClock::days_since`) instead of
`DATEDIFF` in UTC; the "duration" sort of `/loans` now orders by
`loaned_at` with the direction flipped. Wish list dates: see AC3 8–9.
`Local::now()` / `chrono::Local` no longer appear anywhere in `src/`.

**AC5.** API v1 and MCP output untouched (`rfc3339_utc`). MCP
`search_titles.added_since` description and `mcp::tools::args::added_since`
doc state that a bare date is the UTC day and an offset is accepted
(e.g. `2026-10-31T00:00:00+01:00`); behaviour was already that.

**AC6 — tests.**
- Unit: `utils::local_time` (11 tests: half past midnight, winter/summer bounds, spring-forward 23 h day, fall-back 25 h day + repeated hour, a day without midnight (America/Santiago), `days_since`, overdue bound, invalid names); `services::admin_system::validate_time_zone_accepts_iana_names_only`; `routes::admin_system_time_zone` form render; `RecordDates`, `SearchResult::added_date`, `home_added_since::lower_bound`, `map_loan_sort_column`, seed-instant parsing.
- DB: `config::load_from_db_time_zone_seeded_valid_and_invalid`; `tests/date_added.rs::a_title_entered_at_half_past_midnight_in_zurich_is_added_that_day` (23:30 UTC on 4 Nov is "added since 5 Nov" in Zurich, not as a UTC day); `tests/dashboard_overdue.rs::overdue_counts_local_days_in_the_instance_zone` (local day count, overdue bound, list date, duration sort).
- E2E: `tests/e2e/specs/journeys/instance-time-zone.spec.ts` (blank browser → loan seeded at 23:30 UTC on 15 Jan → borrower page shows 16 Jan under the default zone → unknown zone refused → "UTC" saved → 15 Jan at once → zone restored). The TEST_MODE `/debug/seed-overdue-loan` seam gained an optional exact `loaned_at`.

### Deviations

- AC1 "invalid → keep previous + warn on load": an invalid stored row warns and reads as the default `Europe/Zurich`, like every other setting (`load_from_db` rebuilds from `AppSettings::default()`; at boot there is no previous value). A save can never store an invalid name.
- Recent cataloged / recent returns windows stay rolling 7 × 24 h (`NOW() - INTERVAL 7 DAY`), not day-based — unchanged by design (AC4 "if day-based"). The trash "days remaining" stays a rolling 30-day countdown to the purge.
- Display formats unchanged (ISO `YYYY-MM-DD` / `YYYY-MM-DD HH:MM`, localized dates where they already were); only the zone changed. Times no longer carry a "UTC" suffix.
- E2E specs `date-added` and `epic13-smoke` keep UTC days for the web filter (valid in Zurich and in UTC while the new spec runs); comments updated.

### Gate results

- `cargo check` — clean.
- `mem-guard cargo clippy --all-targets -- -D warnings` — clean.
- `cargo sqlx prepare --check --workspace -- --all-targets` (test DB) — clean (no `.sqlx/` change: the touched queries are dynamic `sqlx::query`).
- `cargo test --test locale_parity` — 3 passed.
- Full Rust suite with the DB (`mem-guard cargo test --no-fail-fast`) — 2462 passed, 0 failed, 0 ignored (89 test binaries).
- `npx tsc --noEmit` — clean; `waitForTimeout(` grep — no hit.
- Full Playwright suite on a freshly reset stack (`CI=true --workers=2`) — 444 passed, 2 skipped, 0 failed (3.1 min).
- `chrono-tz 0.10` (`default-features = false, features = ["std"]`): first build of the crate ≈ 20 s (bundled tzdb, no build-time download); `cargo check` of the workspace unaffected afterwards.

### File List

- Cargo.lock
- Cargo.toml
- locales/de.yml
- locales/en.yml
- locales/fr.yml
- locales/it.yml
- (new) migrations/20261104120000_settings_time_zone.sql
- src/config.rs
- src/lib.rs
- src/mcp/tools/args.rs
- src/mcp/tools/search_titles.rs
- src/models/admin_audit.rs
- src/models/loan.rs
- src/models/title_search.rs
- src/models/user.rs
- src/models/wishlist.rs
- src/routes/admin.rs
- src/routes/admin_api_keys.rs
- src/routes/admin_reference_data.rs
- src/routes/admin_system.rs
- (new) src/routes/admin_system_time_zone.rs
- src/routes/admin_users.rs
- src/routes/borrowers.rs
- src/routes/catalog.rs
- src/routes/home.rs
- src/routes/home_added_since.rs
- src/routes/home_search_fragment.rs
- src/routes/loans.rs
- src/routes/mod.rs
- src/routes/proposals.rs
- src/routes/titles.rs
- src/routes/wishlist.rs
- src/services/admin_system.rs
- src/services/proposal_review.rs
- src/services/wishlist_pdf.rs
- src/utils.rs
- (new) src/utils/local_time.rs
- templates/components/browse_table_row.html
- templates/fragments/admin_system_panel.html
- (new) templates/fragments/admin_system_time_zone_form.html
- templates/fragments/admin_trash_panel.html
- templates/fragments/admin_users_row.html
- templates/pages/borrower_detail.html
- templates/pages/home.html
- templates/pages/loans.html
- templates/pages/wishlist.html
- templates/pages/wishlist_detail.html
- tests/dashboard_overdue.rs
- tests/dashboard_recent_activity.rs
- tests/date_added.rs
- tests/e2e/helpers/loans.ts
- tests/e2e/specs/journeys/date-added.spec.ts
- tests/e2e/specs/journeys/epic13-smoke.spec.ts
- (new) tests/e2e/specs/journeys/instance-time-zone.spec.ts
- _bmad-output/implementation-artifacts/15-1-instance-time-zone.md
