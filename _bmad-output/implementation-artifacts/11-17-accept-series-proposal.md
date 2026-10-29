# Story 11.17: Accept a series proposal

Status: ready-for-dev

GitHub issue: part of #493 (MCP server — do NOT close it here); closes #529 (re-validation at acceptance); decides #521 for the series kinds (narrowed to what 11-18 must do). Covers: FR138 (accept, series), FR140, FR146 (accept). Epic 11 — seventeenth story, the **first story that applies a proposal to the catalog**. Not here: metadata / cover acceptance (11-18), editing a proposal before accepting it (11-19), side-by-side view (11-20), nav badge (11-21).

## Story

As a librarian,
I want to accept a series proposal,
so that the series, its attachments and its expected tomes enter the catalog in one step.

## Existing-code reality check (code map, 2026-10-29, `main` 6c5ca48)

| Place | Today | Consequence |
|---|---|---|
| `services/proposals.rs` (1440 lines) | `transition_in(Accept)` locks the proposal (`FOR UPDATE`), its items (`LOCK IN SHARE MODE`), its targets (`FOR UPDATE`, tables in enum order: titles then series), refuses `NothingToApply`, marks `stale` and returns `Transition::WentStale` when a target's `version` moved or it was deleted; `check_staleness_in` does the same without deciding | acceptance code lives in a **new** module (Rule #12); it calls `check_staleness_in` then `transition_in` in one transaction |
| #521 | `title_series` attach / detach and expected-tome writes bump neither `titles.version` nor `series.version` (D-11-9-8, D-11-10-6) | version checks alone miss "position taken meanwhile", "title already in the series" — acceptance re-validates the series' **state** under the series row lock (D-11-17-2) |
| `services/series_proposals.rs` (1356 lines) | item shapes (module doc, D-11-12-2); pure `validate_lines` (positions, omnibus span, overlaps, tome title/ISBN, closed total); `propose_attach` checks lines against `AgentSeriesDetail` inline | extract those inline checks into a **pure** `check_against_series(lines, held, tomes)` used both at filing and at acceptance (DRY) |
| `services/expected_tomes.rs` | `ExpectedTomeService::create_in(conn, series_id, input, origin)` — locks the series row, checks the slot, inserts, settles with an owned title (attach when free; `TitleElsewhere` leaves the tome unsettled) | tomes are created with `ExpectedTomeOrigin::Proposal`; a tome whose ISBN belongs to a title that (after this acceptance) sits elsewhere in the series is **skipped** (#529-1, D-11-17-4) |
| `models/series.rs` | `SeriesModel::create(pool…)`, `TitleSeriesModel::assign_in(conn…)`, `assign_omnibus(pool…)` (own transaction) | add `SeriesModel::create_in(conn…)` and `TitleSeriesModel::assign_omnibus_in(conn…)`; the pool versions delegate |
| `title_series` | UNIQUE `(title_id, series_id, position_number)` only — no uniqueness on a series position | positions are guarded by the locked re-validation, not by the schema |
| `services/proposal_review.rs` (1009 lines) / `routes/proposals.rs` (620) | reject flow: `lock_status_version` → `transition_in(Reject)`; `ReviewError` localized; success = re-rendered `#proposal-detail-body` + OOB entry + `HX-Trigger-After-Swap: modal-close` (D-11-16-6) | accept mirrors it; new web glue in `services/proposal_accept.rs`, handlers next to reject |

## Acceptance Criteria

1. **AC1 — accept action (FR138, UX-DR8).** A pending `series_create` / `series_attach` proposal's detail page shows an **Accept** button next to Reject. It opens a UX-DR8 modal (`#modal-slot`, variant `warning`, no `hx-confirm`) summarising what will be applied (series created or target series, number of titles, number of expected tomes), carrying CSRF and the `version` the page showed (missing / garbled → localized "changed" conflict, D-11-16-11). Proposals of other kinds show no Accept button (11-18).
2. **AC2 — apply (FR138, FR146).** `POST /proposals/{id}/accept` (Librarian+, CSRF, `version`) runs **one transaction**: lock the proposal and compare `version` (moved → conflict, nothing done); lock order proposal → items → series (attach) → titles; staleness re-checked (`check_staleness_in`); the lines re-validated against the catalog as it is now (AC3); then `transition_in(Accept)` (records decider + `decided_at`, FR146) and the apply through `services/`: `series_create` creates the series (name, type, total), `series_attach` uses the target series; each title line is attached at its position (an omnibus on `position..=end_position`, `is_omnibus` rows); each expected-tome line is created with origin `proposal` through `ExpectedTomeService::create_in` (auto-settle rules D-11-10-*). For `series_create` the created series id is recorded on the proposal (`proposals.applied_series_id`) so the page can link it.
3. **AC3 — staleness (FR140, #521, #529).** Nothing is applied and the proposal is marked `stale` (decided at, no decider — D-11-9-7), the mark committed, when: a target title / series was changed (version) or deleted since filing; or, re-checked under the locks: (create) an active series with the same name now exists; (attach) a title line's title is already in the series, a position is held by a live title, a position or ISBN is taken by a live expected tome, a position is above the series' closed total. The answer re-renders the body (status "Outdated") and shows a warning entry that says, in plain words, what changed, that nothing was added, and what to do (`docs/error-message-style.md`).
4. **AC4 — rollback.** Any failure while applying (database error, unexpected conflict) rolls the whole transaction back: no series, attachment or tome is left, and the proposal stays `pending` (decider not recorded); the librarian reads a localized "could not be applied, nothing changed, still pending" message in the modal.
5. **AC5 — result shown.** On success the modal closes after the swap, `#proposal-detail-body` shows status "Accepted", decider and date, a link to the series, and no Accept / Reject buttons; an OOB success entry in `#feedback-list` summarises the result (series created or updated, titles attached, tomes added — of which settled at once — and tomes skipped with the reason) with a link to the series page.
6. **AC6 — other refusals.** Already decided (names the status), changed since opened, unknown proposal, nothing left to apply (11-19 case), unknown decider — localized, in the modal's error region (retarget guard).
7. **AC7 — labels, CSP, roles.** Every new label in `locales/{en,fr,de,it}.yml` (`locale_parity`); no inline script/style; CSRF field in the POST form; `docs/route-role-matrix.md` rows for the two new routes.
8. **AC8 — tests.** Unit: item parsing back to lines (incl. malformed), `check_against_series` cases, result / stale-reason localization ×4. DB integration (`#[sqlx::test]`): accept `series_create` (series, titles incl. omnibus, tomes with origin `proposal`, an owned-ISBN tome settled at once, decider recorded, `applied_series_id`), accept `series_attach`, stale by version (title edited, series deleted), stale by state (#521: position assigned by hand, title attached by hand, tome added by hand; #529: second of two creates with the same name, second of two attaches on the same positions), tome skipped when its ISBN's title is placed elsewhere (#529-1), rollback (a failure injected while inserting tomes leaves nothing and keeps `pending`), version conflict, not pending, CSRF / role through the router. E2E: an agent files a `series_create` over MCP → a Librarian (blank browser, `loginAs`) accepts it in the modal → sees "Accepted" and opens the series page with its titles and expected tomes; stale path: an attach proposal whose series is edited before acceptance → "Outdated" + explanation.

## Tasks / Subtasks

- [ ] Migration `proposals.applied_series_id BIGINT UNSIGNED NULL` (no FK — purge); `ProposalRow` / `Proposal` field (AC2, AC5)
- [ ] Models: `SeriesModel::create_in`, `TitleSeriesModel::assign_omnibus_in`, `TitleSeriesModel::held_positions_in` (locking read), `ExpectedTomeModel::list_by_series_in` (locking read), `ProposalModel::set_applied_series_in` (AC2, AC3)
- [ ] `series_proposals.rs`: pure `check_against_series` extracted from `propose_attach` (filing unchanged in behaviour) (AC3)
- [ ] New `services/series_acceptance.rs`: parse stored items → lines; `accept_series_in(conn, id, user, version)` → `AcceptOutcome::{Accepted(AppliedSeries), Stale(StaleCause)}`; typed `AcceptError` (AC2–AC4, AC6)
- [ ] Web glue `services/proposal_accept.rs` (transaction wrapper, localized result / stale / error copy), handlers `accept_modal` + `accept` in `routes/proposals.rs`, routes registered (AC1, AC5, AC6)
- [ ] Templates: Accept button in `fragments/proposal_detail_body.html`, `fragments/proposal_accept_modal.html`, series link for accepted creates (AC1, AC5)
- [ ] Locale keys ×4; route-role matrix (AC7)
- [ ] Tests: unit, `tests/proposal_accept_series.rs`, E2E `series-proposal-accept.spec.ts` (spec id `SA`) (AC8)
- [ ] Decisions D-11-17-*; #521 narrowed for 11-18, #529 closed

## Dev Notes

- **Transaction order (one tx, short, no I/O):** `lock_status_version` (FOR UPDATE; pending + version check) → `items(lock)` → for attach `lock_series_in(series_id)` → `check_staleness_in` (re-locks proposal, locks titles then series — already held) → stale ⇒ commit + `Stale(Version)` → parse lines → re-validate (create: `active_series_named`; attach: `held_positions_in` + `list_by_series_in`, both locking reads, + `check_against_series` + closed total via `validate_lines`) → conflict ⇒ `transition_in(MarkStale)` + commit + `Stale(State)` → `transition_in(Accept)` (returns `Decided` — `WentStale` here is unreachable but handled as stale) → apply → commit. Every read before the series lock is a locking read, so no snapshot predates the lock (D-11-12-13 argument).
- **Lock order:** series row before title rows, like auto-settle (series → `title_series` insert, which share-locks the title via the FK) — avoids a series/title cycle with a concurrent scan-settle.
- **Apply order:** series → title lines → tome lines, so a tome's owner placed by this proposal is seen by the skip rule and by `create_in`'s settle.
- **Tome skip rule (#529-1):** before `create_in`, if the tome's ISBN is carried by a live title that holds another position of the series (first owner by id, or an owner holding the tome position → not skipped — same choice as `settle_with_owned_title`), the tome is not created and is reported as skipped.
- **Rollback test:** inject a failure with a test-only trigger `BEFORE INSERT ON series_expected_tomes … SIGNAL SQLSTATE '45000'`.
- **E2E:** spec `series-proposal-accept.spec.ts` (sorts after `empty-states`, `dewey-code`, `classification-fieldset` — D-11-11-6, #528); series names unique per run; key created by admin via the UI helper used in `mcp-series-proposals.spec.ts`.
- Pitfalls: CI charset (`CAST(… AS CHAR)`), no repo-wide `cargo fmt`, `mem-guard` + `-j 2`, French copy in router tests, Askama escapes `<`/`&` as `&#60;`/`&#38;`.

### Validation notes (create-story checklist, 2026-10-29)

- Reuse verified: `check_staleness_in`, `transition_in`, `lock_status_version`, `ExpectedTomeService::create_in`, `TitleSeriesModel::assign_in`, `ProposalModel::active_series_named`, `validate_lines`, the UX-DR8 macro, `into_response_with_hx_trigger_after_swap`, `utils::feedback_html_with_link`, E2E helpers `createApiKey` / `mcpCallTool` / `titleIdFromSkeleton`. No new crate.
- One migration (`applied_series_id`): `ProposalRow` / `Proposal` are built in `proposals.rs::detail` and a test in `proposal_review.rs` — both get the field. Dynamic queries only, no `.sqlx` change expected.
- Regression watch: `propose_attach` must keep its exact refusals after the `check_against_series` extraction (`tests/mcp_series_proposals.rs`); `templates_audit` (CSRF, no inline handlers); route-role matrix; `locale_parity`; `proposals_page.rs` assertions on the reject button.
- Disaster prevention: never apply after a `WentStale` / state conflict; never leave the `accepted` status without the applied rows (same transaction); no file or network I/O in the transaction (D-11-9-12); lock series before titles.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.17] — FR138, FR140, FR146.
- [Source: epic-11-autonomous-decisions.md] — D-11-9-7/-8/-9/-11/-12, D-11-10-5/-6/-10/-11, D-11-12-2/-3/-4/-10/-13, D-11-16-5/-6/-10/-11.
- [Source: src/services/series_proposals.rs module doc] — item shapes.
- [Source: GitHub #521, #529].
- [Source: docs/error-message-style.md].

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

### File List
