# Story 17.1: A V-code is recognised whatever its case

Status: done

GitHub issue: #651 (type:bug). Covers: FR172. Epic 17; ships in v1.27.0.

## Story

As a librarian adding a volume,
I want the V-code I type or scan to be recognised,
so that a volume is never refused with a message about media types.

## Reproduction (production logs, 2026-11-08)

The production database is out of reach; the logs are not. `routes/catalog.rs::handle_scan` logs every scan at INFO (`"Processing scan"`, `code`, `code_type`).

- 18:37:20 — `Manual title created`, title 989 "La planète de l'angoisse".
- 18:37:25 and 18:37:57 — `Processing scan`, `code: "11338"`, `code_type: "unknown"` → `feedback.code_unsupported`.
- 18:39:13 — the title is created again by hand (title 990).
- 18:39:17 — `Processing scan`, `code: "11338"`, `code_type: "unknown"` again.

**The input was `11338`: five ASCII digits, no `V`, no lowercase, no invisible character** (a JSON-escaped log would show one). It is not a V-code at all. `V1338` exists since 2026-10-31 15:17 (volume created on title 357), so even `V1338` would have answered "label already used". The scans came 4–5 seconds after the title was created, which fits a scanner reading another barcode of the book (or a mistyped label) better than typing; the logs cannot tell which.

Other non-ISBN codes answered `unknown` in October–November: `V,/+` (2026-11-04 17:49:08, in the middle of a shelving run of `V15xx` scans — a V-code read in another keyboard layout), `wq2205005790`, and a series of `1074C` / `1984M`-shaped codes on 2026-11-01/02. No lowercase `v` reached the server; no `V` + 4 non-digit label either.

Plausible shapes reproduced locally (unit + DB integration tests): `v0042` → `code_unsupported` today (the server tests `starts_with('V')`, the client pre-check `/^V\d{4}$/` lets it through as "unknown"); `V,/+` and `V12345` → `code_unsupported`; `V12A4` → `vcode_invalid`; `11338` → `code_unsupported`.

## Decisions

- **D1 — one rule, positive shape (Rule #20).** `VolumeService::normalize_vcode(code) -> Option<String>`: `[vV]` followed by exactly 4 ASCII digits gives `V` + those digits; anything else gives `None` and is never rewritten. Every V-code reader calls it: `detect_code_type` (so `/catalog/scan` and the home scan `/scan`; story 17.2 will reach it through the catalog scan arm), the loan form and loan scan (`routes/loans.rs`, which upper-cased the whole input), and the search code detection (`services/search.rs`). The canonical code travels through `CodeDetection::normalized_code`, as an ISBN-10 does (story 11-1).
- **D2 — near V-codes are named.** A *near V-code* (`is_near_vcode`, positive shape): `v`/`V` then 1 to 5 printable ASCII characters, no space, at least one of them not a letter (`V12345`, `v12`, `V,/+`, `V12A4`). It is classified `vcode` and refused with `feedback.vcode_invalid` ("Expected V followed by 4 digits (e.g., V0042)"), never `code_unsupported`. Words (`Valérian`, `Vol`, `VABCD`) and spaced inputs are not near V-codes: the scan field also takes search text ("ISBN, V-code, L-code, or search"), and such text keeps the general message (review round 1).
- **D3 — stored labels checked before tightening.** The only writer of `volumes.label` is `VolumeService::create_volume`, which has refused anything but `V` + 4 digits (not `V0000`) since the initial schema; the 837 `Creating volume` log lines since 2026-10-10 all carry `V` + 4 digits. `V` + 4 non-digits was already refused, so nothing stored becomes unreachable. Since review round 1, `V` + 4 letters (`VABCD`) gets `code_unsupported` instead of `vcode_invalid`; both refuse it and both name the V-code shape.
- **D4 — the reported code gets a message naming what was expected (Rule #9).** `feedback.code_unsupported` talked about media-type disambiguation, which exists since epic 13, and was what `11338` received. It now says the code is not recognised, lists the accepted shapes (ISBN, EAN/UPC barcode, V-code V + 4 digits, L-code L + 4 digits) and points to the New title button. No guess is made about which shape a five-digit code was meant to be (Rule #20).
- **D5 — the client stays a pre-check.** `static/js/scan-field.js` keeps `/^V\d{4}$/` for its local check; a lowercase code is "unknown" there and goes to the server, which owns the rule. No second copy of the normalisation.
- **D6 — out of scope.** Lowercase L-codes (no report), invisible characters inside the code (none seen; refused as unknown with the new message).

## Acceptance Criteria

1. **AC1.** `v0042` scanned with a title in context creates the volume `V0042` (catalog scan), and the home scan `v0042` reaches `/volume/{id}`; the loan scan accepts it too.
2. **AC2.** `V12345`, `v12`, `V,/+`, `V12A4` are refused with `feedback.vcode_invalid`, not `code_unsupported`; nothing is created.
3. **AC3.** `11338` and other unrecognised codes get the reworded `code_unsupported` naming the accepted shapes, in all four locales.
4. **AC4.** Unit tests (`normalize_vcode`, `detect_code_type` shapes), DB integration (`tests/scan_vcode_case.rs`), E2E (`vcode-any-case.spec.ts`: a title created by hand, then a lowercase V-code adds its volume; a near V-code names the expected shape; focus stays in the scan field).

## Tasks

- [x] `VolumeService::normalize_vcode` + unit tests.
- [x] `detect_code_type`: V arm through `normalize_vcode`, near-V → `vcode`; unit tests.
- [x] `routes/loans.rs`, `services/search.rs`: use `normalize_vcode`.
- [x] Reword `feedback.code_unsupported` in en/fr/de/it.
- [x] DB integration test, E2E spec.

## Dev Agent Record

### File List

- `src/services/volume.rs` — `normalize_vcode`, `label_from_input` + unit tests.
- `src/routes/catalog_code.rs` — V arm through `normalize_vcode`; near V-codes stay `vcode`; unit tests.
- `src/routes/loans.rs` — loan form and loan scan use `label_from_input`.
- `src/services/search.rs` — V-code arm uses `normalize_vcode`.
- `locales/{en,fr,de,it}.yml` — `feedback.code_unsupported` reworded (Rule #9).
- `tests/scan_vcode_case.rs` — DB integration (catalog scan, home scan, loan scan).
- `tests/e2e/specs/journeys/vcode-any-case.spec.ts` — E2E journey.

### Review

- **Round 1** (Blind Hunter, Edge Case Hunter, Acceptance Auditor). Medium, fixed: any text starting with a V was refused as a malformed V-code, though the scan field also takes search text (`is_near_vcode` added); the integration and E2E checks could not tell `vcode_invalid` from the new `code_unsupported` (both mention `V0042`; now matched on their own wording). Low, fixed: Italian and German vocabulary ("codice V/L", "V-Code/L-Code", as elsewhere), American spelling in English, the button name quoted in every locale, the status of the home-scan fetch checked in the E2E. Low, deferred to #661: lowercase L-codes, the mixed-case double read not caught by the client duplicate guard, no route test of the loan form with a lowercase V-code, the random V-code range of the E2E.
- **Round 2** (combined adversarial + edge pass). No Medium+. Low, accepted: short search words with a digit or a dot (`Vol.2`, `V-2`) read as near V-codes (both answers refuse; both name the shape); `V 0042` and non-ASCII layouts stay `code_unsupported`, which also names the shape. D3 wording corrected. Pre-existing, deferred to #661: the catalog scan renders `vcode_invalid` / `code_unsupported` without the request locale.

### Change Log
