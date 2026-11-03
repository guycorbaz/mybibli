# Story 13.8: Move a volume to another title

Status: ready-for-dev

GitHub issue: #580 (type:change-request). Covers: FR154, UX-DR36. Epic 13; ships in v1.23.0.

## Story

As a librarian,
I want to move a volume to the title it really belongs to,
so that a label attached under the wrong title can be fixed without losing the volume.

## Evidence (production, see #580 comments)

V1527–V1529 sit under title 504 (publisher EAN `5410983213000`, shared by several albums) instead of titles 523–525, which have no volume; V1364 sits under 403 instead of 404.

## Decision (project lead, 2026-11-03)

A volume on loan can be moved; the loan follows the volume (it keeps its label, location and history).

## Existing-code reality check (to verify at story start)

| Place | Today |
|---|---|
| `src/routes/mod.rs:248–264` | `/volume/{id}`, `/edit`, `/update`, `/delete-modal`; `/mark-audit`, `/clear-audit` (l. 376) |
| `templates/pages/volume_detail.html`, `volume_edit.html` | edit form: condition, edition comment, prices — no title |
| `volumes.title_id`, `volumes.version` | optimistic locking via `services/locking.rs::check_update_result` |
| Pattern A modal (`#modal-slot`, `static/js/modal.js`, `templates/components/modal.html` UX-DR8 macro, `HX-Trigger: modal-close`) | the delete-volume modal is the closest precedent |
| title search for a picker | `/catalog/contributors/search` (contributor typeahead) and the home search are precedents; a small `GET /titles/search?q=` fragment returning candidates is acceptable |
| loans | `loans.volume_id` — loans reference the volume, so nothing to move |
| `volumes` counters / dashboard | "volume count" per title is computed by query — verify no cached counter exists |

## Acceptance Criteria

1. **AC1 — action.** A librarian sees "Move to another title" on the volume page; it opens a UX-DR8 modal (Pattern A) with a title search (by title text or ISBN, live titles only, at most ~10 results, current title excluded) and, once a target is picked, a confirmation naming the volume, the current title and the target (all `html_escape`d).
2. **AC2 — move.** Confirming posts `POST /volume/{id}/move` (`title_id`, `version`, CSRF); the service updates `volumes.title_id` with `WHERE id = ? AND version = ?` (`version + 1`) and `check_update_result`; label, location, condition, values, entity labels, audit flag and loans (active included) stay with the volume. Success: modal closes (`HX-Trigger: modal-close`), the page shows the volume under its new title with a success FeedbackEntry, and a `tracing::info!` line records volume, from, to, user.
3. **AC3 — refusals.** Target = current title, a deleted/unknown target, a deleted volume, or a stale version → explicit localized message in the modal error region (FeedbackEntry style, Rule #9 copy).
4. **AC4 — security / a11y.** Librarian+ (`require_role_with_return`), CSRF, the modal inherits the scanner-guard invariant; no `hx-confirm`; axe-core passes on the open modal; CSP clean (templates audit).
5. **AC5 — scan feedback.** The Story 13.1 "label on another title" feedback now says the volume can be moved from its page (adjust the locale string added in 13.1).
6. **AC6 — tests.** Unit: the service's refusal decisions. DB integration (`tests/volume_move.rs`): move; same title refused; deleted target refused; stale version → conflict; an active loan follows the volume (the loan row unchanged, title page of the target lists the volume). E2E (specId `MV`): move a volume from title A to title B through the modal; both title pages updated; Rule #7 style (loginAs).

## Tasks / Subtasks

- [ ] T1 — model/service `VolumeService::move_to_title`
- [ ] T2 — routes (modal fragment, title search fragment, POST move) in a new module (e.g. `src/routes/volume_move.rs`; `titles.rs` / `catalog.rs` are above Rule #12)
- [ ] T3 — templates + i18n (4 locales)
- [ ] T4 — 13.1 string update (AC5)
- [ ] T5 — tests; gates; full suites once before the single push
