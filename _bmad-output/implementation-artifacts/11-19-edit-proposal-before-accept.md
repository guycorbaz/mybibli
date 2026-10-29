# Story 11.19: Edit a proposal before accepting it

Status: review

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR139, UX-DR31 (inline edit and line removal before acceptance). Epic 11 — nineteenth story. Not here: side-by-side view (11-20), nav badge (11-21), cross-kind staleness (#542 stays open for the user).

## Story

As a librarian,
I want to correct a proposal rather than reject it,
so that I keep the agent's work while fixing its mistakes.

## Existing-code reality check (code map, 2026-10-29, `main` ed97afa)

| Place | Today | Consequence |
|---|---|---|
| `proposal_items` (migration `20261028120000`) | `proposed_values JSON`, AR24 quartet (`deleted_at`, `version`); every read filters `deleted_at IS NULL` (`ProposalModel::items`, `subject_lines`, the list's `item_count`, the duplicate reads); not in `soft_delete::ALLOWED_TABLES` (never purged on its own) | a removed line = a soft-deleted item: acceptance, staleness, subject and count ignore it with no change; a new migration adds the trail columns `original_values JSON NULL`, `edited_by_name`, `edited_at` |
| `services/acceptance.rs` | `lock_pending_in` returns `NothingToApply` when no live item is left (anticipated for 11-19); copy `proposals.error_nothing_to_apply` exists | an attach whose title / tome lines were all removed must also answer `NothingToApply` (its series line remains) |
| `services/series_acceptance.rs::parse_items` | parses stored items back into `StoredSeriesProposal` | an edit is checked by re-parsing the proposal **with the edited item** and running the filing validation (`validate_series_name`, `validate_total`, `validate_lines`, `check_against_series` for the edited line of an attach) |
| `services/metadata_proposals.rs` | `validate_fields(map, roles, year)`; contributors as `{name, role}` | an edited scalar or contributor list is validated with the filing rules against the **live** roles; `current` / `current_date` / `current_truncated` stay as filed (staleness baseline, D-11-13-2) |
| `services/metadata_acceptance.rs` | `parse_items` → `revalidate` → `paired_contributors` | run on the edited proposal before saving: an edit can never leave a proposal acceptance refuses as malformed |
| `services/proposal_review.rs` | `LineView` per item, `ReviewDetail`, `ReviewError` (localized) | lines carry their item id and edit / remove affordances; the detail adds the trail (edited lines, removed lines) |
| `routes/proposals.rs` (767 lines) | detail, reject / accept modals, `decided_response` | the edit handlers go to a new `routes/proposal_edit.rs` (Rule 12) |
| `static/js/inline-form.js` | `inline-form-add-toggle` reveals a server-rendered slot; `inline-form-add-cancel` hides it (clears `input[name=name]`) | reuse the toggle; add a `inline-form-slot-cancel` action that hides the slot and resets its form (no field cleared by name) |

## Acceptance Criteria

1. **AC1 — what can be edited (FR139).** On a **pending** proposal's detail page, each editable line has an **Edit** button revealing an inline form (`inline-form.js`, CSP-clean, CSRF via `csrf.js`): series creation line — name, type (open / closed), total; title line — position, omnibus flag and last position; expected-tome line — position, title, ISBN; metadata scalar line — the proposed value (`description` multi-line, `publication_year` a number); contributors line — each proposed contributor's name and role (live roles), a cleared name drops that contributor. The series line of an attach and the cover line have nothing to edit.
2. **AC2 — removing a line (FR139).** Each removable line has a **Remove** button opening the UX-DR8 modal (`remove` variant, page's version); confirming removes the line. The series line can never be removed (D-11-12-2); a cover proposal's single line cannot be removed (reject instead); the last line of a metadata proposal cannot be removed (reject instead — the proposal would lose its subject).
3. **AC3 — same validation as filing.** An edit is refused (400, field hint, nothing saved) when the edited proposal would not pass filing: series name / type / total rules, positions 1..=10 000 and within a closed total, omnibus range, a position used twice in the proposal, tome title / ISBN rules, a tome ISBN twice; for an attach, the edited line against the series as it is now (position held, tome position or ISBN taken, title already in the series); for a creation, an active series with the edited name; metadata values by `validate_fields` (live roles). After saving, the proposal still parses and re-validates as acceptance does (`parse_items` / `revalidate` / `paired_contributors`).
4. **AC4 — the agent's values are kept.** The first edit or the removal of a line stores the agent's values (`original_values`); every edit / removal records who and when. The detail page shows, under an edited line, "Edited by X on D — the assistant proposed: …", and lists removed lines with who removed them and when — before and after the decision. `equivalence_key` does not change (D-11-15-2); the subject is read from the live items (#535 unchanged). An edit that changes nothing is not recorded.
5. **AC5 — optimistic locking.** Every edit and removal carries the proposal `version` the page showed, compares it under the proposal row lock and bumps it; a moved version is refused with the project's usual conflict message (`error.conflict`), nothing saved. The re-rendered page carries the new version (accept / reject / further edits). A decided or stale proposal refuses edits (11-16 copy).
6. **AC6 — nothing left to apply.** A proposal with no line left to apply (an attach whose title and tome lines were all removed) offers only Reject; accepting it anyway is refused (`NothingToApply`, nothing changed).
7. **AC7 — acceptance uses the edited values.** Accepting applies the edited values and ignores removed lines; staleness compares the targets of the live lines only.
8. **AC8 — labels, CSP, roles.** New labels in the four locales (`locale_parity`); no inline script / style; routes Librarian+ (`/proposals/{id}/items/{item_id}`, `/remove-modal`, `/remove`).
9. **AC9 — tests.** Unit: each edit's value building and validation (series line, title, tome, scalar, contributors), refusal classes, error localization ×4, summaries. DB integration (`#[sqlx::test]`, real router): edit + trail columns + version bump; second save with the old version refused; removal; series line / cover / last metadata line not removable; an attach left with only its series line cannot be accepted; accepting applies an edited position and an edited metadata value and skips a removed line; a decided proposal refuses edits. E2E: an agent files a series proposal and a metadata proposal over MCP → a librarian (blank browser, `loginAs`) edits a position, removes a line, accepts, finds the series as edited; edits a metadata value, accepts, finds the edited value on the title page.

## Tasks / Subtasks

- [x] Migration `proposal_items` trail columns (AC4)
- [x] Model: trail read, item update / removal, guarded version bump (AC4, AC5)
- [x] `services/proposal_edit.rs`: edit / remove in one transaction, pure value building + validation, `EditError` localized (AC1–AC5)
- [x] `series_acceptance`: attach with nothing left → `NothingToApply`; `can_accept` aware of applicable lines (AC6)
- [x] Review detail: item ids, edit / remove flags, trail notes, removed lines, roles (AC1, AC4)
- [x] Routes + templates + `inline-form.js` slot cancel (AC1, AC2, AC8)
- [x] Locale keys ×4 (AC8)
- [x] Tests: unit, `tests/proposal_edit.rs`, E2E `proposal-edit-accept.spec.ts` (AC9)
- [x] Decisions D-11-19-*

## Dev Notes

- **Transaction:** `lock_for_decision` (status, version) → items (share) → build + validate → for an attach, `lock_series_in` then the edited line against `held_positions_in` / `live_slots_in` → item `UPDATE` (`original_values` assigned **first**: MariaDB evaluates single-table `SET` left to right) → `UPDATE proposals SET version = version + 1 WHERE id = ? AND version = ?` → commit. Same lock order as acceptance (proposal → items → series), no file I/O.
- **Stored shapes stay the contracts:** title line `{"line":"title","position","is_omnibus"[,"end_position"]}`; tome `{"line":"expected_tome","position","title","isbn"}`; creation `create_series_line(name, type, total)`; field items keep every key but `proposed`; contributors `{name, role, role_id}` with the library's role spelling.
- **Forms:** one `POST /proposals/{id}/items/{item_id}` reading a `HashMap<String, String>` (indexed `contributor_name_N` / `contributor_role_N`); version missing or garbled → conflict.
- **E2E:** spec id `PE`; a series proposal creates a series → the spec sorts after the specs that assume no series (D-11-11-6 / #528): `series-proposal-edit.spec.ts`; wait for the metadata fetch to settle before filing (it bumps the title version).
- Pitfalls: CI charset (`CAST(… AS CHAR)`), no repo-wide `cargo fmt`, `mem-guard` + `-j 2`, French copy in router tests, Askama escaping (`&#39;` etc.).

### Validation notes (create-story checklist, 2026-10-29)

- Reuse verified: `ProposalModel::{lock_for_decision, items, active_series_named}`, `series_acceptance::parse_items`, `series_proposals::{validate_series_name, validate_total, validate_lines, check_against_series, create_series_line}`, `ExpectedTomeModel::{lock_series_in, live_slots_in}`, `TitleSeriesModel::held_positions_in`, `expected_tomes::closed_total_of`, `metadata_proposals::validate_fields`, `metadata_acceptance::{parse_items, revalidate, paired_contributors}`, `ContributorRoleModel::find_all` (plain read before the transaction; acceptance re-checks role liveness under lock), `AcceptError::NothingToApply` + `proposals.error_nothing_to_apply`, the UX-DR8 `modal` macro (`remove` variant), `HtmxResponse::into_response_with_hx_trigger_after_swap` (Pattern A modal + OOB entry, D-11-16-6), `inline-form-add-toggle`.
- Regression watch: `tests/proposals_page.rs`, `tests/proposal_accept_series.rs`, `tests/proposal_accept_metadata_cover.rs`, `tests/proposal_history.rs` (subject from live items), `templates_audit` (no `hx-confirm`, CSRF on `method="POST"` forms — the edit forms use `hx-post`, token via `csrf.js`), `locale_parity`, `tests/seed_drift.rs` (AR24 quartet unchanged).
- Disaster prevention: never touch `current` / `current_date` / `current_truncated` / `target_version` / `equivalence_key`; never remove the series line; never save an edit that acceptance would refuse as malformed; one transaction per edit, proposal row locked first.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.19] — FR139, UX-DR31.
- [Source: epic-11-autonomous-decisions.md] — D-11-12-2, D-11-13-2/-4, D-11-15-2, D-11-16-5/-6/-10/-11, D-11-17-6, D-11-18-3.
- [Source: module docs of services/series_proposals.rs, services/metadata_proposals.rs, services/cover_proposals.rs].
- [Source: docs/error-message-style.md].

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

- Migration `20261030090000_proposal_item_edits.sql`: `proposal_items.original_values`, `edited_by_name`, `edited_at`; a removed line is a soft-deleted item (D-11-19-2).
- `services/proposal_edit.rs`: `edit_line` / `remove_line` (one transaction each, proposal row lock + version, guarded bump), pure value building and filing-rule validation (`check_series_edit`, `edited_proposed`), what each line allows (`editable`, `removable`, `keep_of`, `has_lines_to_apply`), trail summaries (`line_summary`), localized `EditError`.
- `series_acceptance`: an attach with no title / tome line left → `NothingToApply`; the detail page and the accept modal offer Accept only when something is left to apply.
- Review detail: item ids, edit / remove flags, edit notes, removed lines, live roles for the contributor form; routes `routes/proposal_edit.rs`; templates `proposal_detail_body.html` + `proposal_line_edit.html`; `inline-form.js` gains `inline-form-slot-cancel`.
- Decisions D-11-19-1..9.
- Gates (dev): `cargo clippy --all-targets -D warnings` clean; `cargo test` 2071 passed, 0 failed; `sqlx prepare --check` OK (no macro query added); `locale_parity` green; waitForTimeout grep clean; `tsc --noEmit` clean; E2E full suite on a reset stack `CI=true --workers=2`: 380 passed, 2 skipped, 1 known flaky (shelving undo, passed on retry).

### File List

- migrations/20261030090000_proposal_item_edits.sql (new)
- src/models/proposal.rs
- src/services/proposal_edit.rs (new)
- src/services/proposal_review.rs
- src/services/series_acceptance.rs
- src/services/mod.rs
- src/routes/proposal_edit.rs (new)
- src/routes/proposals.rs
- src/routes/mod.rs
- templates/fragments/proposal_detail_body.html
- templates/fragments/proposal_line_edit.html (new)
- static/js/inline-form.js
- locales/{en,fr,de,it}.yml
- tests/proposal_edit.rs (new)
- tests/e2e/specs/journeys/series-proposal-edit.spec.ts (new)
- _bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md
