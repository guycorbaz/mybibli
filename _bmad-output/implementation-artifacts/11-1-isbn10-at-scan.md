# Story 11.1: Recognise an ISBN-10 at scan

Status: review

GitHub issue: #497 (type:bug, Medium). Covers: FR122. Epic 11 — first story (production fix, ships in v1.20.1 with 11-2 and 11-3).

## Story

As a librarian,
I want an ISBN-10 to be scanned like an ISBN-13,
so that older books and comics get their metadata and are not catalogued twice.

## Existing-code reality check (code map, 2026-10-27)

| Place | Today | Problem |
|---|---|---|
| `src/routes/catalog.rs::detect_code_type` (l. 388) | `isbn` only for 13 digits starting `978`/`979`; any other 8–13-digit code → `upc`; a trailing `X` → `unknown` | `2205001213` becomes a `upc` title with no metadata (prod titles 271, 282) |
| `src/routes/catalog.rs::handle_scan` (l. 434) | `code` is used verbatim by `TitleService::create_from_isbn`, `wishlist::autolink_feedback_html`, `skeleton_feedback_html`, `fetch_metadata_chain` | must receive the ISBN-13 |
| `src/routes/home_scan.rs::handle_home_scan` (l. 67) | reuses `detect_code_type`, then `TitleModel::find_id_by_isbn(pool, code)` and `fallback_url(code)` | must look up / forward the ISBN-13 |
| `src/services/search.rs::detect_code` (l. 19) | accepts the ISBN-10 *shape* (9 digits + digit/`X`) as `CodeType::Isbn(q)` without conversion | `TitleModel::find_by_isbn` gets the 10-char string → never matches a title stored as ISBN-13 → falls through to fulltext → **verified: a home search by ISBN-10 does not find the title today** |
| `src/services/title.rs::validate_isbn13_checksum` (l. 21) and `catalog.rs::validate_issn_from_ean` (l. 369) | two copies of the EAN-13 check-digit loop | the conversion needs the same computation — a third copy is forbidden (Rule #1) |
| `static/js/scan-field.js::detectPrefix` | classifies an ISBN-10 as `upc`/`unknown` but **does not block** submission (only ISBN-13 and V-code are pre-validated client-side); `data-detected-prefix` is read nowhere | no change needed — the server is authoritative |

Data model: titles keep ISBN in `titles.isbn` (13 chars). No migration. No new user-facing string (no locale change).

## Acceptance Criteria

1. **AC1 — valid ISBN-10 is an ISBN.** Given a valid ISBN-10 such as `2205001213`, when it is scanned on `/catalog`, then `detect_code_type` classifies it as `isbn` (media type Book, code type Isbn) and exposes its ISBN-13 `9782205001211`; `handle_scan` uses the ISBN-13 for the duplicate check, the title creation (`titles.isbn = 9782205001211`), the wishlist auto-link, the skeleton feedback and the provider chain — exactly as if the ISBN-13 had been scanned.
2. **AC2 — `X` check digit.** An ISBN-10 whose check digit is `X` (upper- or lower-case) receives the same treatment.
3. **AC3 — bad checksum.** A 10-character code whose ISBN-10 checksum fails is not taken for an ISBN: a 10-digit one stays `upc` (media-type selector), one ending in `X` stays `unknown`.
4. **AC4 — no duplicate.** Given a title already catalogued under its ISBN-13, when its ISBN-10 is scanned, then the existing title is returned (info feedback "already in your catalog") and no row is created.
5. **AC5 — one helper.** ISBN-10 checksum validation and ISBN-10→ISBN-13 conversion live in ONE helper in `src/utils.rs` (`isbn10_to_isbn13(&str) -> Option<String>`), used by both `routes/catalog.rs::detect_code_type` and `services/search.rs::detect_code`; the EAN-13 check-digit computation is shared (`utils::ean13_check_digit`) instead of copied a third time.
6. **AC6 — search.** A home-page search (`/?q=`) by a valid ISBN-10 finds the title stored under its ISBN-13. An ISBN-10-shaped query with a bad checksum keeps today's behaviour (raw lookup, then fulltext).
7. **AC7 — home scan.** `GET /scan?code=<ISBN-10>` redirects to the title stored under the ISBN-13, and the `/catalog?code=` fallback carries the ISBN-13.
8. **AC8 — tests.** Unit tests: valid ISBN-10, `X` check digit (both cases), bad checksum, wrong length / non-digit, conversion of the two production examples (`2205001213`→`9782205001211`, `2205011502`→`9782205011500`) and an `X` example; `detect_code_type` and `detect_code` cases. DB-backed integration: search by ISBN-10 and home-scan redirect by ISBN-10. E2E: an ISBN-10 scan creates the title with its ISBN-13; an ISBN-10 scan returns an existing title; an `X` ISBN-10; a bad-checksum 10-digit code is not an ISBN; a home search by ISBN-10 finds the title.
9. **AC9 — no split.** `catalog.rs` (3,050 lines, above Rule #12) is not split here; additions there stay minimal.

## Tasks / Subtasks

- [x] T1 — `src/utils.rs` helpers (AC5, AC8)
  - [x] `pub fn ean13_check_digit(first12: &str) -> Option<u32>` (12 ASCII digits, weights 1/3)
  - [x] `pub fn isbn10_to_isbn13(code: &str) -> Option<String>`: exactly 10 chars, 9 ASCII digits + digit/`X`/`x`, Σ(dᵢ·(10−i)) ≡ 0 mod 11, returns `"978" + first 9 + ean13 check digit`
  - [x] unit tests
  - [x] refactor `TitleService::validate_isbn13_checksum` and `catalog.rs::validate_issn_from_ean` onto `ean13_check_digit` (existing tests guard behaviour)
- [x] T2 — `detect_code_type` (AC1–AC3, AC9): new field `CodeDetection::normalized_code: Option<String>` (Some only when the scanned form is rewritten); ISBN-10 branch before the `upc` branch; unit tests
- [x] T3 — `handle_scan`: rebind `code` to `detection.normalized_code` right after detection; log both forms
- [x] T4 — `home_scan.rs`: same rebinding; integration test in `tests/home_scan_redirect.rs`
- [x] T5 — `search.rs::detect_code`: valid ISBN-10 → `CodeType::Isbn(isbn13)`; update `test_detect_isbn10`; integration test in `tests/search_filter_browse.rs` (or a new `tests/search_isbn10.rs`)
- [x] T6 — E2E `tests/e2e/specs/journeys/catalog-isbn10.spec.ts` (specId `IX`), helper `specIsbn10()` in `tests/e2e/helpers/isbn.ts`
- [x] T7 — gates: fmt, clippy `-D warnings`, `cargo test`, DB tests, sqlx check, flake grep, targeted then full E2E

## Dev Notes

- ISBN-10 checksum: weights 10..1 over the 10 positions, `X` = 10 and only valid in position 10; valid iff sum % 11 == 0. ISBN-13 conversion: prefix `978` (every ISBN-10 maps to 978), keep the 9 body digits, recompute the EAN-13 check digit. ISBN-10 has no 979 form.
- Canonicalisation point: rebind `code` once, right after `detect_code_type`, so every downstream use (`create_from_isbn`, autolink, skeleton text, `spawn_fetch`, error log) sees the ISBN-13 without touching each call. The `"isbn"` arm already relies on `create_from_isbn` validating the ISBN-13 checksum — a converted ISBN-13 always passes.
- `detect_code` (search) upper-cases the query, so `x` is already handled there; the scan path must accept `x` itself (`code.trim()` only).
- Client JS stays untouched: it never blocks a 10-char code, and duplicating the mod-11 logic in JS would be a second source of truth.
- `handle_scan_with_type` (UPC + chosen media type) is not reached by an ISBN-10 any more; unchanged.
- Out of scope: ISBN-10 entry in the manual title form, wishlist "By ISBN" form, title-edit ISBN (#272) — story 11.10 reuses the helper for expected tomes; production data clean-up of titles 271/282/287 is the user's (issue #497 § Data clean-up); user-manual wording goes with the epic documentation story 11-24.
- Rule #12: `catalog.rs` gains a few lines only (one branch, one struct field, one rebinding); the EAN-13 refactor removes more than it adds.

### Regression map (validated 2026-10-27)

- Every 10-character code literal in the test suites was checked against the mod-11 rule: `catalog.rs` test `9771234567` (sum ≡ 7 mod 11) stays `upc`; `scanner-guard.spec.ts` types `9782070360` into a dialog input (never submitted, and sum ≡ 1 mod 11 anyway); `search.rs` test `207036024X` has a bad checksum (stays raw `Isbn`), `0201633612` is valid → its expected value becomes `Isbn("9780201633610")`.
- No other caller builds `CodeDetection` (`home_scan.rs` only reads it), so the new field touches `detect_code_type` and its tests only.
- `create_from_isbn` re-validates the ISBN-13 checksum: the converted value must therefore be computed with the same EAN-13 algorithm — guaranteed by sharing `ean13_check_digit`.
- E2E wording: EN "This title is already in your catalog." / FR "Ce titre est déjà dans votre catalogue." → `/already in your catalog|déjà dans votre catalogue/i`.
- E2E ISBN-10 values must be derived from a 978-prefixed `specIsbn("IX", n)` (drop `978` and the EAN check digit, append the mod-11 digit) so the ISBN-13 stays unique per spec; pick one `n` whose mod-11 digit is `X` for AC2.

### Testing standards

- Unit tests alongside code (`#[cfg(test)] mod tests`); DB tests with `#[sqlx::test(migrations = "./migrations")]`.
- E2E: `loginAs(page)` per test, unique ISBNs via the `IX` specId, no `waitForTimeout`, i18n-aware regexes (`/already in your catalog|déjà dans votre catalogue/i` — check fr wording).

### References

- `_bmad-output/planning-artifacts/epics.md` § Story 11.1, FR122
- Issue #497
- `CLAUDE.md` Foundation Rules #1, #2, #3, #12, #13

## Dev Agent Record

### Agent Model Used

claude-opus-5-5

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created
- T1: `utils::ean13_check_digit` + `utils::isbn10_to_isbn13`; `TitleService::validate_isbn13_checksum` and `catalog.rs::validate_issn_from_ean` now delegate to the shared EAN-13 computation (no third copy).
- T2/T3: `CodeDetection::normalized_code`; ISBN-10 branch placed before the ISSN/UPC branches; `handle_scan` rebinds `code` once after detection and logs both forms.
- T4: `home_scan.rs` rebinds the same way — lookup and `/catalog?code=` fallback use the ISBN-13.
- T5: `search::detect_code` maps a valid ISBN-10 to `CodeType::Isbn(isbn13)`; a bad-checksum ISBN-10 shape keeps the raw lookup.
- T6: `catalog-isbn10.spec.ts` (specId `IX`) + `specIsbn10()` helper. The spec is rerunnable against a non-reset stack: a first scan accepts either the new-title skeleton carrying the ISBN-13 or one more "already in your catalog" entry, and always asserts that no media-type selector appears; "existing title" checks count entries before/after the scan.
- T7 gates (2026-10-27, run under the station-wide `mem-guard`): `cargo clippy --all-targets -- -D warnings` clean; `cargo test --no-fail-fast` against the test DB — 61 test binaries, all `ok`, 0 failed (lib: 1190 passed); `cargo sqlx prepare --check --workspace -- --all-targets` clean; flake grep clean; `tsc --noEmit` clean. E2E: targeted spec 5/5 (also twice against a non-reset stack); full suite after `e2e-reset.sh`, `CI=true --workers=2`: **340 passed, 2 skipped, 0 failed, 0 flaky**. An earlier full run (before the rerun fix) had 339 passed + 1 flaky `shelving.spec.ts:193` — known failure #499, unrelated.
- `cargo fmt --check` reports diffs repo-wide, identical on `main` (rustfmt of Rust 1.99); CI does not run fmt; left untouched to keep the diff focused.

### File List

- `src/utils.rs` — `ean13_check_digit`, `isbn10_to_isbn13`, unit tests
- `src/services/title.rs` — `validate_isbn13_checksum` on the shared helper
- `src/routes/catalog.rs` — `CodeDetection::normalized_code`, ISBN-10 branch, `handle_scan` rebinding, `validate_issn_from_ean` refactor, unit tests
- `src/routes/home_scan.rs` — rebinding
- `src/services/search.rs` — ISBN-10 → ISBN-13 in `detect_code`, updated `test_detect_isbn10`
- `tests/home_scan_redirect.rs` — 3 DB-backed tests
- `tests/search_isbn10.rs` — new, 3 DB-backed tests
- `tests/e2e/helpers/isbn.ts` — `specIsbn10()`
- `tests/e2e/specs/journeys/catalog-isbn10.spec.ts` — new, 5 E2E tests
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — 11-1 → review

### Change Log

- 2026-10-27 — Validation pass (checklist.md): added the regression map (10-char literals in the suites, `CodeDetection` construction sites, EAN-13 re-validation in `create_from_isbn`, E2E wording and ISBN-10 derivation). No blocker found.
- 2026-10-27 — Implementation T1–T7; status → review.
