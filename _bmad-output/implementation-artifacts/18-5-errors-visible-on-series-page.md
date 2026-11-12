# Story 18.5: Errors visible on the series page

Status: in-progress

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

## Acceptance Criteria

1. **AC1.** On the series page, opening the delete modal of a series deleted meanwhile (another tab) shows the not-found message in `#feedback-list`; focus stays where it was.
2. **AC2.** Every page on the base layout has exactly one `#feedback-list` (D1); the seven pages that had one keep theirs, unchanged in place.
3. **AC3.** The audit of D2 passes, and fails on a page with two regions or none (helper-level unit cases).
4. **AC4.** E2E: the #526 scenario (series deleted through a second page, then "Delete" clicked on the first) asserts the message next to nothing else and that focus did not move; axe passes on the series page.
5. **AC5.** The other pages listed in Context are covered by D1; recorded in the Dev Agent Record.

## Tasks / Subtasks

- [ ] T1 — base layout block + overrides on the seven pages (AC2)
- [ ] T2 — audit (AC3)
- [ ] T3 — E2E (AC1, AC4)
- [ ] T4 — gates

## Dev Notes

- Delete-modal route: `GET /series/{id}/delete-modal` → `AppError::NotFound` when the series is gone; the button is `hx-get` with `hx-target="#modal-slot"`.
- Rule #21: no focus redirection; Rule #23: the spec uses its own series.
- Gates as in 18-4 (E2E on 18080, DB on 3307; spec file name must sort after `empty-states` if it creates series — #528: name it `series-*.spec.ts`).

### References

- Issue #526; `src/error/mod.rs` (`IntoResponse`); `templates/layouts/base.html`; `src/templates_audit.rs::pages_using_feedback_list_target_declare_slot`

## Dev Agent Record

### Agent Model Used

### Completion Notes List

### File List
