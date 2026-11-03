# Story 13.8: Move a volume to another title

Status: done

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

- [x] T1 — model/service `VolumeService::move_to_title`
- [x] T2 — routes (modal fragment, title search fragment, POST move) in a new module (e.g. `src/routes/volume_move.rs`; `titles.rs` / `catalog.rs` are above Rule #12)
- [x] T3 — templates + i18n (4 locales)
- [x] T4 — 13.1 string update (AC5)
- [x] T5 — tests; gates; full suites once before the single push

## Completion Notes

**What was done**

- **Model** `src/models/volume_move.rs` (new, 150 lines): the picker query (`search_candidates` — live titles by title text or ISBN, separators stripped, current title excluded, exact-ISBN match first, `LIMIT 10`; `find_candidate`), and the move SQL (`lock_volume_in` `FOR UPDATE`, `title_is_live_in` `LOCK IN SHARE MODE`, `set_title_in` — `UPDATE … SET title_id, version = version + 1 WHERE id = ? AND version = ? AND deleted_at IS NULL`). Dynamic `sqlx::query` only, so no `.sqlx/` change.
- **Service** `src/services/volume_move.rs` (new, 197 lines): `VolumeService::move_to_title` (an `impl VolumeService` block in its own module). One transaction: lock the volume, compare its version, share-lock the target title, then the pure decision `move_refusal` (`NoTarget` / `SameTitle` / `TargetGone`), then the optimistic UPDATE. Refusals: no target and same title → `BadRequest`, deleted/unknown target and deleted volume → `NotFound`, stale version → `Conflict`, each with its own `error.volume_move_*` message (Rule #9 copy, 4 locales). On success `tracing::info!(volume_id, label, from_title_id, to_title_id, user_id, "Volume moved to another title")`. No audit table: `admin_audit` is for admin actions only, and the spec says logs are enough. Only `title_id` / `version` / `updated_at` change, so the location, condition, values, entity labels (keyed by volume id), audit flag and loans (`loans.volume_id`) stay with the volume. Series membership and expected-tome settlement belong to titles, so they are unchanged. Per-title volume counts are computed live (`COUNT(*)`), not cached.
- **Routes** `src/routes/volume_move.rs` (new, 468 lines incl. tests), registered in `src/routes/mod.rs`. All are Librarian+ (`require_role_with_return(…, "/volume/{id}")`):
  - `GET /volume/{id}/move-modal`: HTMX only (405 otherwise, like the delete modal). UX-DR8 macro, variant `warning`, Pattern A.
  - `GET /volume/{id}/move/candidates?q=`
  - `GET /volume/{id}/move/selection?title_id=`
  - `POST /volume/{id}/move`
- **Modal design.** The picker (intro, search field, candidate list) is in `body_html`, which the macro renders *outside* the confirm form. Enter in the search field therefore never submits the move, and a scanner burst's trailing Enter does nothing. The search field carries `data-modal-default-focus`, as the trash type-to-confirm input does. Picking a candidate (`hx-get` → `#volume-move-selection`, inside the form via `inner_form_html`) brings the hidden `title_id` and the confirmation sentence naming the volume, its current title and the target (title — primary author). Every fragment is an auto-escaped Askama render. A pick the move would refuse (same title, deleted title) shows the refusal and no `title_id`.
  - `hx-trigger="input changed delay:300ms, search"` is used, with no JS filter and no new JS.
  - Confirming without a pick → the server refuses with "no title was chosen", shown in the modal error region.
- **Success response.** A success FeedbackEntry for `#feedback-list` (`afterbegin`), the title row replaced out of band (`#volume-title-row`) and `HX-Trigger-After-Swap: modal-close` (story 11-16 shape, so the OOB lookup runs before modal.js empties the slot). Errors go through `AppError` + `ModalConfirmRetargetGuard` into `data-modal-error`.
- **Volume page.** `templates/pages/volume_detail.html` gets a `#feedback-list` region, the title row wrapped in `#volume-title-row` (shared fragment `fragments/volume_title_row.html`, bound with `{% let %}` + include) and the "Move to another title" button (Librarian+, `data-modal-trigger`, `hx-target="#modal-slot" hx-swap="innerHTML"`). `VolumeDetailTemplate` gains `move_label` (3 lines added to `catalog.rs`).
- **AC5.** `feedback.volume_on_other_title_hint` reworded in en/fr/de/it ("…or open the volume: from its page you can move it to the right title"). The comment in `catalog_existing_label.rs` was updated to match.
- **i18n.** `volume.move_*` (10 keys) and `error.volume_move_*` (5 keys) were added in all four locales.

**Tests** (all green)

- **Unit, `--lib`** (57 passed with the `templates_audit` / `volume_detail` / `catalog_existing_label` filters):
  - `services::volume_move`: the refusal decisions, and localized messages of the right `AppError` kind in 4 locales.
  - `routes::volume_move`: `title_id` parsing, the candidate name, escaping of candidates / selection / picker, and the modal shape (no `hx-confirm`, picker outside the form, selection inside it).
  - `volume_detail_tests::volume_detail_move_button_is_librarian_only`.
- **DB integration, `tests/volume_move.rs`** (11 passed):
  - A move changes only the title and version; live counts follow.
  - Same title refused; no target refused; deleted and unknown target refused; stale version → conflict; deleted volume refused.
  - An active loan follows the volume: the loan row is unchanged, the target title page lists the volume and the source page does not.
  - Candidates: live, other titles, by text or by ISBN with separators, limit 10.
  - Modal / picker / selection render escaped; the modal answers 405 to a direct navigation.
  - POST: success headers and body; a refusal under `X-Modal-Confirm` gets no retarget; a replayed form → 409.
  - An anonymous visitor cannot move, and gets no button.
- **Other test suites.** `locale_parity`, `locale_html_safety`, `scan_existing_label`, `contextual_help_tooltips` and `home_scan_redirect` pass.
- **E2E** `tests/e2e/specs/journeys/volume-move.spec.ts` (specId `MV`, V-codes V7600–V7699, `loginAs`):
  - The full journey: scan two titles and a volume, open the volume page, open the modal, run axe-core on the open dialog, type the target ISBN as a scanner burst (`simulateScan` — the scanner guard lets it reach the search field), pick, confirm. Both title pages follow.
  - Confirm without a pick → the error stays in the modal; Cancel leaves the volume in place.
  - Run together with `scan-duplicates-existing-label`, `title-detail-volumes` and `scanner-guard`: 17/17 passed (`CI=true --workers=2`).
- **Gates.** `npx tsc --noEmit` is clean, `cargo clippy --all-targets -- -D warnings` is clean, and the flake gate finds no `waitForTimeout`.

**File sizes.** `src/routes/catalog.rs` is 3215 lines (it was already over the Rule #12 limit; this story adds 3 lines and puts everything else in new modules). No new file is above 2000 lines.

**Deviations / notes**

- The confirm button is not disabled before a target is picked. Disabling it would have needed new JS. Instead, the search field lives outside the form (so Enter cannot submit), and a confirm without a pick gets an explicit localized refusal in the modal.
- The "Title:" / "Label:" labels of the volume page are still hard-coded English (pre-existing). The shared title-row fragment keeps the same text.
- The second point of #580 ("a scan of a shared ISBN should offer a choice") is out of scope here.

### Review Findings

Code review, a single round on `1359952`: no Medium+ findings, 3 Low findings plus 1 minor, all applied in the `fix(13-8)` commit.

- [x] **[Low] L1 — a title delete could race a move.** The title delete counted volumes, then soft-deleted, with no lock, so a volume moved to that title in between could end up under a deleted title.
  - Fix: the delete is now one transaction, `TitleService::delete_without_volumes` (`src/services/title.rs`). It locks the title `FOR UPDATE`, then counts live volumes, then soft-deletes via the new `SoftDeleteService::soft_delete_in`.
  - Both title-delete routes use it: `DELETE /title/{id}` (`title_lifecycle.rs`) and the legacy `DELETE /catalog/title/{id}` (`catalog.rs`).
  - The move share-locks its target, so the two now serialise. The comment of `move_to_title` was corrected to say this, and its limits.
  - DB tests: `a_title_delete_waits_for_an_in_flight_move`, `a_move_waits_for_an_in_flight_title_delete` and `the_title_delete_refuses_a_title_with_volumes`.
- [x] **[Low] L2 — E2E V-codes could collide.** They were a random pick among 50 inside home.spec's range (≥ V1000). They now come from a clock counter in V0480–V0579, which no other spec uses.
- [x] **[Low] L3 — unbounded query and plain 422.**
  - The picker query is cut to 200 characters (`bounded_query`).
  - `version` is read leniently. A missing or malformed value gives a localized FeedbackEntry (`error.volume_move_bad_form`, 4 locales) instead of axum's plain 422. Tested by unit tests and `a_malformed_version_gets_a_localized_feedback_entry`.
- [x] **[Minor] Hard-coded "Title:" label.** The title row's "Title:" is now the key `volume.title_field` in 4 locales.

