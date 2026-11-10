# Story 17.5: Find a title by its id

Status: done

GitHub issue: #653 (type:change-request). Covers: FR176, UX-DR44 (title id). Epic 17; ships in v1.27.0.

## Story

As a librarian following up what the agent reports ("990 is a duplicate of 989"),
I want to reach a title from its number,
so that I can act on it without guessing a URL.

## Context

The MCP agent names titles by their database id (`get_title`, `search_titles` return `id`). The web UI shows no id and its search field cannot find one: the only way to title 990 is typing `/title/990` in the address bar.

The home search field (`templates/pages/home.html`, `#search-field`, `name="q"`) searches as you type (debounce 100 ms, at least 2 characters) through `GET /?q=…` → `routes/home.rs` → `SearchService::search` (`services/search.rs`), which detects codes first (`detect_code`: V-code, L-code, ISBN) and lists an exact hit **ignoring the filters**; an L-code redirects. A fast burst + Enter (scanner) goes to `GET /scan` (`routes/home_scan.rs`), which redirects to the record of a known code and otherwise to `/catalog?code=…`.

Story 17-3 (paused on its own branch) changes `detect_code` and the title forms: this story stays out of `detect_code` so 17-3 rebases cleanly.

## Decisions

- **D1 — shape: `#` then the id (positive shape, Rule #20).** `#` followed by 1 to 20 ASCII digits (a `u64`; above `u64::MAX` it is text), the first one not `0` (`#990`, `#1`). Nothing else is an id: a bare number (`990`, `1984`) stays a text search — a year or a number in a title is common, and a bare number of 8–13 digits can be an ISBN / UPC. `#` is never part of an ISBN, a UPC, a V-code or an L-code, so the shape cannot collide with them. No space between `#` and the digits; leading/trailing spaces are trimmed as for every query. `#0990`, `#`, `#99a`, `# 990` are plain text.
- **D2 — the search lists, `/scan` opens.** The live search must never navigate while the user is typing (`#9` on the way to `#990`), so the search field lists the title as its single result, like an exact ISBN / V-code hit, ignoring the genre / state / "added since" / no-cover / no-volume filters. `GET /scan` (Enter after a fast burst) opens `/title/{id}` for a live title; for a miss it goes to `/?q=%23<id>`, where the search explains the miss (instead of `/catalog?code=#990`, where it would be refused as an unknown code).
- **D3 — a miss is never silently empty.** `SearchOutcome::TitleIdMiss { id, in_trash }`: a title in the Trash (soft-deleted, not yet purged) gets "Title #990 is in the Trash, so the search does not list it. An administrator can restore it from Admin → Trash." (the UI's own labels in each locale); an unknown or purged id gets "No title has the ID #990: it may have been permanently deleted, or the number is wrong. Check the number, or type your search without the # to search titles, authors and ISBNs." (Rule #9). Both replace the empty state (no "Add this title" call to action), whatever the filters. The Trash is staff business: below Librarian a trashed id gets the "unknown" message (review round 1), so an anonymous visitor cannot list deleted ids. A failed Trash check degrades to "unknown".
- **D4 — the title page shows the id.** A small line above the title heading, outside `#title-metadata` (which the edit form swaps): "Title ID #990" — the same `#990` the search field accepts. 
- **D5 — scope.** Web search field and home scan only. The REST API v1 (`/api/v1/titles?q=`) treats a miss as an empty list, as it already does for a redirect; the MCP `search_titles` (`search_catalog`) is unchanged (the agent has `get_title { id }`).

## Acceptance Criteria

1. **AC1.** Typing `#<id>` of a live title in the home search field lists that title alone, with any filter active.
2. **AC2.** `#<id>` of a title in the Trash shows the Trash message; an unknown id shows the not-found message; neither shows the "Add this title" call to action; on HTMX and on direct navigation (`/?q=%23990`).
3. **AC3.** `GET /scan?q=%23<id>` redirects to `/title/<id>` for a live title, to `/?q=%23<id>` otherwise.
4. **AC4.** `/title/<id>` shows "#<id>" (`#title-id`), in all four locales.
5. **AC5.** Bare numbers, ISBNs, V-codes, L-codes behave as before.
6. **AC6.** Unit tests (shape parser), DB integration (search outcome live / trash / unknown, filters ignored; home scan redirect), E2E (`title-id-search.spec.ts`, on the spec's own titles, Rule #23).

## Tasks

- [x] `services/title_id_search.rs`: `parse_title_id` (shape) + `lookup` (live / trash / unknown) + unit tests.
- [x] `TitleModel::is_in_trash`.
- [x] `SearchService::search`: id arm before `detect_code`; `SearchOutcome::TitleIdMiss`; api_v1 maps it to an empty list.
- [x] Home: miss message on the HTMX fragment and the full page.
- [x] `/scan`: id arm.
- [x] Title page: `#title-id`.
- [x] Locale keys in en/fr/de/it.
- [x] DB integration test, E2E spec.

## Dev Notes

- No `sqlx::query!` macro: dynamic queries only, so no `.sqlx/` change.
- `home.rs` is already above 2000 lines (pre-existing): keep the additions there to plumbing; rendering goes in `routes/home_search_fragment.rs` (`empty_copy`, the one source of the empty-state copy).
- E2E: new spec id (check `specIsbn("` usage), `loginAs`, no `waitForTimeout`; delete a title through its own UI to put it in the Trash.

## Dev Agent Record

### File List

- `src/services/title_id_search.rs` (new) — `parse_title_id`, `TitleIdMiss`, `lookup` + unit tests.
- `src/models/title.rs` — `TitleModel::is_in_trash`.
- `src/services/search.rs` — id arm before `detect_code`; `SearchOutcome::TitleIdMiss`.
- `src/routes/api_v1.rs` — a miss is an empty list.
- `src/routes/home.rs`, `src/routes/home_search_fragment.rs`, `templates/pages/home.html` — `empty_copy` (one source of the empty-state copy), the miss message on the HTMX fragment and the full page, filters ignored.
- `src/routes/home_scan.rs` — id arm.
- `src/routes/titles.rs`, `templates/pages/title_detail.html` — `#title-id`.
- `locales/{en,fr,de,it}.yml` — `empty.title_id_*`, `title_detail.title_id`.
- `tests/title_id_search.rs` (new) — DB integration; `tests/{date_added,search_filter_browse,search_isbn10}.rs` — new outcome arm.
- `tests/e2e/specs/journeys/title-id-search.spec.ts` (new, spec id `TI`).

### Review

- **Round 1** (Blind Hunter, Edge Case Hunter, Acceptance Auditor). Medium, dismissed: "an empty Add-this-title link on a miss" (the `status_message` macro renders no CTA when label or URL is empty); "type-ahead looks up every prefix" (each intermediate answer is right for what is typed, like any type-ahead; recorded in #667). Low, fixed: anonymous visitors no longer learn which ids are in the Trash; a failed Trash check degrades to "unknown"; ids up to 20 digits (any `u64`, as the title page shows them); the unknown-id copy says how to search text (without the `#`); "Title ID" in English; the bare-number integration test has a positive control; the E2E bare number is no longer a valid ISBN-10 and the URL check is exact. Low, deferred to #667: API v1 paging/genre on an id hit, the highlighted filter chip and dropped sort on a full-page hit, literal `#42` text searches, DE/IT and other-filter coverage, `#title-id` above the `<h1>`.
- **Round 2** (combined pass). No Medium+; round-1 fixes verified (no path leaks the Trash to an anonymous visitor). Low, fixed: the full-page miss tested with a filter, the anonymous test asserts the exact "unknown" copy on the fragment and the full page, "permanently deleted" (UI vocabulary), "Title ID" in the spec and fixtures, the E2E bare-number check no longer asserts a library-wide "No matches" (Rule #23).

### Change Log
