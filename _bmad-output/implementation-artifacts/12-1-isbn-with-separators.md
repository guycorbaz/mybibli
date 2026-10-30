# Story 12.1: ISBN with hyphens or spaces

Status: ready-for-dev

GitHub issue: #502 (type:code-review-finding, deferred from the 11-1 review). Covers: FR148. Epic 12 — first story; ships in v1.22.0 at the epic close.

## Story

As a librarian,
I want an ISBN typed as printed on the book to be recognised,
so that typing `978-2-205-00121-1` finds or creates the same title as scanning it.

## Existing-code reality check (code map, 2026-10-30)

| Place | Today | Problem |
|---|---|---|
| `src/routes/catalog.rs::detect_code_type` (l. 378) | classifies the code as given (`code.trim()` in `handle_scan`, l. 452): `isbn` only for 13 bare digits `978/979…`, then a valid bare ISBN-10 (11-1), then `issn` / `upc` / `unknown` | `978-2-205-00121-1` and `2-205-00121-3` fall to `unknown` → "unsupported code" warning |
| `src/routes/catalog.rs::handle_scan` (l. 462) | rebinds `code` to `detection.normalized_code` once, right after detection (11-1) | the rebinding point already exists — a compacted form only has to come back through `normalized_code` |
| `src/routes/home_scan.rs::handle_home_scan` (l. 80–90) | reuses `detect_code_type` + the same rebinding | inherits the fix for free |
| `src/services/search.rs::detect_code` (l. 19) | classifies the trimmed, upper-cased query; called by the web search `SearchService::search` (l. 156) with the raw `trimmed` query | a hyphenated ISBN becomes `Text`; the full-text search does not match the title stored under `9782205001211` |
| `src/services/search.rs::compact_code` (l. 368, private) | **already exists** (story 11-7, MCP `search_titles`): strips whitespace, `-` and Unicode hyphens/dashes `U+2010–U+2015`, and returns the compacted string only when it is ISBN-shaped (13 digits, or 9 digits + digit/`X`/`x`); otherwise the query unchanged | used only by `search_catalog` (l. 279) — the scan and the web search do not use it |
| `src/services/expected_tomes.rs::normalize_isbn` (l. 240) | a **second, narrower** separator rule: removes `-` and whitespace only (not the Unicode hyphens/dashes), then converts / validates | two copies of "what is an ISBN separator" — Rule #1 |
| `static/js/scan-field.js::detectPrefix` | `978-2-…` matches neither `^(978\|979)\d{10}$` nor `^\d{8,13}$` → `unknown`, which is **not** blocked client-side (only `isbn` and `vcode` are pre-validated) | no change needed — the server is authoritative (same conclusion as 11-1) |

Data model: unchanged (`titles.isbn` holds 13 bare digits). No migration. No new user-facing string (no locale change).

## Acceptance Criteria

1. **AC1 — scan, ISBN-13 with separators.** Given an ISBN-13 typed with hyphens and/or spaces (`978-2-205-00121-1`, `978 2 205 00121 1`, a Unicode hyphen `U+2010`, a non-breaking space), when it is entered on `/catalog`, then it is classified `isbn`, `normalized_code` carries `9782205001211`, and `handle_scan` treats it exactly as the bare code: existing title returned ("already in your catalog") or title created with `titles.isbn = 9782205001211` and fetched.
2. **AC2 — scan, ISBN-10 with separators.** Given an ISBN-10 typed with separators (`2-205-00121-3`, `2 205 00121 3`, an `X`/`x` check digit after a hyphen), it is classified `isbn` and `normalized_code` carries its ISBN-13 — the 11-1 conversion applied to the compacted form.
3. **AC3 — no new false positive.** A code that is not an ISBN once its separators are removed keeps today's classification **of the raw input** (`unknown` for anything with separators): a hyphenated 13-digit code that does not start with `978`/`979` (e.g. `400-1234-567890`), a hyphenated ISBN-10 shape with a bad mod-11 checksum, a hyphenated 977 ISSN-EAN, a title-like text with hyphens (`Spider-Man 3`). Bare codes (no separator) are classified exactly as before — every existing `detect_code_type` test passes unchanged. A hyphenated ISBN-13 with a bad EAN check digit is classified `isbn` like its bare form today, and is then refused by `create_from_isbn` with the existing invalid-ISBN feedback.
4. **AC4 — home scan.** `GET /scan?code=978-2-205-00121-1` (and the URL-encoded spaced form) redirects to the title stored under `9782205001211`; the `/catalog?code=` fallback for an unknown ISBN carries the bare ISBN-13.
5. **AC5 — web search.** A home search (`/?q=`) by an ISBN-13 or ISBN-10 typed with separators finds the title stored under its ISBN-13. When no title matches, the full-text fallback runs on the query **as typed** (today's behaviour — `fulltext_search(pool, trimmed, …)`), so a hyphenated text query is unaffected.
6. **AC6 — one helper (Rule #1).** `compact_code` moves from `services/search.rs` to `src/utils.rs` (next to `isbn10_to_isbn13`, e.g. `pub fn compact_isbn(input: &str) -> String`, same contract) and is used by `detect_code_type`, `SearchService::search` and `search_catalog`. The separator set itself lives in one place (e.g. `utils::strip_isbn_separators(&str) -> String`, used by `compact_isbn` **and** by `expected_tomes::normalize_isbn`, which thereby also accepts Unicode hyphens/dashes — a harmless widening, its other rules unchanged). No other copy of the separator rule. The existing unit tests move or stay green.
7. **AC7 — MCP unchanged.** `search_titles` keeps its 11-7 behaviour (its tests stay green); it now calls the shared helper.
8. **AC8 — tests.** Unit: the moved `compact_isbn` tests plus `detect_code_type` cases for AC1–AC3 (hyphens, spaces, Unicode hyphen, non-breaking space, ISBN-10 with `X` after a hyphen, the AC3 negatives, bare codes unchanged) and `detect_code` / web-search arm cases. DB-backed integration: web search by a hyphenated ISBN-13 and ISBN-10 (`tests/search_isbn10.rs` or a new `tests/search_isbn_separators.rs`), home-scan redirect by a hyphenated ISBN (`tests/home_scan_redirect.rs`). E2E (new spec, specId `IH`): a hyphenated ISBN-13 typed in `#scan-field` creates the title under its bare ISBN-13 (skeleton shows the bare form, no media-type selector); the same ISBN typed with spaces returns the existing title; a hyphenated ISBN-10 is recognised; a home search by the hyphenated form opens/finds the title.
9. **AC9 — no split.** `catalog.rs` is above Rule #12 (3,104 lines); additions there stay minimal (one compaction step in `detect_code_type` + unit tests). No split in this story.

## Tasks / Subtasks

- [ ] T1 — `src/utils.rs`: move `compact_code` → `pub fn compact_isbn(input: &str) -> String` (same body, same doc, mention FR148/#502 and 11-7), move its unit test; `services/search.rs::search_catalog` calls it (AC6, AC7)
- [ ] T1b — `utils::strip_isbn_separators` shared by `compact_isbn` and `expected_tomes::normalize_isbn` (AC6); `normalize_isbn` tests stay green, add one with a Unicode hyphen
- [ ] T2 — `detect_code_type` (AC1–AC3, AC9): compute `compact = utils::compact_isbn(code)`; when `compact != code`, classify the compact form and **keep that result only if it is `isbn`** (ISBN-13 `978/979` + 13 digits, or a valid ISBN-10 → ISBN-13), returning it with `normalized_code = Some(<bare ISBN-13>)`; otherwise fall through to today's classification of the raw `code`. Bare inputs take today's path untouched. Unit tests.
- [ ] T3 — `handle_scan` / `home_scan.rs`: no code change expected (they already rebind on `normalized_code`) — verify, and log the raw and normalised forms as 11-1 does (AC1, AC4)
- [ ] T4 — `SearchService::search` (l. 156): `detect_code(&utils::compact_isbn(trimmed))`; every fallback keeps `trimmed` (AC5). Unit test on `detect_code` with compacted input; DB integration tests
- [ ] T5 — E2E `tests/e2e/specs/journeys/catalog-isbn-separators.spec.ts` (specId `IH`): reuse the `scanExpectingIsbn` / `titleExistsEntries` idiom of `catalog-isbn10.spec.ts` (rerunnable against a non-reset stack); build the hyphenated forms from `specIsbn("IH", n)` / `specIsbn10`-style derivation (AC8)
- [ ] T6 — gates: clippy `--all-targets -D warnings`, targeted `cargo test` (lib `utils::`, `routes::catalog::`, `services::search::`; DB tests `search_*`, `home_scan_redirect`), `tsc --noEmit`, flake grep, targeted E2E; full suites once on the final code before the single push

## Dev Notes

- **Compaction rule (do not change it):** `compact_isbn` removes `char::is_whitespace` (covers U+00A0 non-breaking space and U+202F narrow NBSP) and `-` plus `U+2010..=U+2015` (hyphen, non-breaking hyphen, figure dash, en dash, em dash, horizontal bar), and returns the compact form **only** when something was removed and the result is ISBN-shaped. That shape test is deliberately loose (any 13 digits); `detect_code_type` must therefore re-check that the compact form really classifies as `isbn` before using it (AC3) — otherwise `400-1234-567890` would turn from `unknown` into `upc`, a behaviour change nobody asked for.
- **Why keep the raw classification for non-ISBN compactions:** a typed EAN/UPC with separators is not a real input path (scanners never emit separators — #502), and the media-type selector a `upc` classification triggers would be a new UI path to test. Out of scope.
- **ISBN-10 after compaction:** call `utils::isbn10_to_isbn13(&compact)`; it already accepts `x`. `detect_code` (search) upper-cases its input, the scan path does not — pass the compact form as is.
- **Order in `detect_code_type`:** V-code / L-code checks first are unaffected (a 5-char `V0042` has no separator, and `compact_isbn` never returns a 5-char string). Put the compaction attempt right before the existing ISBN-13 branch.
- **`normalized_code` contract (11-1):** `Some` only when the scanned form is rewritten. A hyphenated ISBN-13 is rewritten (separators dropped) → `Some(bare)`. A hyphenated ISBN-10 → `Some(isbn13)`.
- **Web search full-text fallback:** `SearchService::search` passes `trimmed` to every `fulltext_search` call; compacting only the argument of `detect_code` keeps that. The MCP `search_catalog` already does exactly this (l. 279) — mirror it.
- **Out of scope (record, do not fix):** ISBN fields of the manual title form, the title-edit ISBN (#272), the wishlist "By ISBN" form, expected-tome ISBN input (11-10 reuses `isbn10_to_isbn13` — check whether it would benefit; if so, note it in the review, do not widen this story). User-manual wording goes with Story 12.7.
- **Rule #12:** `catalog.rs` (3,104 lines) grows by a few lines of production code plus unit tests; report the delta in the completion notes (11-1's notes were corrected for claiming a shrink).

### Regression map

- `detect_code_type` tests in `catalog.rs` use bare codes only → unchanged by construction (the compaction path is taken only when `compact != code`).
- `expected_tomes.rs` tests (`normalize_isbn_*`) must pass unchanged — only the separator set widens.
- `search.rs` tests: `detect_code` cases are bare; `compact_code_strips_separators_from_isbn_shapes_only` moves to `utils.rs` under its new name — adjust the call, keep every assertion.
- MCP tests (`tests/mcp_*`, `mcp-read-tools.spec.ts`) that search by a hyphenated ISBN must stay green — they exercise the moved helper.
- E2E wording (from 11-1): EN "This title is already in your catalog." / FR "Ce titre est déjà dans votre catalogue." → `/already in your catalog|déjà dans votre catalogue/i`.
- Typing in `#scan-field`: `scanner.ts::simulateTyping` or `fill` + `press("Enter")` (the 11-1 spec uses `fill`); `scan-field.js` submits on Enter after `trim()` — inner spaces are kept and reach the server.

### Testing standards

- Unit tests alongside code (`#[cfg(test)] mod tests`); DB tests with `#[sqlx::test(migrations = "./migrations")]`.
- E2E: `loginAs(page)` per test, unique ISBNs via specId `IH` (not used by any spec today), no `waitForTimeout`, i18n-aware regexes, rerunnable against a non-reset stack.
- Heavy commands under `mem-guard`; never run `cargo fmt` on the whole repository (it reformats ~146 files) — format touched files only if at all.

### References

- `_bmad-output/planning-artifacts/epics.md` § Epic 12, Story 12.1, FR148
- Issue #502; story 11-1 (`_bmad-output/implementation-artifacts/11-1-isbn10-at-scan.md`) for the rebinding pattern and E2E idiom; story 11-7 for `compact_code`
- `CLAUDE.md` Foundation Rules #1, #2, #3, #12, #13

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created

### File List
