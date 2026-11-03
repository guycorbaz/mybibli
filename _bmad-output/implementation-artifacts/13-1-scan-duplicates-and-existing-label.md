# Story 13.1: Scan duplicates and an existing label

Status: done

GitHub issues: #574 (type:bug), #585 (type:bug). Covers: bug fixes. Epic 13 — first story; ships in v1.23.0 at the epic close.

## Story

As a librarian scanning a pile of books,
I want a double read of the scanner to be ignored, and a label I scan again to tell me where it is,
so that I am never shown an error for something I did right.

## Evidence (production logs, 2026-10-31 → 2026-11-01, v1.22.0)

16 "Le label Vxxxx est déjà assigné à …" refusals, all for labels the librarian's own scans had created shortly before:

- **Double read** (V1315, V1398, V1465): the same label posted twice 0.17–1 s apart; the first created the volume, the second was refused.
- **Pair re-scanned, same title** (V1403, V1407, V1408, V1409, V1414, V1426): ISBN + label scanned again 1–2 min later; the ISBN returns the existing title, the label is refused as if it belonged elsewhere.
- **Label under another title** (V1527–V1529 under title 504 — a publisher EAN shared by several albums; V1364 under 403 instead of 404; V1320–V1322 re-scanned under 325, they belong to 326–328): the refusal names the owner, but with the bare ISBN when metadata has not arrived, and offers no way forward.

## Existing-code reality check (code map, 2026-11-03)

| Place | Today | Problem |
|---|---|---|
| `static/js/scan-field.js::initScanFields` (l. 91–148) | every Enter on a non-empty field posts `/catalog/scan` via `htmx.ajax`, then clears the field | a scanner double read posts the same code twice |
| `src/routes/catalog.rs::handle_scan`, `"vcode"` arm (l. 580) | existing label + active location → shelve (l. 587–700); otherwise falls through to the title-context check and `VolumeService::create_volume` | an existing label with a title in context always tries to **create** a volume and fails |
| `src/services/volume.rs::resolve_duplicate_label` (l. 98–121) | live volume → `AppError::BadRequest(feedback.volume_duplicate)` naming the owner title text | same message whether the owner is the title in context or another one |
| `src/routes/catalog.rs` error arm (l. 1004–1005) | `tracing::error!("V-code scan failed")` for every error, BadRequest included | a user-input refusal counted as an ERROR |
| CR #300 phantom-volume modal (l. 726–800) | fires only for a **new** label (`!existing_vol_was_some`) | unchanged |

`catalog.rs` is 3,177 lines (above Rule #12): add the new branch as a small helper in a sibling module (e.g. `src/routes/catalog_existing_label.rs`) rather than inline.

## Acceptance Criteria

1. **AC1 — double read ignored (client).** In `scan-field.js`, a code identical to the previous submitted code, submitted again within `DUPLICATE_SCAN_WINDOW_MS` (1000 ms, a named constant) of it, is not posted; the field is cleared; no feedback is added (a discreet `aria-live` note is acceptable, not required). A different code, or the same code after the window, is posted normally. Applies to every code type.
2. **AC2 — existing label, same title.** Given a V-code that labels a live volume of the title in context and no active location, when it is scanned, then no creation is attempted and the feedback is an **info** entry "Label V… is already recorded for this title." (new key `feedback.volume_already_recorded`, all four locales). The context banner is unchanged.
3. **AC3 — existing label, other title.** Given a V-code that labels a live volume of another title, when it is scanned with a title in context, then no creation is attempted and the feedback is a **warning** naming the owner title and its ISBN when it has one (`"V1527" is already on "Les Gorilles…" (ISBN …)`), linking to `/volume/{id}`, and saying the volume can be moved from its page (wording prepared for Story 13.8; until 13.8 ships, "open the volume to check it"). New key `feedback.volume_on_other_title`. The phantom-volume modal is not involved.
4. **AC4 — unchanged paths.** Existing label + active location still shelves; new label still goes through CR #300 and `create_volume`; a soft-deleted label still goes through `resolve_duplicate_label` (#442 reuse rules); no title in context still answers `volume_no_title`.
5. **AC5 — logging.** In the V-code arm, `AppError::BadRequest` / `Conflict` from `create_volume` are logged at `warn`, others stay `error`. The AC2/AC3 branches log at `info`.
6. **AC6 — HTML safety.** Title text and ISBN are `html_escape`d before entering the feedback (CSP: no inline handlers; the link is a plain `<a href>`).
7. **AC7 — tests.** Unit: the pure decision (`same title` / `other title`) and the message building. DB integration (`tests/scan_title_context.rs` or a new `tests/scan_existing_label.rs`): same-title re-scan → info, no new volume; other-title re-scan → warning naming the owner, no new volume, volume unchanged. E2E (new spec, specId `DL`): (a) a label submitted twice within 200 ms creates one volume and shows no error; (b) the ISBN + label pair scanned again shows the info entry; (c) a label scanned under another title shows the warning with a link to the volume. No `waitForTimeout`.

## Tasks / Subtasks

- [x] T1 — `scan-field.js`: `DUPLICATE_SCAN_WINDOW_MS`, remember `{code, at}` of the last post; skip identical within the window (AC1)
- [x] T2 — `catalog.rs` V-code arm: after the active-location branch and the title-context checks, if `existing_vol_lookup` is `Some`, delegate to the new helper (AC2–AC4); keep the delta in `catalog.rs` minimal
- [x] T3 — helper module: decision + message, `html_escape`, owner ISBN lookup (AC3, AC6)
- [x] T4 — logging levels (AC5)
- [x] T5 — locale keys in en/fr/de/it; `touch src/lib.rs`; `cargo test --test locale_parity`
- [x] T6 — tests (AC7)
- [x] T7 — gates: clippy `--all-targets -D warnings`, targeted `cargo test`, `sqlx prepare --check`, templates audit, `tsc --noEmit`, flake grep, targeted E2E; full suites once on the final code before the single push

## Dev Notes

- The double read is client-side only: both posts go through the same field handler, so a timestamp in the closure suffices; no session write per scan (the DB is slow during the nightly backups).
- `existing_vol` already carries `title_id`; the owner title comes from `TitleModel::find_by_id` (`title`, `isbn`).
- Do not change `resolve_duplicate_label`: it still guards the INSERT race (two tabs creating the same new label).

## Completion Notes

- **AC1** — `static/js/scan-field.js`: `DUPLICATE_SCAN_WINDOW_MS = 1000`, pure `isDuplicateScan(last, code, now)` (exposed as `window.mybibliIsDuplicateScan`), per-field `{code, at}` of the last post; an identical code within the window is dropped and the field cleared, silently. The field also carries `data-last-scan-at` so E2E specs that deliberately re-scan the same code can wait the window out (`waitForDuplicateScanWindow` in `tests/e2e/helpers/scanner.ts`, a `waitForFunction`, no `waitForTimeout`).
- **AC2/AC3/AC6** — new `src/routes/catalog_existing_label.rs` (209 lines): `decide` (pure), `owner_label` (title + ISBN; the ISBN is not repeated when the title text still IS the ISBN, before metadata arrives), `same_title_feedback` (info), `other_title_feedback` (warning + plain `<a href="/volume/{id}">`), `existing_label_response`. Escaping is done once, by `feedback_html` / `feedback_html_with_link` (they escape message, hint, href and link label; pre-escaping would double-escape). The "open the volume to check it" advice is its own key, `feedback.volume_on_other_title_hint`, so Story 13.8 only edits locales.
- **AC4** — `catalog.rs` V-code arm: the existing-volume lookup is borrowed instead of moved, and after the title-context checks an existing live label returns through the helper. The CR #300 guard condition lost its `!existing_vol_was_some` (always true at that point now). Shelve, new-label, soft-deleted-label (#442) and no-title paths unchanged.
- **AC5** — `create_volume` errors: `BadRequest`/`Conflict` → `warn`, others `error`; the new branches log `info`.
- **Locales** — `feedback.volume_already_recorded`, `volume_on_other_title`, `volume_on_other_title_hint`, `volume_open_link`, `volume_owner_with_isbn` in en/fr/de/it.
- **Tests** — 9 unit tests in the new module; `tests/scan_existing_label.rs` (4 DB tests: same title → info + nothing written; other title → warning naming escaped owner + ISBN + link, volume unchanged; new label still created; no title context still `volume_no_title`); E2E `scan-duplicates-existing-label.spec.ts` (specId `DL`, V0590–V0592).
- **Deviation / side effect** — the client guard applies to every code type (as specified), so existing specs that re-scanned the same code right after the first scan would have had the second scan dropped. Seven sites got a `waitForDuplicateScanWindow(page)` (catalog-cd-sequence, catalog-metadata ×2, catalog-title, cover-image, provider-chain ×3). Two tests asserted the old "already assigned" error on a same-title re-scan and were rewritten to the new info behaviour (catalog-volume AC2, catalog-metadata AC6).
- **File size** — `src/routes/catalog.rs` is 3,191 lines (was 3,177; +14, mostly comments); still above Rule #12 as before.

### Review Findings

Round 1 (independent reviewer, on f30e7f3): 0 High, 1 Medium, 5 Low.

- [x] [Review][Patch] M1 — a failed shelve or a deleted active location fell through to the #585 branch and showed "already recorded" for a volume that did not move. Now a failed `update_location` returns an error (`feedback.volume_shelve_failed`, Rule #9 copy) and a vanished active location returns a warning (`feedback.active_location_gone`) after clearing it; regression test `deleted_active_location_says_not_shelved_not_already_recorded` in `tests/scan_existing_label.rs`.
- [x] [Review][Patch] L1 — a failed scan post is forgotten (`htmx:responseError` / `htmx:sendError` on `/catalog/scan`), so re-pressing Enter on the restored code is not dropped; a successful post is still remembered.
- [x] [Review][Patch] L2 — owner title unreadable → new key `feedback.volume_on_unknown_title` ("…already assigned to another title") instead of a bare "?".
- [x] [Review][Patch] L3 — `catalog-volume.spec.ts` waits for the "already in your catalog" info specifically.
- [x] [Review][Patch] L4 — wording: en/de/fr/it say "assigned to / zugeordnet / attribué à / assegnata a"; it uses "esemplare" in the hint and link.
- [x] [Review][Patch] L5 — a `Conflict` from `create_volume` now shows its own (user-facing) message at warning level instead of `error.internal`.

Round 2: 0 High/Medium, 4 Low.

- [x] [Review][Patch] L1 — `scan-field.js` forgets the last post only when the failed request's `code` (htmx 2 `requestConfig.formData`) equals it, and also on `htmx:timeout`.
- [x] [Review][Patch] L2 — the quotes enclose the owner title only; the ISBN clause (`feedback.volume_owner_isbn_suffix`, renamed from `volume_owner_with_isbn`) follows outside them in all four locales; unit test `quotes_enclose_the_title_only_with_the_isbn_outside`.
- [x] [Review][Patch] L3 — Rule #9: the hint now says what to do with the book in hand ("Scan another label for this book, or open the volume to check it.", fr/de/it equivalents, it keeps "esemplare").
- [x] [Review][Dismiss] L4 — noted, not fixed: `shelve_failed` is covered by a unit test only (a DB failure on the UPDATE is not reproducible in the integration harness); pre-existing behaviour of `VolumeModel::update_location` returning `Ok` on 0 rows affected is out of this story's scope.
