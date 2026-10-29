# Story 11.18: Accept a metadata or cover proposal

Status: ready-for-dev

GitHub issue: part of #493 (MCP server — do NOT close it here); closes #521 (staleness of writes that do not bump `version`, narrowed by 11-17 to the metadata / cover kinds), #531 (a contributor removed from a title cannot be re-added), #533 (refuse an acceptance whose staged cover is missing). Covers: FR138 (accept, metadata and cover), FR140, FR144, FR146 (accept). Epic 11 — eighteenth story. Not here: editing a proposal before accepting it (11-19), side-by-side view (11-20), nav badge (11-21).

## Story

As a librarian,
I want to accept a metadata or cover proposal,
so that a title I could not complete gets the information or cover the agent found.

## Existing-code reality check (code map, 2026-10-29, `main` 8515e13)

| Place | Today | Consequence |
|---|---|---|
| `services/series_acceptance.rs` (909 lines) | `accept` (deadlock retry, own transaction) → `accept_in`: `lock_for_decision` (kind, status, version), items (share lock), series lock, `check_staleness_in`, state re-validation, `transition_in(MarkStale / Accept)`, apply. Types `AcceptOutcome`, `AcceptError` (incl. `WrongKind`), `StaleCause`, `AcceptConflict` live here | the kind-independent pieces (preamble, deadlock retry, outcome / error / stale types) move to a new `services/acceptance.rs` (DRY, Rule 12), re-exported by `series_acceptance` so its callers keep compiling; `AcceptOutcome` becomes generic over what was applied (default `AppliedSeries`) |
| `services/metadata_proposals.rs` (1465) | item shapes in the module doc (D-11-13-2): one `"line":"field"` item per field, `current` next to `proposed`, `current_truncated` when the current value was cut; `validate_fields`, `normalize_text` | acceptance parses the items back, re-validates `proposed` with `validate_fields` (roles: the live role of the stored `role_id`), compares `current` with the live value (normal form) unless truncated |
| `services/cover_proposals.rs` (730) / `cover_staging.rs` (490) | one `"line":"cover"` item: `staged_file`, `replaces_existing`, `current` (cover URL at filing, cache-buster included); `cover_staging::promote` (→ `/covers/{id}.jpg`, `StagingError::Missing`); purge keeps an accepted proposal's file 1 h | the existence check runs **before** the transaction, promotion **after** its commit (D-11-9-12) |
| `services/proposal_accept.rs` (376) / `routes/proposals.rs` (767) | `can_accept` = pending series kinds; `accept` handler calls `series_acceptance::accept`; localized copy (applied / stale / errors) | `can_accept` covers every kind; the handler dispatches on the kind through `acceptance::accept`; new copy for metadata / cover results and conflicts |
| `models/contributor.rs` | `TitleContributorModel::add_to_title` plain INSERT; unique key `(title_id, contributor_id, role_id)` ignores `deleted_at` (#531) | `add_to_title_in(conn…)` reactivates a soft-deleted junction row before inserting; the pool version delegates (manual flow fixed too) |
| `tasks/metadata_fetch.rs` | `do_update` skips fields named in `manually_edited_fields` (`subtitle`, `description`, `publisher`, `publication_date`, `dewey_code`); `update_cover_image_url` skips when `cover_image_url` is marked; contributors are not tracked there (the fetch only adds an author, `INSERT IGNORE`) | acceptance marks the written scalar fields (`publication_year` as `publication_date`) and the cover; contributors need no mark |
| `services/proposal_review.rs` (1028) / `fragments/proposal_detail_body.html` | a decided cover proposal shows "the proposed image is no longer available" (D-11-16-10 note) | an **accepted** cover proposal shows the title's live cover as "now the title's cover" |

## Acceptance Criteria

1. **AC1 — accept action (FR138, UX-DR8).** A pending `metadata` or `cover` proposal's detail page shows the **Accept** button (11-17's modal, `warning` variant, CSRF, the page's `version`). The modal summarises what is applied: for metadata the fields that will be written and the number of contributors proposed; for a cover, that the image becomes the title's cover (replacing the current one when there is one). Same note as 11-17: a changed catalog means nothing is applied.
2. **AC2 — metadata apply (FR138, FR144, FR146).** One transaction: lock the proposal and compare `version`; items (share lock); `check_staleness_in` (locks the title, compares `version`); the title's live values read under that lock; staleness by state (AC4); `transition_in(Accept)` (decider, `decided_at`); then the scalar fields are written (`publication_year` as `YYYY-01-01`), the written fields are added to `manually_edited_fields` (`publication_year` tracked as `publication_date`), `titles.version` is bumped; each proposed contributor is resolved with `find_or_create` semantics (same name by the SQL collation → reused, else created), then skipped when the title already has that contributor id + role (live junction row) or when the item resolves two entries to the same contributor + role, else added (a soft-deleted junction row is reactivated, #531).
3. **AC3 — cover apply (FR138, FR144, FR146, #533).** Before the transaction, the staged file must exist (`<COVERS_DIR>/.staging/<staged_file>`, name checked with `is_staged_name`; staging unavailable counts as missing) — else the acceptance is **refused** (409, nothing changed, still pending, "the proposed image is no longer available — reject it and ask the assistant again"). The transaction: lock, version, items, `check_staleness_in`, staleness by state (AC4), `transition_in(Accept)`, bump `titles.version` (so a second pending cover / metadata proposal for the title goes stale). After the commit: `cover_staging::promote` (replaces `covers/{id}.jpg`), then `cover_image_url = /covers/{id}.jpg?v=<unix time>` and `cover_image_url` added to `manually_edited_fields`. If promotion fails after the commit (`Missing` race, I/O), the cover is not written, the proposal stays accepted, and the librarian reads a warning saying the image could not be installed and what to do.
4. **AC4 — staleness (FR140, #521).** Nothing is applied and the proposal is marked `stale` (committed, no decider) when: the title was changed (`version`) or deleted since filing; or, under the title lock, (metadata) a scalar item's stored `current` differs from the live value in normal form (`publication_year`: the stored `current_date`, else its year, against the live `publication_date`) — not checked when `current_truncated` (version only); a proposed contributor's role was deleted; (cover) the live `cover_image_url` differs from the stored `current`. The answer re-renders the body ("Outdated") with a warning entry naming what changed.
5. **AC5 — refusals and rollback.** Malformed stored items (unknown field, bad value by `validate_fields`, missing staged name) → 400 "cannot be applied as it is" (still pending, D-11-17-6). Any failure while applying rolls the transaction back (still pending, 409 "nothing changed"). Already decided / changed since opened / unknown / unknown decider: 11-16 / 11-17 copy.
6. **AC6 — result shown.** On success the modal closes after the swap, the body shows "Accepted", decider and date; an OOB success entry summarises the result — metadata: fields updated, contributors added and already present; cover: the title now shows the cover — with a link to the title page. An accepted cover proposal's detail shows the title's live cover instead of "no longer available" (D-11-16-10 note).
7. **AC7 — re-download protection (#232).** A later automatic metadata re-download (`update_title_from_metadata`) leaves the accepted scalar fields untouched, and a later cover re-fetch (`update_cover_image_url`) leaves the accepted cover untouched.
8. **AC8 — #531.** Removing a contributor from a title then adding the same contributor with the same role back succeeds (manual flow and acceptance), reactivating the junction row; adding a contributor already live on the title still answers the duplicate message.
9. **AC9 — labels, CSP, roles.** New labels in the four locale files (`locale_parity`); no inline script / style; no new route (the 11-17 routes serve every kind).
10. **AC10 — tests.** Unit: item parsing (metadata, cover, malformed), the field comparison (normal form, truncated, year / date), localization ×4 of results, conflicts and the new refusal. DB integration (`#[sqlx::test]`, real router): metadata acceptance (fields, `YYYY-01-01`, `manually_edited_fields`, version bump, decider), contributor reuse + creation + already present + reactivated junction row, stale by version, stale by `current` (a write that does not bump the version), stale by deleted role, cover promotion (file moved, URL cache-busted, marked edited), replacement of an existing cover, stale cover (background fetch wrote another cover), missing staged file refused (still pending), re-download protection, #531 regression on the manual flow. E2E: an agent files a metadata and a cover proposal over MCP → a Librarian (blank browser, `loginAs`) accepts both → the title page shows the new values and the cover; a stale path for each.

## Tasks / Subtasks

- [ ] `services/acceptance.rs`: shared `AcceptError`, generic `AcceptOutcome<A>`, `StaleCause`, `AcceptConflict` (+ metadata / cover variants), deadlock retry, locked preamble; `accept` dispatching on the kind; `series_acceptance` re-exports and uses them (AC2–AC5)
- [ ] #531: `TitleContributorModel::add_to_title_in` reactivates a soft-deleted row; `ContributorModel::find_by_name_exact_in` / `create_in`; regression test (AC8)
- [ ] `services/metadata_acceptance.rs`: parse items, compare, apply (AC2, AC4, AC5)
- [ ] `services/cover_acceptance.rs`: pre-check, transaction, post-commit promotion and write (AC3–AC5)
- [ ] Web: `can_accept` for every kind, modal summary per kind, result / stale / refusal copy, handler dispatch, accepted cover display (AC1, AC6)
- [ ] Locale keys ×4 (AC9)
- [ ] Tests: unit, `tests/proposal_accept_metadata_cover.rs`, E2E `metadata-cover-proposal-accept.spec.ts` (AC10)
- [ ] Decisions D-11-18-*; close #521, #531, #533

## Dev Notes

- **Metadata transaction:** `lock_for_decision` → kind / status / version → items (share) → parse + re-validate → `check_staleness_in` (locks the title `FOR UPDATE`) → stale ⇒ commit → live values (`FOR UPDATE` read of the title row, `FOR UPDATE` read of its `title_contributors` rows incl. soft-deleted ones — next-key locks on `idx_title_contributors_title` keep a concurrent manual add out) → roles of the stored `role_id`s (live?) → conflict ⇒ `transition_in(MarkStale)` + commit → `transition_in(Accept)` → write fields + contributors → commit.
- **Cover:** existence check outside (no file I/O inside, D-11-9-12); inside only database work; promotion and URL write after the commit; the URL write is its own short transaction (`FOR UPDATE` on the title for the `manually_edited_fields` merge), no second version bump.
- **Comparison normal form:** `metadata_proposals::normalize_text(…, multiline)` on both sides (`description` multi-line); `None` ↔ `null`.
- **Contributors are additions only** (D-11-13-4): their `current` list is display only, never a staleness baseline.
- **E2E:** spec id `MA`; titles created by scanning `specIsbn("MA", n)`, wait for the metadata fetch to settle before filing (it bumps the version); no series.
- Pitfalls: CI charset (`CAST(… AS CHAR)`), no repo-wide `cargo fmt`, `mem-guard` + `-j 2`, French copy in router tests, Askama escaping.

### Validation notes (create-story checklist, 2026-10-29)

- Reuse verified: `ProposalModel::lock_for_decision` / `items(conn, id, true)`, `ProposalService::check_staleness_in` / `transition_in`, `metadata_proposals::{validate_fields, normalize_text}`, `cover_staging::{promote, is_staged_name, staging_dir_for}`, `AppState::cover_staging_dir()` / `covers_dir`, `tasks::metadata_fetch::{update_title_from_metadata, update_cover_image_url}` (pub, used by existing tests), 11-17's accept modal / handler / `decided_response`, E2E `mcpCallTool` and the proposals-review / series-proposal-accept specs' key and scan helpers. No new crate, no migration, no new route.
- Regression watch: `tests/proposal_accept_series.rs` (types moved but re-exported), `proposals_page.rs` (Accept button now on every pending kind), `contributor` manual flow (duplicate message kept for a live row), `cover_manual_survives_refetch.rs`, `templates_audit`, `locale_parity`.
- Disaster prevention: no file I/O inside the decision transaction; never write `cover_image_url` unless the promotion succeeded; never mark `accepted` without the metadata writes (same transaction); lock the title before its junction rows (same order as `check_staleness_in`).

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.18] — FR138, FR140, FR144, FR146.
- [Source: epic-11-autonomous-decisions.md] — D-11-9-12, D-11-13-2/-4/-6/-9/-10, D-11-14-5/-6/-8/-11, D-11-16-6/-10, D-11-17-2/-3/-5/-6.
- [Source: module docs of services/metadata_proposals.rs, services/cover_proposals.rs, services/cover_staging.rs].
- [Source: GitHub #521 (latest comment), #531, #533].
- [Source: docs/error-message-style.md].

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

### File List
