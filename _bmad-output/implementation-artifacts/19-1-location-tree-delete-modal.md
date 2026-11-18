# Story 19.1: Location tree delete through the modal (closes #698)

Status: ready-for-dev

## Story

As a librarian on `/locations`,
I want deleting a location to ask for confirmation in my language, in the application's own dialog,
so that a scanner's Enter cannot delete a location and the labels I hear are mine.

## Context

The tree rows are built in Rust (`src/routes/locations.rs::render_node_at_depth`, l. 196–340). Since #695 (v1.28.1) their controls are visible, so they are used. The delete control (l. 271) carries:

- `hx-delete="/locations/{id}" hx-confirm="Delete {name} ({label})?"` — the browser's native `confirm()`: English, and not watched by `static/js/scanner-guard.js` (it watches `dialog[open]` / `[aria-modal="true"]`), so a scanner's Enter confirms the deletion. It is the only grandfathered entry of `templates_audit::ALLOWED_HX_CONFIRM_RUST_SITES` (#138).
- Hard-coded English `aria-label`s on the three controls: `Add child under {name}`, `Edit {name}`, `Delete {name}`.

Found while preparing the story:

- The controls are rendered for `role >= Librarian` (`can_edit`, l. 421), but `delete_location` requires **Admin** (l. 619): a librarian sees a delete control that always answers a refusal.
- `LocationService::delete_location` (`src/services/locations.rs:227`) builds its refusals (`location.has_children`, `location.has_volumes`) with `t!()` **without locale**: they come out in the instance language, not the request's.
- The success answer is a hand-written green `<div>` swapped over the tree item (`outerHTML`), not a feedback entry.
- Other Rust-built markup carries hard-coded English accessible names: `aria-label="Pagination"` (`routes/home_search_fragment.rs`), `aria-label="Dismiss"` (`routes/catalog.rs:91`, `middleware/pending_updates.rs:206`, `utils.rs:315`), `aria-label="Contributors"` (`routes/catalog_contributors.rs:409`). Same class of defect as #698, outside the template audit.

## Decisions

- **D1 — UX-DR8 modal, Pattern A**, like the borrower / series / expected-tome deletes. The tree's delete control becomes `hx-get="/locations/{id}/delete-modal" hx-target="#modal-slot" hx-swap="innerHTML" data-modal-trigger aria-haspopup="dialog" aria-expanded="false"`. New handler `locations::delete_modal` (HTMX only, 405 + `Allow: GET` otherwise; Admin; a gone location → `build_already_deleted_response(loc, "#feedback-list")`), new fragment `templates/fragments/location_delete_modal.html` calling `components/modal.html::modal` with the `delete` variant, method `DELETE`, `action_url = /locations/{id}`, target `#feedback-list` / `afterbegin`. Title: "Delete {name} ({label})?" (raw name through `t!()`, Askama escapes); body: it goes to the Trash for 30 days.
- **D2 — success answer.** `delete_location` answers a standard feedback entry (`utils::feedback_html_pub`, success) into `#feedback-list`, plus an OOB removal of the tree item, and closes the modal with `HtmxResponse::into_response_with_hx_trigger_after_swap("modal-close")`. The tree item gets a stable id `tree-node-{id}` (the OOB element is `<div id="tree-node-{id}" hx-swap-oob="delete"></div>`). Focus: the modal restores it to its trigger if still in the document, else modal.js's existing fallback (Rule #21) — the case where the trigger left with the row is checked in E2E.
- **D3 — refusals in the modal.** `has_children` / `has_volumes` stay `AppError::BadRequest`, now built in the request locale (`LocationService::delete_location(pool, id, loc)`); from the modal they land in its `data-modal-error` region (ModalConfirmRetargetGuard), the modal stays open. Not pre-checked when opening the modal (one source of truth: the delete itself).
- **D4 — delete shown to Admin only.** The delete control is rendered only when the session is Admin (new `can_delete` alongside `can_edit`); add and edit stay Librarian+. Rights unchanged — only the dead control goes.
- **D5 — labels localized.** `location.tree.add_child_named`, `location.tree.edit_named`, `location.tree.delete_named` (`%{name}`), plus the modal keys `location.delete_modal_title` / `_body` / `_confirm`, in the four locale files.
- **D6 — the audit covers Rust-built markup (retro v1.28.1 A2).** In `src/templates_audit.rs`:
  - `ALLOWED_HX_CONFIRM_RUST_SITES` becomes empty (steady state, like the template one);
  - new `rust_markup_has_no_inline_style_or_handlers`: a `style="` or an `on<event>="` attribute in a string literal of production Rust code fails;
  - new `rust_markup_accessible_names_are_localized`: an `aria-label`, `title`, `alt` or `placeholder` attribute whose literal value starts with a letter (i.e. not interpolated) in production Rust code fails; allowlist empty.
  - "Production Rust code" = every `src/**/*.rs` except `templates_audit.rs`, `*_tests.rs`, and the text after a file's first `#[cfg(test)]` (the project convention puts the test module last; a unit test pins that a `#[cfg(test)]` before production code would be reported, not silently skipped — or the story documents the chosen cut).
  - The existing hits are fixed in this story (D7), except the one counted site above.
  - The test-module cut is a `#[cfg(test)]` followed by a `mod` declaration (`pub(crate) mod` included); a `#[cfg(test)]` on a single item does not end production code (`modal_confirm_retarget_guard.rs` has one).
- **D7 — the other hard-coded names.** `Pagination` (`pagination.aria_label`), `Contributors` (`title_detail.contributors`) and `Dismiss` in the catalog scan error and the pending-updates failure entries (new `feedback.dismiss_aria`) go through `t!()` in the request locale; the scan error's Retry / Edit labels too (they had no locale either). **Changed in dev:** `utils::feedback_html_action`'s dismiss has no request locale (127 `feedback_html` callers) — it stays, counted, in `ALLOWED_LITERAL_NAMES_RUST_SITES` and is localized when the feedback renderers merge (#701).

## Acceptance Criteria

1. **AC1.** On `/locations`, as Admin, the tree's delete control opens the UX-DR8 modal in `#modal-slot`; Confirm deletes the location (Trash), the row leaves the tree, a success entry appears in `#feedback-list`, the modal closes. No `hx-confirm=` remains anywhere under `src/` or `templates/`.
2. **AC2.** With the modal open, a scanner burst ending in Enter does not confirm the deletion (scanner-guard) — E2E with `simulateScan`.
3. **AC3.** Deleting a location that has children or volumes shows the refusal inside the open modal, in the request language; nothing is deleted.
4. **AC4.** A Librarian sees the add and edit controls but no delete control; `DELETE /locations/{id}` and `GET /locations/{id}/delete-modal` as Librarian are refused as before.
5. **AC5.** The three control labels, the modal copy and the refusals are in the request language (FR and EN checked in E2E with the i18n-aware matchers; four locale files, `tests/locale_parity.rs` green).
6. **AC6.** `templates_audit` fails on a Rust-built `hx-confirm=`, inline `style=` / `on…=` and a literal (non-localized) `aria-label` / `title` / `alt` / `placeholder` in production code; all allowlists empty; the D7 sites fixed.
7. **AC7.** Unit tests: the renderer (Admin vs Librarian controls, localized labels, `tree-node-{id}` id, no `hx-confirm`), the modal handler (405 non-HTMX, gone location), the audit helpers (the production-code cut, one positive and one negative case per pattern).

## Tasks / Subtasks

- [x] T1 — `delete_modal` handler + route + `location_delete_modal.html` (AC1, D1)
- [x] T2 — `delete_location`: feedback entry + OOB removal + `modal-close`; `tree-node-{id}` id (AC1, D2)
- [x] T3 — `LocationService::delete_location` takes the locale (AC3, D3)
- [x] T4 — `can_delete` (Admin) in the renderer and its callers (AC4, D4)
- [x] T5 — locale keys ×4 (AC5, D5, D7)
- [x] T6 — audit: empty Rust allowlist, two new tests, production-code cut (AC6, D6)
- [x] T7 — D7 sites through `t!()` (AC6)
- [x] T8 — unit tests (AC7)
- [x] T9 — E2E in `locations.spec.ts`: modal delete (replaces the `confirm()` case and the `hx-delete` click of the #457 case), scanner burst, refusal in the modal, Librarian sees no delete, FR labels (AC1–AC5)
- [x] T10 — CLAUDE.md: the modal scanner-guard paragraph names the Rust-markup audit; the `hx-confirm` "allowlist frozen empty" sentence covers Rust too
- [ ] T11 — gates

## Dev Notes

- Pattern to copy: `src/routes/borrowers.rs::delete_modal` (l. 333) + `templates/fragments/borrower_delete_modal.html`; target / swap parameters as in `expected_tome_delete_modal.html`. Success with OOB + modal close: `HtmxResponse { main, oob }` and `into_response_with_hx_trigger_after_swap("modal-close")` (see the proposals reject modal).
- `#feedback-list` exists on every base page since story 18-5 (`templates_audit::every_base_page_has_one_feedback_region`).
- Existing E2E to update: `locations.spec.ts:159` (accepts the browser `confirm()`), `:328` (#457 case clicks `button[hx-delete^="/locations/"]`), `:202` (#695 visibility — Librarian now sees two controls, Admin three). Use the "really seen" check only if 19-2's helper has landed; otherwise keep the computed-opacity assertion already there.
- Rule #23: assert only on locations the spec creates. Rule #21: focus after the modal closes.
- A3 practice (retro v1.28.1): D2 (where the success message lands, the row disappearing), D3 (refusal inside the modal) and D4 (delete hidden from Librarians) change what users see — **confirmed by the project lead on 2026-11-18** (D1 modal, D2 success entry + row removed, D3 refusal inside the modal, D4 delete hidden from Librarians).
- `mem-guard` for every heavy command; one sub-agent at a time (station shared with kesh).

### References

- Issue #698; epic 19, story 19.1 (`_bmad-output/planning-artifacts/epics.md`); retrospective `patch-v1-28-1-retro-2026-11-18.md` A2.
- `src/templates_audit.rs` l. 30–49 (allowlists), l. 382 (`hx_confirm_in_rust_strings_matches_allowlist`).
- CLAUDE.md — "Modal scanner-guard invariant", "Modal lifecycle hardening", "CSP & hardening headers".

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (`claude-opus-5-5`)

### Debug Log References

### Completion Notes List

### File List

- `src/routes/locations.rs`, `src/routes/mod.rs`, `src/services/locations.rs`, `templates/fragments/location_delete_modal.html` (new)
- `src/templates_audit.rs`
- `src/routes/catalog.rs`, `src/routes/catalog_contributors.rs`, `src/routes/home_search_fragment.rs`, `src/middleware/pending_updates.rs`
- `locales/{en,fr,de,it}.yml`
- `tests/e2e/specs/journeys/locations.spec.ts`
- `CLAUDE.md`, `_bmad-output/implementation-artifacts/sprint-status.yaml`
