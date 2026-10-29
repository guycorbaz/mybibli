# Story 11.16: Proposals page — list, detail, reject

Status: ready-for-dev

GitHub issue: part of #493 (MCP server — do NOT close it here); closes #536 (reject-then-refused E2E). Covers: FR137, FR138 (reject), FR146 (reject), UX-DR31 (list, detail, reject part). Epic 11 — sixteenth story, the **first librarian-facing web surface for proposals**. Not here: accept (11-17 series, 11-18 metadata / cover), edit before accepting (11-19), side-by-side current vs proposed view (11-20), nav entry + pending badge + dashboard indicator (11-21).

## Story

As a librarian,
I want to see pending proposals and turn down the ones I do not want,
so that the agent's suggestions are reviewed in one place.

## Existing-code reality check (code map, 2026-10-29, `main` f17418c)

| Place | Today | Consequence |
|---|---|---|
| `src/services/proposals.rs` (1440 lines) | `ProposalService::{list, detail, transition, transition_in}`; `Decision::Reject { by, reason }` normalises the reason (`normalize_rejection_reason`: trim, empty → None, ≤ `MAX_REJECTION_REASON_CHARS` = 1000 chars → `ReasonTooLong`), checks an active decider (`UnknownDecider`), `NotPending` / `NotFound`; records `decided_by`, `decided_by_name`, `decided_at`, bumps `version` | the reject handler goes through `transition_in` in its own transaction after an optimistic-lock check on `proposals.version`; no new decision logic. Rule #12: the new code goes in a new module `services/proposal_review.rs` (file already 1440 lines) |
| `src/models/proposal.rs` (946 lines) | `ProposalModel::list(filter{status, kind, api_key_id}, page)` (25 / page, newest first, `ProposalSummaryRow` with key label, item count, reason, equivalence key); `subject_lines(ids)`; `find`, `items`, `lock_status` | list reuses `ProposalService::list` + `subject_lines` (11-15's `ProposalSubject::from_lines`) for the "target" column; add `lock_status_version` and two batch label reads (live titles, live series) |
| Item shapes | series (`services/series_proposals.rs` module doc), metadata (`metadata_proposals.rs`), cover (`cover_proposals.rs`) — see D-11-12-2, D-11-13-2, D-11-14-5 | the detail page parses each item into a typed display line; any item that matches no shape is shown as escaped JSON (never dropped) |
| `services/cover_staging.rs` | `discard(staging_dir, name)` (missing file OK), `is_staged_name`; `/covers` refuses dot-segments; `AppState::cover_staging_dir()` | reject calls `discard` **after the commit** for each cover item's `staged_file` (failure logged; the scheduled purge catches leftovers); a new Librarian-only route serves a staged file by **proposal id** (the name comes from the DB and is re-validated with `is_staged_name`) |
| UX-DR8 modal | `templates/components/modal.html` macro with `inner_form_html`; Pattern A in `#modal-slot` (`static/js/modal.js`, listens to `HX-Trigger: modal-close`); `ModalConfirmRetargetGuard` keeps a Confirm's error inside the modal | reject modal = macro, variant `remove` (amber: a decision, not a deletion); the reason `<textarea>` goes in `inner_form_html` |
| `#526` | the series page has no `#feedback-list`, so `AppError` retargets land nowhere | the proposals list and detail pages carry `<div id="feedback-list">` |
| 11-15 E2E (`mcp-proposal-history.spec.ts`) | sets the delay to 45 then restores 90, serial | a rejection right after filing is refused with 45 or 90 alike — no coupling as long as the delay is not 0 |

## Acceptance Criteria

1. **AC1 — access (FR137).** `GET /proposals` and every route below require Librarian or Admin. An anonymous visitor is redirected to login with a `next` back to the page (`require_role_with_return`); HTMX-only fragments answer 405 to a non-HTMX request, like the other modal fragments.
2. **AC2 — the list (FR137, UX-DR31).** `/proposals` shows one row per live proposal: kind (localized), target (the proposed series name for a creation, else the live series / title name linking to its page, or "no longer in the catalog" with its id when the target is deleted), filing date, API key label, status (localized badge), item count, and a link to the detail. Filters `status` (pending — the default —, accepted, rejected, stale, all) and `kind` (all, series_create, series_attach, metadata, cover) via a GET form; unknown values fall back to the default. 25 per page, newest first, with previous / next links that keep the filters. An empty result shows an empty state. The page has a `#feedback-list` region (#526 lesson).
3. **AC3 — the detail (FR137, NFR47).** `GET /proposals/{id}` shows the header (kind, status, filing date, key label; for a decided proposal who decided and when — "the system" for stale — and the rejection reason), the sources as links opening in a new tab (`target="_blank" rel="noopener noreferrer"`), and the items by shape: series line (name / type / total, or the target series), title lines (title link, position, omnibus range), expected-tome lines (position, title, ISBN), metadata fields (field label, current value, proposed value; contributors as name + role lists; `publication_year` with its current full date), cover (proposed image served by AC4, the title's current cover next to it, dimensions and size). Every agent value is rendered as escaped text (Askama auto-escape, never `|safe`). An unknown / soft-deleted proposal is a 404. An item matching no shape is shown as escaped JSON.
4. **AC4 — staged cover route.** `GET /proposals/{id}/cover` (Librarian+) answers the staged JPEG of a cover proposal (`image/jpeg`, `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`); 404 when the proposal is unknown, not a cover proposal, the stored name fails `cover_staging::is_staged_name`, staging is unavailable, or the file is gone. The file name never appears in a URL. `/covers/.staging/…` stays 404.
5. **AC5 — reject (FR138, FR146, D-11-9-7).** A pending proposal's detail shows a "Reject" button opening a UX-DR8 modal (`#modal-slot`, no `hx-confirm`) with an optional reason (`<textarea maxlength="1000">`), CSRF token and the proposal's `version`. `POST /proposals/{id}/reject`: role check, optimistic lock (a version that moved → localized conflict), `transition_in(Reject)` in one transaction → status `rejected`, `decided_by` / `decided_by_name` / `decided_at` recorded, reason trimmed (empty → none); after the commit every cover item's staged file is discarded (`cover_staging::discard`; failure logged, not shown). Success: `HX-Trigger: modal-close`, the re-rendered detail body (status rejected, decider, reason, no Reject button) and an OOB success entry in `#feedback-list`. Refusals, localized per `docs/error-message-style.md`: reason over 1000 characters (field hint), already decided (names the status, tells to reload), changed since opened, unknown proposal; they land in the modal's error region (retarget guard). Non-pending proposals show no Reject button.
6. **AC6 — labels, CSP.** Every new label and message in `locales/{en,fr,de,it}.yml` (`cargo test --test locale_parity`); no inline script / style / handlers (`templates_audit` green); every POST form has the CSRF hidden field; no `hx-confirm`.
7. **AC7 — tests.** Unit: shape parsing per kind (incl. unknown shape fallback), filter parsing, target labels, error localization in four locales. DB integration (`#[sqlx::test]`, real router): listing (default pending, status / kind filters, pagination, key label, deleted target), detail (escaping of a `<script>` value, sources `target=_blank rel=noopener noreferrer`, items per kind), role guard (anonymous → login, librarian OK), reject with and without a reason (decided by / at, reason stored, too-long reason refused, not pending refused, version conflict refused, CSRF required), discard of a staged cover on rejection, staged-cover route (served, 404 cases, non-cover). E2E: a Librarian logs in from a blank browser, opens a proposal, rejects it with a reason; then the agent's equivalent re-filing is refused with the reopening date and `list_my_proposals` shows the reason and `resubmittable_from` (#536).

## Tasks / Subtasks

- [ ] Model: `lock_status_version`, `live_title_briefs(ids)`, `live_series_names(ids)` (AC2, AC3, AC5)
- [ ] `services/proposal_review.rs`: `ListFilter` parsing, `list_page` (rows + subject + target label), `ReviewLine` parsing per shape, `review_detail`, `reject` (+ discard after commit), `ReviewError` with `localized(loc)` / `into_app_error(loc)` (AC2, AC3, AC5)
- [ ] `routes/proposals.rs`: list, detail, cover, reject-modal, reject; register in `routes/mod.rs` (AC1–AC5)
- [ ] Templates: `pages/proposals.html`, `pages/proposal_detail.html`, `fragments/proposal_detail_body.html`, `fragments/proposal_reject_modal.html` (AC2–AC6)
- [ ] Locale keys ×4 (AC6)
- [ ] Tests: unit, `tests/proposals_page.rs`, E2E `proposals-review.spec.ts` (AC7)
- [ ] `docs/route-role-matrix.md` rows; decisions D-11-16-*

## Dev Notes

- **Transaction for reject:** `BEGIN` → `lock_status_version` (`SELECT status, version … FOR UPDATE`) → `NotFound` / `NotPending` / version mismatch → `transition_in(Reject)` → staged names of the proposal's cover items read in the same transaction (`staged_cover_files_of`) → `COMMIT` → `discard_all`. No file I/O inside the transaction (D-11-9-12).
- **Version absent:** the modal macro omits `version` when it is 0; proposals start at version 1, so the form always carries it. A missing field reads as 0 → conflict.
- **Escaping:** values may hold `<`, quotes, line breaks (description). Render with `{{ }}` only; descriptions in a `whitespace-pre-line` block.
- **Dates:** the DB session runs in UTC; show `YYYY-MM-DD HH:MM UTC`.
- **Staged cover route:** read the item through `ProposalService::detail`, take the first `"line": "cover"` item, `is_staged_name` → read with `tokio::fs::read` from `state.cover_staging_dir()`.
- **Target labels:** batch reads `SELECT id, title, cover_image_url, media_type FROM titles WHERE id IN (…) AND deleted_at IS NULL` and `SELECT id, name FROM series WHERE id IN (…) AND deleted_at IS NULL` (dynamic queries, no `.sqlx` change).
- **E2E:** spec id `PR`; a fresh title per run (specIsbn); a metadata proposal (unique title, so the rejection blocks nothing else); creates no series (#528); key created and revoked by an admin; then `clearCookies` + `loginAs(page, "librarian")` for the journey. French copy matched with i18n regexes.
- Pitfalls: CI charset → `CAST(… AS CHAR)` on string / JSON expressions; no repo-wide `cargo fmt`; `mem-guard` on heavy commands.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.16] — FR137, FR138, FR146, UX-DR31, NFR47.
- [Source: epic-11-autonomous-decisions.md] — D-11-9-5 (sources), D-11-9-7 (reason, stale decider), D-11-9-11 (decider name), D-11-12-2, D-11-13-2, D-11-14-2/-5 (shapes, staging), D-11-15-7 (#536).
- [Source: src/services/{series,metadata,cover}_proposals.rs module docs] — item shapes.
- [Source: docs/error-message-style.md].

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5

### Debug Log References

### Completion Notes List

### File List
