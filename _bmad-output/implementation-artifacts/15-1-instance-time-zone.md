# Story 15.1: Instance time zone

Status: ready-for-dev

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
