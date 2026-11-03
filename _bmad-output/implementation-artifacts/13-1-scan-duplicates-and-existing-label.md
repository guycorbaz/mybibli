# Story 13.1: Scan duplicates and an existing label

Status: ready-for-dev

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

- [ ] T1 — `scan-field.js`: `DUPLICATE_SCAN_WINDOW_MS`, remember `{code, at}` of the last post; skip identical within the window (AC1)
- [ ] T2 — `catalog.rs` V-code arm: after the active-location branch and the title-context checks, if `existing_vol_lookup` is `Some`, delegate to the new helper (AC2–AC4); keep the delta in `catalog.rs` minimal
- [ ] T3 — helper module: decision + message, `html_escape`, owner ISBN lookup (AC3, AC6)
- [ ] T4 — logging levels (AC5)
- [ ] T5 — locale keys in en/fr/de/it; `touch src/lib.rs`; `cargo test --test locale_parity`
- [ ] T6 — tests (AC7)
- [ ] T7 — gates: clippy `--all-targets -D warnings`, targeted `cargo test`, `sqlx prepare --check`, templates audit, `tsc --noEmit`, flake grep, targeted E2E; full suites once on the final code before the single push

## Dev Notes

- The double read is client-side only: both posts go through the same field handler, so a timestamp in the closure suffices; no session write per scan (the DB is slow during the nightly backups).
- `existing_vol` already carries `title_id`; the owner title comes from `TitleModel::find_by_id` (`title`, `isbn`).
- Do not change `resolve_duplicate_label`: it still guards the INSERT race (two tabs creating the same new label).
