# Story 18.5: Errors visible on the series page

Status: done

GitHub issue: #526 (closes). Covers: fix. Epic 18; ships in v1.28.0. Sprint-status key: `18-5-errors-visible-on-series-page`.

## Story

As a librarian on a series page,
I want an error to be shown when an action fails,
so that a click that did nothing is never silent.

## Context

`AppError::IntoResponse` answers every error with `HX-Retarget: #feedback-list` + `HX-Reswap: beforeend` (polish-1), and the CSRF rejection lands there too (`csrf.js`). Only seven page templates declare a `#feedback-list`: `admin`, `catalog`, `home`, `proposal_detail`, `proposals`, `title_detail`, `volume_detail`. Pages that issue HTMX requests without one — where any failure is silent — are, on 2026-11-12: **`series_detail`** (the #526 report: delete modal of a tome / of the series deleted in another tab, CSRF rejection), `borrower_detail`, `contributor_detail`, `loans`, `wishlist`, `wishlist_detail`, `wishlist_new`. Plus any page whose included fragments issue HTMX requests.

## Decisions

- **D1 — one default region in the layout, not seven copies.** `layouts/base.html` renders `{% block feedback_region %}<div id="feedback-list" aria-live="polite" aria-relevant="additions" …>{% endblock %}` at the top of `<main>`, inside a centred container, collapsing to no height while empty (`empty:`). A page that already places its own `#feedback-list` (the seven above; catalog's is part of its layout) overrides the block with nothing. Every page on the base layout therefore has exactly one region — the #526 class of bug cannot come back with a new page.
- **D2 — audit.** `templates_audit` replaces `pages_using_feedback_list_target_declare_slot` (#325) with a stricter rule: a page extending `layouts/base.html` declares `id="feedback-list"` **iff** it overrides `feedback_region` (no zero, no duplicate id); a page on another layout that targets `#feedback-list` declares it (the #325 rule, kept).
- **D3 — no change to the series page's own `#series-feedback`** (11-11 wish-list refusals render there by design).
- **D4 — focus.** A `beforeend` swap into the region never moves focus (Rule #21); the region is `aria-live="polite"`.

- **D5 — show the server's message, not the generic line (found in dev).** With a region on the page, the first E2E run showed "Erreur serveur (404) — veuillez réessayer." instead of the server's not-found copy: HTMX 2 drops 4xx/5xx bodies (`responseHandling`), so the `HX-Retarget` that `AppError::IntoResponse` sets since polish-1 never applied outside modals, and `mybibli.js`'s `htmx:responseError` handler stood in with `errors.server_error_retry` — HTTP code and "try again" even for a 404 (Rule #9). Fix: that handler inserts the server's entry when the answer is a positive shape (`HX-Retarget: #feedback-list` and a body of exactly one `.feedback-entry`), the generic line otherwise. HTMX still treats the response as an error. (A first attempt opted the answer into the HTMX swap from `csrf.js`; review showed it turned errors into successes — see Review Findings.)
- **D6 — AA contrast of red delete buttons on hover (found in dev).** axe flagged the series page Delete button while hovered (`text-red-600` on `hover:bg-red-50`, 4.36 < 4.5). Same pattern on 10 buttons → `text-red-700` (dark variant unchanged; `dark:text-red-400` added to the five admin rows, which had none).

## Acceptance Criteria

1. **AC1.** On the series page, opening the delete modal of a series deleted meanwhile (another tab) shows the not-found message in `#feedback-list`; focus stays where it was.
2. **AC2.** Every page on the base layout has exactly one `#feedback-list` (D1); the seven pages that had one keep theirs, unchanged in place.
3. **AC3.** The audit of D2 passes, and fails on a page with two regions or none (helper-level unit cases).
4. **AC4.** E2E: the #526 scenario (series deleted through a second page, then "Delete" clicked on the first) asserts the message next to nothing else and that focus did not move; axe passes on the series page.
5. **AC5.** The other pages listed in Context are covered by D1; recorded in the Dev Agent Record.

## Tasks / Subtasks

- [x] T1 — base layout block + overrides on the seven pages (AC2)
- [x] T2 — audit (AC3)
- [x] T3 — E2E (AC1, AC4)
- [x] T4 — gates

## Dev Notes

- Delete-modal route: `GET /series/{id}/delete-modal` → `AppError::NotFound` when the series is gone; the button is `hx-get` with `hx-target="#modal-slot"`.
- Rule #21: no focus redirection; Rule #23: the spec uses its own series.
- Gates as in 18-4 (E2E on 18080, DB on 3307; spec file name must sort after `empty-states` if it creates series — #528: name it `series-*.spec.ts`).

### References

- Issue #526; `src/error/mod.rs` (`IntoResponse`); `templates/layouts/base.html`; `src/templates_audit.rs::pages_using_feedback_list_target_declare_slot`

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (`claude-opus-5-5`)

### Completion Notes List

- **T1** — `layouts/base.html`: `{% block feedback_region %}` at the top of `<main>` with the default region (`max-w-4xl` container, `aria-live="polite"`, `empty:pt-0`). The seven pages with their own region (admin, catalog, home, proposal_detail, proposals, title_detail, volume_detail) override it with nothing, their region unchanged in place. The 21 other base pages inherit it — among them the ones that issued HTMX requests with nowhere for an error to land: **series_detail** (#526), borrower_detail, contributor_detail, loans, wishlist, wishlist_detail, wishlist_new (AC5). The two `bare.html` pages (setup, csrf_drifted) target no `#feedback-list`.
- **T2** — `templates_audit::every_base_page_has_one_feedback_region` replaces `pages_using_feedback_list_target_declare_slot` (#325): base page → declares iff overrides; other layout → declares if it targets. HTML and Askama comments stripped. `feedback_region_issue_reads_the_four_shapes` unit-tests the shapes.
- **D5** — `static/js/mybibli.js`: `serverFeedbackEntry(xhr)` (positive shape, parsed through a `<template>`) + `injectServerFeedback` (newest first, `htmx.process`); `restoreScanField` and every other `responseError` listener run as before.
- **D6** — 10 templates, one class each.
- **T3 — E2E.** `series-feedback-region.spec.ts` (new; named `series-*` to sort after the specs that assume no series, #528): series deleted from a second page, Delete on the stale one → one entry with the server's not-found copy (4 locales), no generic line, no dialog, focus on the button, axe clean. `admin-reference-data.spec.ts`: a refused Add (genre that exists) shows the server's "already exists" and the slot stays open with the name. `title-volume-add.spec.ts`: an AppError-shaped scan failure shows its own message and still restores the code into the field.
- **T4 — Gates.** See the end of the file.

### File List

- `templates/layouts/base.html`; `templates/pages/{admin,catalog,home,proposal_detail,proposals,title_detail,volume_detail}.html` (block override)
- `static/js/mybibli.js`
- `src/templates_audit.rs`
- `templates/pages/{series_detail,title_detail,borrower_detail,contributor_detail,audit_list}.html`, `templates/fragments/admin_ref_{genre,role,volume_state,node_type,label}_row.html` (D6)
- `tests/e2e/specs/journeys/series-feedback-region.spec.ts` (new), `admin-reference-data.spec.ts`, `title-volume-add.spec.ts`
- `CLAUDE.md` (Modal lifecycle paragraph: how an AppError reaches the page)
- `_bmad-output/implementation-artifacts/18-5-errors-visible-on-series-page.md`, `sprint-status.yaml`

### Review Findings

Code review 2026-11-12 (Blind Hunter; Edge Case Hunter + Acceptance Auditor). No High.

- [x] [Review][Patch] **The `csrf.js` swap opt-in turned AppErrors into successes** (Medium ×2 + Low ×3, both layers) — `isError = false` skipped every `htmx:responseError` listener (scan code not restored, 1 s duplicate guard left armed, search state not reset) and made `afterRequest` see `successful` (a refused admin Add closed its slot and cleared the name); `shouldSwap` also let `hx-push-url` push a failing URL, and `beforeend` put the error below older entries. Replaced by D5 (the error handler inserts the server's entry; HTMX keeps it an error). Regression E2E for the refused Add and for an AppError-shaped scan failure.
- [x] [Review][Patch] **`text-red-700` darker in dark mode on the admin rows** (Low) — `dark:text-red-400` added.
- [x] [Review][Patch] **Story record empty, scope additions undocumented** (auditor) — D5, D6 and this record.
- [x] [Review][Dismiss] `hx-select` would filter the inserted entry — no element uses `hx-select`, and D5 no longer swaps through HTMX.
- [x] [Review][Defer] **Region at the top of `<main>`: an error from a control low on a long page may land off-screen** (Medium/Low) — same placement as the seven existing regions; announced by `aria-live`; no focus move (Rule #21). Tracked in a follow-up issue.
- [x] [Review][Defer] Audit gaps (`{%-` whitespace control, single quotes, a region in an included fragment or under `{% if %}`; none exists today) and the pre-existing replace-mode OOB on `#feedback-list` in admin routes dropping `aria-live` — follow-up issue.
- [x] [Review][Dismiss] File-name ordering is not a guarantee under `fullyParallel` — true, and that is #528; the name follows the existing convention.

### Gates (final code)

clippy `--all-targets -D warnings` clean; `sqlx prepare --check` green (no query change); full `cargo test` with DB (light build: `--no-run` on one job, `line-tables-only`, after an oomd kill at link time) — **2728 passed, 0 failed**; E2E on a fresh stack, `CI=true --workers=2` — **478 passed, 2 skipped, 0 failed, 0 flaky**; `tsc --noEmit` green. Review clean (no Medium+ left; two deferred Lows in #689).
