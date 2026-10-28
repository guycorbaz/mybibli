# Story 11.11: Expected tomes — series page and wishlist

Status: done

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR142, FR147, UX-DR34 (UX-DR16 grid, UX-DR8 Modal reused). Epic 11 — eleventh story; consumes 11-10 (`ExpectedTomeService`, auto-settle at scan). Carry-overs from 11-10: the scan-to-settle E2E (D-11-10-7), the localization of `ExpectedTomeError` (D-11-10-4), and a legible rendering of the states of #524 (tome above a lowered total, tome on a position a restored title holds again). Related open issues: #519 (`/series` list counting — out of scope), #524 (rendered here, not prevented).

## Story

As a librarian,
I want to see and manage a series' missing tomes from its page and send them to the wishlist,
so that I know what to buy.

## Existing-code reality check (code map, 2026-10-28, `main` 4564ed4)

| Place | Today | Consequence |
|---|---|---|
| `src/routes/series.rs` (693 lines) | `series_detail_page` builds `positions` with `SeriesService::get_series_positions` (closed: 1..=total with gaps; open: assigned positions only), counts owned/gaps, then `sort_positions` | merge the series' live expected tomes into `positions` **before** the sort; counters unchanged (a tome does not own a position) |
| `src/services/series.rs` (941 lines) | `SeriesPositionInfo` built in `get_series_positions`, `build_position_grid` and one test helper | gains `expected: Option<ExpectedTomeCard>` (`None` in the three constructors) |
| `templates/components/series_gap_grid.html` | gap card = red dashed `series-card-gap series-gap-stripes` div with "Missing #N" | a gap carrying a tome shows the tome title (and ISBN); Librarian/Admin get Edit / Delete / Add-to-wishlist; a bare gap gets a "Name this tome" link for them |
| `services::expected_tomes` | CRUD + `settle_for_title`; `ExpectedTomeError` English `Display` (MCP copy) and `From<… > for AppError` (English) | add `ExpectedTomeError::localized(loc)` for the web; MCP keeps `Display` (D-11-7 / D-11-9-2 precedent) |
| `ExpectedTomeModel::list_by_series` | live tomes of a live series by position; `settled_title_id` already reduced by the settled predicate (title live AND holds the position) | the page uses it as is; a settled tome is hidden (its position is an owned card) |
| `routes::wishlist::wishlist_create` | inserts via `WishlistItemModel::create`, writes a `wishlist_add` audit row inline | extract the audit write into a shared helper (Rule #1) used by both entry points |
| `WishlistItemModel::find_by_isbn` | oldest live item with that exact ISBN | duplicate check for the tome's ISBN-13 (the wishlist form and the scan store ISBN-13) |
| `routes::wishlist::autolink_feedback_html` (CR #242) | scan banner `data-wishlist-autolink` when the scanned ISBN is on the wishlist | the "wishlist item is linked" part of the E2E |
| `routes::catalog_isbn_banners::isbn_banners` | scan feedback: wishlist banner + one entry per settle outcome | the "tome shows as owned" part of the E2E |
| `templates/components/modal.html` (UX-DR8 macro), `fragments/series_delete_modal.html` | `GET …/delete-modal` HTMX-only fragment into `#modal-slot`, Confirm posts with CSRF + version | same shape for deleting a tome |
| `SeriesService::update_series` | accepts a total below a live tome's position (#524 path 1); `detect_restore_conflicts` ignores tomes (#524 path 2) | not changed: the page renders those tomes in a "to review" list with the reason |

## Acceptance Criteria

1. **AC1 — grid (UX-DR34, UX-DR16).** On `/series/{id}` every live, unsettled expected tome whose position is a gap of the grid is rendered in that gap square with its title, its ISBN when set, and an "Expected" marker; the square keeps the missing styling (dashed border + hatch, UX-DR16). For an open series, a tome adds a gap square at its position (ordered with the owned squares). A settled tome is not rendered separately (its position is the owned title's card). Anonymous visitors and every role see the tomes; the owned / total / gaps counters do not change.
2. **AC2 — tomes to review (#524).** A live unsettled tome that cannot sit in a gap is listed below the grid in a "Expected tomes to review" section, with a reason: *above the total* ("Position N is above the series total of T.") for a closed series whose total is now below it, *position held* ("Position N is held by «title».") when a live title of the series holds it. Same actions as in the grid for Librarian/Admin, so the librarian can move or delete it.
3. **AC3 — create (FR147).** A Librarian/Admin sees "Add expected tome" in the series actions and "Name this tome" on a bare gap; both open `GET /series/{id}/expected-tomes/new` (the gap link pre-fills `?position=N`). The form (position, title, optional ISBN; CSRF hidden field first) posts to `POST /series/{id}/expected-tomes`; success redirects (303) to `/series/{id}`; a refusal re-renders the form with the typed values and the localized message (status 422), nothing saved.
4. **AC4 — edit (FR147).** "Edit" opens `GET /series/{id}/expected-tomes/{tome_id}/edit` (prefilled, hidden `version`); `POST /series/{id}/expected-tomes/{tome_id}` saves through `ExpectedTomeService::update`; same success/refusal shape as AC3 (a stale version and a settled tome are refusals with their localized message).
5. **AC5 — delete (FR147, UX-DR8).** "Delete" fetches `GET /series/{id}/expected-tomes/{tome_id}/delete-modal` (HTMX only, 405 otherwise) into `#modal-slot`: the UX-DR8 `delete` variant naming the tome, Cancel first, Confirm posting `POST /series/{id}/expected-tomes/{tome_id}/delete` with CSRF + `version`; success answers `HX-Redirect: /series/{id}`; a refusal surfaces as a localized error (AppError path, lands in the modal error region). No `hx-confirm` anywhere.
6. **AC6 — add to wishlist (FR142, UX-DR34).** Each tome rendered for a Librarian/Admin has "Add to wishlist", an `hx-post` to `POST /series/{id}/expected-tomes/{tome_id}/wishlist` targeting `#series-feedback`. It creates a wishlist item with the tome's title, its ISBN (or none) and a note naming the series and the position ("Series «name», tome N", in the librarian's language), writes the `wishlist_add` audit row, and answers a success feedback entry with a link to the item.
7. **AC7 — no duplicate.** When the tome's ISBN is already on the wishlist (live item), nothing is created and the answer is an info entry saying so, with a link to the existing item. A tome without ISBN is a duplicate when a live item without ISBN carries the same title and the same note.
8. **AC8 — roles & scoping.** Every form, modal and mutation requires Librarian (Anonymous → login redirect with return to the series page; HTMX mutation → the usual 303/HX-Redirect); a tome id that does not belong to the series in the path, or is deleted, is a 404. The page itself stays readable anonymously.
9. **AC9 — localized errors.** `ExpectedTomeError::localized(loc)` renders every variant through `t!()` (`expected_tome.error.*`), in en/fr/de/it, following docs/error-message-style.md: field errors terse (title required / too long, invalid ISBN, position out of range), conflicts as what happened → why → what to do (position held, position taken, duplicate ISBN, settled, changed by someone else, not found), database error without internals. The MCP `Display` stays English.
10. **AC10 — CSP & CSRF.** No inline script/style/handler (`templates_audit` green); every new `<form method="POST">` carries `_csrf_token` as first child; HTMX posts inherit the token via `csrf.js`.
11. **AC11 — tests.** Unit: grid placement (gap filled, open-series insert ordered, above total, held, settled skipped), wishlist note text, every `localized` variant non-empty in the four locales and distinct from the key. Router-level integration (`tests/expected_tomes_web.rs`, `#[sqlx::test]`, `tests/common/web.rs`): page shows a tome anonymously without actions and with actions for a librarian; create / edit / refusal (422 + localized message) / stale version; delete modal 405 without HTMX, delete success; tome of another series → 404; anonymous POST refused; wishlist create (item fields + note + audit), duplicate by ISBN, duplicate without ISBN; lowered total and held position listed under "to review". E2E `tests/e2e/specs/journeys/series-expected-tomes.spec.ts` (spec id `XT`; file name per D-11-11-6): (a) Librarian creates a closed series, adds an expected tome with an ISBN through the form, sees it in its gap, sends it to the wishlist (then the second click says it is already there), scans the ISBN on `/catalog` — the feedback shows the settle and the wishlist banner — and the series page shows the title owned at that position; (b) edit then delete a tome through the modal; (c) lowering the total moves a tome to "to review"; (d) a blank browser (anonymous) sees the tome title without actions.

## Tasks / Subtasks

- [x] T1 — `ExpectedTomeError::localized` + `expected_tome.error.*` keys ×4 (AC9).
- [x] T2 — `SeriesPositionInfo.expected` + pure placement `services::expected_tomes::place_on_grid` returning the stranded tomes (AC1, AC2).
- [x] T3 — `src/routes/series_expected_tomes.rs`: form pages, create, update, delete modal + delete, wishlist; routes in `routes/mod.rs` (AC3–AC8).
- [x] T4 — Templates: `pages/expected_tome_form.html`, `fragments/expected_tome_delete_modal.html`, grid card + "to review" section in `series_detail.html` / `series_gap_grid.html`; labels ×4 (AC1–AC6, AC10).
- [x] T5 — Wishlist: shared audit helper, duplicate lookup without ISBN (`WishlistItemModel::find_without_isbn`) (AC6, AC7).
- [x] T6 — Tests (AC11): unit, `tests/expected_tomes_web.rs`, E2E spec.
- [x] T7 — Gates: clippy `--all-targets -D warnings`, full `cargo test` (test DB 3307), `cargo sqlx prepare --check --workspace -- --all-targets`, locale parity, flake grep, `tsc --noEmit`, full Playwright `CI=true --workers=2` on a reset stack (port 18080).

## Dev Notes

- **Layering:** routes thin (parse, role, call service, render); placement rule pure in `services::expected_tomes` (unit-tested); SQL only in models.
- **Placement happens before `sort_positions`** so the sort sees tome squares as gaps (bucket "gap" in the Dewey / Title sorts — they never lead).
- **Closed total:** use `expected_tomes::closed_total`-equivalent (closed **and** total > 0); a closed series without total behaves like an open one for placement.
- **Wishlist ISBN:** the tome's ISBN is already a normalised ISBN-13, exactly what `find_by_isbn` and the scan auto-link compare.
- **Error rendering:** the form handlers catch `ExpectedTomeError` and re-render with `localized`; `Database` is logged and shown as the generic message. Delete / wishlist map to `AppError` with the localized message (NotFound / Conflict / BadRequest by the same classification as `From<ExpectedTomeError> for AppError`).
- **No `hx-confirm`**, modal fragment rendered through the UX-DR8 macro (`delete` variant, `hx_target` `#series-feedback`).
- **Rule #12:** new route file; `series.rs` only gains the merge call and the labels.
- Format only touched leaf files (`rustfmt --edition 2024 <file>`); never `cargo fmt`, never rustfmt `src/lib.rs` / `src/main.rs`. Heavy commands under `mem-guard`, cargo `-j 2`.
- **E2E data isolation:** spec id `XT`; series names and ISBN sequences carry a run-unique suffix so a rerun on a non-reset stack does not hit a previous run's wishlist item.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.11] — FR142, FR147, UX-DR34, UX-DR16, UX-DR8.
- [Source: _bmad-output/implementation-artifacts/11-10-expected-tomes-data-auto-settle.md] — service API, D-11-10-1..12.
- [Source: src/routes/series.rs#delete_modal] — UX-DR8 modal pattern.
- [Source: src/routes/wishlist.rs#wishlist_create, #autolink_feedback_html] — wishlist write and scan link.
- [Source: docs/error-message-style.md] — error copy.
- GitHub #524 — states rendered by AC2.

### Review Findings

Round 1 (2026-10-28, Blind Hunter + Edge Case Hunter + Acceptance Auditor on `git diff main...HEAD`) — decisions in D-11-11-7:

- [x] [Review][Patch] "Add to wishlist" errors (deleted tome / series, server error) were lost: the `AppError` retarget aims at `#feedback-list`, absent from the series page [Medium] — the handler renders every refusal into `#series-feedback` (200).
- [x] [Review][Patch] After a version conflict the edit form kept the stale version, so every resubmit conflicted again [Medium] — the form is re-rendered with the tome as it now stands and its current version.
- [x] [Review][Patch] The anonymous-mutation test was stopped by CSRF, not by the role check [Medium] — `Browser::anonymous` (real anonymous session + token); create / delete / wishlist all asserted 303 → login.
- [x] [Review][Patch] French copy said "liste d'envies" while the rest of the French UI says "wish list" [Medium] — aligned.
- [x] [Review][Patch] Duplicate label keys (`edit`, `delete`, `add_to_wishlist`, `cancel`) [Low] — existing `series.*`, `wishlist.add_to_wishlist`, `common.cancel` reused.
- [x] [Review][Patch] `invalid_isbn` field hint ended with an instruction [Low] — terse ("ISBN must be a valid ISBN-13 or ISBN-10.").
- [x] [Review][Patch] Refusal page carried the POST-only URI into the layout's `next` links [Low] — the GET form address is used.
- [x] [Review][Patch] A tome / series that vanished between the checks re-rendered the form with 422 [Low] — 404.
- [x] [Review][Patch] Wishlist accepted a settled tome; the edit form of a settled tome only refused on save [Low] — "already in your catalog" info entry; the edit form shows the settled message up front.
- [x] [Review][Patch] Silent drop in the unreachable `place_on_grid` branch [Low] — logged.
- [x] [Review][Patch] Double click on "Add to wishlist" [Low] — `hx-disabled-elt="this"`; test message no longer claims a concurrency check.
- [x] [Review][Patch] No test for escaping in the wishlist feedback, nor for delete on a deleted series [Low] — added.
- [x] [Review][Dismiss] "A settled tome can become invisible and block its position" — settled requires the title to hold the position, so the position is never a gap while the tome is hidden; once the title leaves, the tome reads unsettled and is shown again (D-11-10-10). "Delete works on a trashed series" — `find` joins the live series, already a 404 (now tested).
- [x] [Review][Accept] No-ISBN dedup keyed on the localized note (locale / rename / hand edit can duplicate) and an ISBN added after a no-ISBN wish (D-11-11-4); check-then-insert race between two librarians; non-integer / overflowing position reads as out of range; tome squares do not sort by name under `?sort=title` (gaps bucket last by design, #235); inherited `CAST(publication_year AS UNSIGNED)` in the wishlist model; `expected_tome.error.series_not_found` / `.database` not reached from the web (generic messages there); E2E run stamp wraps every ~5.5 h.

Round 2 (2026-10-28, confirmation pass on the round-1 fixes): **no Medium+ finding**. Lows (D-11-11-8):

- [x] [Review][Patch] The version-conflict message said "reload the page" while the form already shows the latest version [Low] — wording changed in the four locales.
- [x] [Review][Patch] Repeated Edit / Delete / Add-to-wishlist controls had no accessible context [Low] — `aria-label` names the tome.
- [x] [Review][Defer] A failed delete-modal GET (tome deleted meanwhile) is invisible: the series page has no `#feedback-list` for the `AppError` retarget (same for the series delete modal and CSRF rejections) [Low] — #526.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

- Series page: unsettled tomes fill their gap square (title, ISBN, "Expected" marker); open series get a square at the tome's position; settled tomes stay hidden behind the owned card; tomes above a lowered total or on a held position are listed under "Expected tomes to review" with the reason (#524, D-11-11-2). Placement is the pure `expected_tomes::place_on_grid`, run before the sort.
- Routes in `src/routes/series_expected_tomes.rs`: full-page add / edit forms (refusal → 422 + localized message, typed values kept), UX-DR8 delete modal + `POST …/delete` (HX-Redirect), `POST …/wishlist` (D-11-11-3).
- Wishlist: title + ISBN + note "Series «name», tome N"; duplicate by ISBN, or for a tome without ISBN by title + note (D-11-11-4); shared `audit_wishlist_add` helper; `utils::feedback_html_with_link` for the link to the item.
- `ExpectedTomeError::localized` / `into_app_error` with `expected_tome.error.*` in the four locales; MCP keeps the English `Display` (D-11-11-5).
- E2E spec renamed `series-expected-tomes.spec.ts` (D-11-11-6): a series created early in the alphabetical schedule broke empty-states / dewey-code / classification-fieldset.
- Gates after review: clippy `--all-targets -D warnings` clean; `cargo test` 1854 passed / 0 failed; `cargo sqlx prepare --check` green; flake grep clean; `tsc --noEmit` clean; Playwright `CI=true --workers=2` on a reset stack (18080): 367 passed, 2 skipped, 0 failed.
- Gates before review: clippy `--all-targets -D warnings` clean; `cargo test` 1852 passed / 0 failed (test DB 3307); `cargo sqlx prepare --check` green; flake grep clean; `tsc --noEmit` clean; Playwright `CI=true --workers=2` on a reset stack (18080): 367 passed, 2 skipped, 0 failed.

### File List

- src/routes/series_expected_tomes.rs (new)
- templates/pages/expected_tome_form.html (new), templates/fragments/expected_tome_delete_modal.html (new), templates/components/expected_tome_actions.html (new)
- templates/components/series_gap_grid.html, templates/pages/series_detail.html
- src/routes/series.rs, src/routes/mod.rs, src/routes/wishlist.rs
- src/services/expected_tomes.rs, src/services/series.rs
- src/models/wishlist.rs, src/utils.rs
- locales/en.yml, locales/fr.yml, locales/de.yml, locales/it.yml
- tests/expected_tomes_web.rs (new), tests/common/web.rs
- tests/e2e/specs/journeys/series-expected-tomes.spec.ts (new)
- _bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md, sprint-status.yaml

## Change Log

- 2026-10-28 — story created (ready-for-dev).
- 2026-10-28 — validated against the create-story checklist: GET forms use `require_role_with_return` (Anonymous bounces back to the form), mutations `require_role`; router-level tests build the router with `tests/common/mcp.rs::router` + `tests/common/web.rs::Browser`; the wishlist note is plain text (rendered escaped by the wishlist pages); placement must skip settled tomes using the predicate-reduced `settled_title_id` (`list_by_series`), never the raw column.
- 2026-10-28 — implementation complete (review).
- 2026-10-28 — code review: round 1 (3 layers) 4 Medium + Lows fixed, round 2 clean (2 Lows fixed, 1 deferred to #526) (done).
