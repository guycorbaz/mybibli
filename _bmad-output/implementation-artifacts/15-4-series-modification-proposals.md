# Story 15.4: Series modification proposals

Status: review

GitHub issue: #590 (type:change-request). Covers: FR164, UX-DR41 (series part). Epic 15; ships in v1.25.0.

## Story

As a librarian,
I want the agent to propose corrections to a series it knows,
so that finished series are closed at the right total without me checking each one.

## Decisions (project lead, 2026-11-04 — #590)

A total lower than an owned position or a live expected tome is **refused and explained** — at filing; stale at acceptance if the catalogue changed meanwhile — naming the positions in the way. The agent first proposes removing the surplus expected tomes (in the same proposal). Series kinds keep `version`-based staleness.

Cases from the agent session (#590): Tintin, Cubitus, Bernard Prince, Survivants, Wayne Shelton to close at 24 / 39 / 18 / 5 / 14; Neptune declared closed at 3, finished at 2; Jeremiah finished at 42.

## Design decisions (dev, 2026-11-05 — recorded for the review)

- **D1 — removal lines are untargeted.** `ProposalTargetTable` has no expected-tome table (`target_table ENUM('titles','series','volumes')`); adding one means a migration plus every match on the table. A removal line carries `tome_id` and the tome `version` read at filing in its JSON, and acceptance re-reads the tome under the series lock: gone or version changed → stale (`AcceptConflict::TomeChanged`, new).
- **D2 — "accepted fields count as hand-set" is vacuous for series.** `manually_edited_fields` exists on `titles` only; nothing automatic (provider, re-download) ever writes `series.name` / `series_type` / `total_volume_count`. No column is added; the story file and the CLAUDE.md entry say so. Revisit only if a provider starts writing series.
- **D3 — one pending proposal per series, any kind** (existing `pending_targeting(Series, id)` rule kept). A pending `series_attach` blocks a `series_update` on the same series and vice versa; the refusal names the pending proposal id. Keeps the "one thing at a time per series" model and avoids an accepted update making a pending attach stale behind the librarian's back.
- **D4 — equivalence key = the exact change.** `series_update:series/<id>:<canonical change>` where the change is the sorted, normalized set of proposed fields (`name=<folded>`, `type=…`, `total=…`) and removed tome ids, hashed (SHA-256, the key column is 700 chars). A rejected rename does not block a later "close at 24". *Amended in dev:* the key is **kept from the agent's filing**, like the other series kinds (`proposals.rs` rejection comment: an edited value must still block the agent's own) — not recomputed from the live lines.
- **D5 — the stricter total rule applies to proposals only.** The hand edit (`SeriesService::update_series`, `total ≥ COUNT(owned titles)`) is unchanged in this story; tighten it later if wanted (out of scope — say so in the completion notes, no silent change).

## Starting point (from the code map, 2026-11-05)

- Kinds: `ProposalKind` `src/models/proposal.rs:24-56` (+ `ALL`); DB `proposals.kind ENUM(...)` last changed in `migrations/20261104090000_proposals_label_attach.sql:13` → new MODIFY migration, existing values first, `series_update` appended.
- Every kind match to extend: `services/proposals.rs` `version_counts` :630 (series_update → **true**), rejection key recompute :988; `services/acceptance.rs` dispatch :362-379 (+ `Applied`); `services/proposal_accept.rs` `is_series` :23, `modal_sentences` :31, `AcceptSummary` :120, `applied_messages` :166, `applied_entry` :303, `stale_why_of` :380; `services/proposal_history.rs` `ProposalSubject` :170 / `from_lines` :200 / `equivalence_key` :278 / `describe` :330; `services/proposal_review.rs` `kind_label` :72, `TargetLabel::of` :126, `parse_line` :542, `can_edit` :784; `services/proposal_edit.rs` `editable` :363, `keep_of` :374, `removable` :389, `keep_reason` :395, `edit_line_in` :816, `has_lines_to_apply` :936; `routes/proposals.rs` :92 (view_series link).
- Filing model to follow: `services/series_proposals.rs::propose_attach` :956 + `file_unique` :1037 (lock filing key → duplicate reads → `ProposalService::file_in` → `log_filed` after commit); validation helpers `validate_series_name` :585, `validate_total` :598; error enum + `impl Refusal` :156-273. MCP tool pattern: `src/mcp/tools/propose_series_attach.rs` (closed `Args`, `deny_unknown_fields`, `series_version` pin, `sources`), helpers in `proposal_args.rs` / `series_proposal_args.rs`.
- Acceptance model: `services/series_acceptance.rs` (lock_pending_in → parse → `ExpectedTomeModel::lock_series_in` FOR UPDATE → `check_staleness_in` → live re-check → `transition_in` → apply); `AcceptConflict` `services/acceptance.rs:47` (has `SeriesExists`, `BeyondTotal`, …).
- Series data: table `series` (`name`, `series_type ENUM('open','closed')`, `total_volume_count INT NULL`, quartet). `SeriesModel::update_with_locking` `src/models/series.rs:238` is pool-only → add `update_in(conn, …)` (version-guarded, bumps version). Held positions: `TitleSeriesModel::held_positions_in` :816 (omnibus ranges via `end_position`). Tomes: `ExpectedTomeModel::live_slots_in` `src/models/expected_tome.rs:180`, `soft_delete_in` :371; lock order **series row → tome rows** (`ExpectedTomeService::lock_tome` :629).
- Review: `templates/fragments/proposal_detail_body.html` (current/proposed table :88-128 to reuse, edit/remove buttons :194-215), `templates/fragments/proposal_line_edit.html` (`series_new` inputs :11 to reuse for name/type/total).
- Lock-scope test model: `tests/mcp_label_proposals.rs:570` (tx1 held open after `accept_in`, tx2 with `innodb_lock_wait_timeout = 2`).
- **File sizes (Rule #12):** `proposal_edit.rs` 1723, `proposals.rs` 1616, `series_proposals.rs` 1613, `proposal_review.rs` 1613, `models/proposal.rs` 1543, `locales/en.yml` 1590 lines. New logic goes in **new modules** (`services/series_update_proposals.rs`, `services/series_update_acceptance.rs`, and a `series_update` helper module for review/edit if `proposal_edit.rs` / `proposal_review.rs` would pass ~1850); the existing files only gain dispatch arms.

## Acceptance Criteria

1. **AC1 — kind and shapes.** New kind `series_update` (enum + migration). Items: one **update line** `{"line":"series_update", "name"?, "series_type"?, "total_volume_count"?, "current": {"name","series_type","total_volume_count"}, "fingerprint"}` targeting `series/<id>` with `expected_version`; zero or more **removal lines** `{"line":"tome_removal","tome_id","tome_version","position","title"}` (untargeted, D1). Shapes documented in the new module's doc comment. A proposal has the update line always (even when it only removes tomes: then no field is proposed).
2. **AC2 — MCP tool `propose_series_update`.** Args (closed schema): `series_id`, `series_version?` (pin), `name?`, `series_type?` (`open`/`closed`), `total_volume_count?`, `remove_expected_tomes?: [tome_id]`, `sources` (required, propose-tool rules). Description contains "does not change the catalog" and `` `sources` ``; registered in `registry()`, `PROPOSE_TOOLS` (src and `tests/common/mcp.rs`), `ALL_TOOLS`, `MCP_TOOLS` (`tests/e2e/helpers/mcp.ts`); `get_series` description mentions it next to `propose_series_attach`; `docs/route-role-matrix.md` tool list updated. Answer = `filed_document`.
3. **AC3 — filing refusals** (English agent messages, `Refusal` classes, nothing written): unknown/deleted series (NotFound); `series_version` pin ≠ live (TargetChanged); invalid name (1..=255 trimmed) or type/total (`validate_total`: open → no total, closed → 1..=10 000; `series_type: closed` without a total and no current total → refused; a total alone on an open series → refused, say "set series_type closed"); **no change** (every proposed field equals the current value and no removal); a removal id that is not a live expected tome **of this series**, or listed twice; rename to the name of another live series; **total rule**: a closed total lower than any owned position (omnibus end included) or than the position of a live expected tome **not removed by the same proposal** → refused, the message lists the positions in the way (owned and expected separately) and tells the agent to propose removing the surplus expected tomes; duplicate pending proposal on the series (D3, names its id); equivalent rejection within the resubmission delay (D4).
4. **AC4 — acceptance.** `POST /proposals/{id}/accept` dispatches to `series_update_acceptance::accept_in` (pub, for the lock-scope test). Order: proposal → items → **series FOR UPDATE** → **removed tome rows FOR UPDATE** (by id, after the series) → `check_staleness_in` (series `version` → stale) → live re-check: rename collision (`SeriesExists`), each removal tome gone / version changed / moved to another series (`TomeChanged`, new), total rule against the live held positions and live tomes minus the removed ones (`BelowPosition { positions }`, new — stale with the positions named, not a 409) → `transition_in(Accept)` → apply in the same transaction: `SeriesModel::update_in` (only the proposed fields; open clears the total; bumps `version`), `ExpectedTomeModel::soft_delete_in` for each removal. Deadlock retry as the other kinds (`in_retried_transaction`). D2 documented. Applied feedback names what changed ("Series renamed / closed at 24 / 2 expected tomes removed").
5. **AC5 — review screen (UX-DR41).** Detail shows a current-vs-proposed table for name / type / total (only proposed fields, unchanged ones omitted) and a list "Expected tomes removed" (position — title). The update line is editable (name / type / total inputs as `series_new`), not removable; removal lines are removable; edits and removals pass the filing rules (total rule, no change) and refuse with the localized reason. Accept modal sentence for the kind; kind label and filter option `proposals.kind_series_update`; all new keys in **en/fr/de/it** (`tests/locale_parity.rs`).
6. **AC6 — history.** `ProposalSubject` / `describe` / `list_my_proposals` show the kind; rejection blocks the exact change (D4) for `proposal_resubmission_delay_days`.
7. **AC7 — lock scope (epic 12 retro A1).** Locking reads start from the target index or PK lists. Regression test `an_update_acceptance_on_one_series_does_not_lock_proposals_of_another` (model `tests/mcp_label_proposals.rs:570`): tx1 holds an accepted `series_update` on series A; tx2 accepts / rejects a proposal on series B within a 2 s lock wait.
8. **AC8 — tests.** Unit: validation, total rule (owned omnibus range, tome removed by the same proposal frees its position, open type), equivalence key canonical form, review line parsing. DB integration (`tests/mcp_series_update_proposals.rs` for filing via MCP, `tests/proposal_accept_series_update.rs` for acceptance): each refusal writes nothing; accepted update changes the series and removes the tomes atomically; stale on version change, on a tome changed, on a position owned meanwhile above the new total; failure while applying rolls back; CSRF + role. E2E (`tests/e2e/specs/journeys/series-update-proposal.spec.ts`, own spec id): an agent (MCP over HTTP, propose key) proposes closing a series at a new total with one expected tome removed; the librarian (real `loginAs`) reviews current vs proposed, accepts; the series page shows "closed", the new total and no longer the removed tome. Full suites green; flake gate.
9. **AC9 — docs.** CLAUDE.md "Proposal lifecycle" gains a `series_update` paragraph (shapes, D1–D4, lock order). Manual chapters are story 15-8.

## Tasks

- [x] Migration: `proposals.kind` + `series_update`; `ProposalKind` variant; `cargo sqlx prepare`.
- [x] `SeriesModel::update_in`; tome lookup by ids of one series (locking + non-locking).
- [x] `services/series_update_proposals.rs`: request, validation, total rule (pure fn, unit-tested), errors + `Refusal`, filing via `file_unique`-shaped transaction.
- [x] MCP tool `src/mcp/tools/propose_series_update.rs` + registries + test helpers + docs list.
- [x] `services/series_update_acceptance.rs` + dispatch + new `AcceptConflict` variants + librarian copy.
- [x] Review / edit / history arms (new helper module if size requires).
- [x] Locales en/fr/de/it.
- [x] Tests (unit, DB integration, lock scope, E2E).
- [x] CLAUDE.md.

## Dev Agent Record

### Completion Notes

- **Shapes and filing** (`services/series_update_proposals.rs`): update line + untargeted removal lines (D1); only changed fields are stored; `resolve_change` drops values equal to the current ones; `TotalNeedsClosed` when a total is given for an open series without `series_type: closed`. `note` was dropped from the design: `sources` carry the agent's reasons (no second free-text field to validate). Errors: `SeriesUpdateError` wraps `SeriesProposalError` for the shared rules (name, total range, unknown series, version pin, pending duplicate) — no duplicated messages; `SeriesProposalService::file_unique` / `Duplicate` are reused (`pub(crate)`). The name collision uses `ProposalModel::active_series_named` (case/accent-insensitive) at filing, edit and acceptance.
- **Total rule** (`positions_in_the_way`, `check_total_rule`, pure): applies only when the change touches type or total and the series ends closed — a rename of a series already in a #524 state stays possible. Owned positions above the total are listed apart from expected tomes (an owned position is not repeated as expected).
- **Acceptance** (`series_update_acceptance.rs`): stored change re-validated (`resolve_change` must give back the same change) → series `FOR UPDATE` → `check_staleness_in` (version) → removed tomes `FOR UPDATE` by id (`TomeChanged`) → rename collision (`SeriesExists`) → total rule on live data (`BelowPosition`) → `transition_in` → `SeriesModel::apply_update_in` (bumps `version`, keeps the description) + `soft_delete_in` per removal.
- **Lock scope:** proposals and series rows are locked by PK / target index; the regression test accepts an update on series B and rejects a proposal on series C while A's acceptance is open. Found while writing it: the share-locked read of a series' tomes (`live_slots_in`, also used by the series_attach acceptance and the line edit) next-key locks the first tome entry of the next series in `uq_expected_tomes_position`, so a tome **write** on the adjacent series waits for the commit (a wait, no cycle). Kept (correctness of the live re-check needs the lock) and documented in the module doc, CLAUDE.md and the test; to raise at review as a possible follow-up issue for both series kinds.
- **Review / edit:** `series_update` line → current-vs-proposed table (`data-series-update`), `tome_removal` → "Expected tomes removed"; the edit form reuses the `series_new` inputs; a removal line can be removed only if what remains still passes the rules (`series_update_edit::check_removal_in`), the update line never.
- **D2:** nothing marked hand-set (no automatic writer of series fields). **D5:** hand edit unchanged (count rule).
- Tests: unit (14 in the new modules + fingerprint pin + version_counts), DB integration `tests/proposal_series_update.rs` (7), existing proposal / MCP suites green (one assertion in `mcp_series_proposals.rs` narrowed: the update tool has no `titles`), E2E `series-update-proposal.spec.ts` (spec id SU) green with the series proposal specs at `CI=true --workers=2`. Full suites: run once on the final code, before the push.

### File List

- `migrations/20261105120000_proposals_series_update.sql` (new)
- `src/services/series_update_proposals.rs`, `src/services/series_update_acceptance.rs`, `src/services/series_update_edit.rs`, `src/mcp/tools/propose_series_update.rs` (new)
- `src/models/proposal.rs`, `src/models/series.rs`, `src/services/{mod,acceptance,proposals,proposal_history,proposal_review,proposal_edit,proposal_accept,series_proposals}.rs`, `src/routes/proposals.rs`, `src/mcp/tools/{mod,get_series}.rs`
- `templates/fragments/proposal_detail_body.html`, `templates/fragments/proposal_line_edit.html`
- `locales/{en,fr,de,it}.yml`
- `tests/proposal_series_update.rs` (new), `tests/mcp_series_proposals.rs`, `tests/common/mcp.rs`, `tests/e2e/helpers/mcp.ts`, `tests/e2e/specs/journeys/series-update-proposal.spec.ts` (new)
- `docs/route-role-matrix.md`, `CLAUDE.md`

### Review Findings (2026-11-05 — Blind Hunter, Edge Case Hunter, Acceptance Auditor)

No High; three Medium, fixed in this story.

- [x] [Review][Patch] (Medium) `resolve_change` re-validated the current total when the agent touched neither type nor total: a closed series with a hand-set total above 10 000 (or none) could not be renamed or lose tomes — at filing, at the acceptance re-validation and in the line edit [series_update_proposals.rs, series_update_acceptance.rs, series_update_edit.rs] — type/total left alone are no longer validated (unit tests).
- [x] [Review][Patch] (Medium) AC8 tests missing: rollback on a failed apply, CSRF + role (accept and line edit), review line parsing, success / stale messages, `positions_text` [tests/proposal_series_update.rs, proposal_review.rs, proposal_accept.rs].
- [x] [Review][Patch] (Medium) Lock-scope wording overstated ("never waits") and no issue for the neighbour wait (Rule 11) — wording corrected; #627 filed (affects series_attach too).
- [x] [Review][Patch] (Low) A case-only rename missed another series of that name (`active_series_named` returned the series itself) — `ProposalModel::other_series_named` (except self) at filing, edit, acceptance.
- [x] [Review][Patch] (Low) A rename to the name of a pending creation was filed — refused (`PendingDuplicate`, series name).
- [x] [Review][Patch] (Low) A name equal up to Unicode normalization counted as a change — NFC comparison.
- [x] [Review][Patch] (Low) A settled (owned) tome could be listed for removal — refused (`TomeOwned`).
- [x] [Review][Patch] (Low) Dead-end Remove button on the single removal of a removals-only proposal — not offered; empty trail summary of such an update line — "Name, type and total stay as they are."
- [x] [Review][Patch] (Low) Position lists uncapped — at most 20 named, then "… (+N)".
- [x] [Review][Patch] (Low) The applied feedback did not say the series was renamed (`renamed` unused) — "Series renamed to …"; `type_total` reduced to the total.
- [x] [Review][Patch] (Low) Soft-delete row count ignored at apply — anything but 1 row rolls back.
- [x] [Review][Patch] (Low) `list_my_proposals` description ignored the new kind (subject, equivalence) — updated; CLAUDE.md "recomputed at rejection" now says metadata only.
- [x] [Review][Patch] (Low) Misleading comment on the unreachable "series gone" branch; wrong hint key for `TotalNeedsClosed`; duplicated open/closed total mapping (`SeriesFields::from_shape`) and position join (`positions_list`).
- [x] [Review][Patch] (Low) Weak test assertions on bare numbers — full localized phrases.

Deviations recorded (Low, no change): acceptance order is series → staleness → removed tomes (the series → tomes lock order holds); tests live in one file `tests/proposal_series_update.rs`; `SeriesModel::apply_update_in` relies on the row lock (no version filter); removed tomes are locked one `find_in` per id; removed tomes render one line each with the heading as prefix. **Total rule exception** (rename / removals only skip it) — to confirm by the project lead.

Dismissed: omnibus positions diverging at filing (one `title_series` row per position, verified); `find_in` returning trashed tomes (it filters `deleted_at`); stale copy "title" wording (`stale_feedback_title` says "nothing was changed"); fingerprint including removals (D4: the exact change is blocked); a locking name read at acceptance (a `FOR UPDATE` through the collation function would scan and lock every series; window narrow, same as the edit path); E2E `getByText(CLOSED)` strictness (green; the series page shows it once).

### Review pass 2 (2026-11-05)

One Medium, fixed; five Low, fixed.

- [x] [Review][Patch] (Medium) `ProposalModel::subject_lines` skipped the `series_update` line: no subject on `/proposals` and `subject: null` in `list_my_proposals` [src/models/proposal.rs] — line added; DB test `lists_show_the_series_of_a_change`.
- [x] [Review][Patch] (Low) A type / total repeated as they are (or a closed series without total in the edit form) was still validated — `resolve_change` treats values equal to the current ones as left alone (the edit's special case removed).
- [x] [Review][Patch] (Low) The edit form's `max="10000"` blocked a rename of a series with a larger hand-set total in the browser — not set on the update line (the server validates a changed total).
- [x] [Review][Patch] (Low) Doc comment of `lock_series_named` displaced by `other_series_named` — restored.
- [x] [Review][Patch] (Low) The tool description did not list the owned-tome refusal — added.

