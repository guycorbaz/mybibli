# Story 12.5: The title can be proposed

Status: ready-for-dev

GitHub issue: #563 (type:change-request) — closes it. Covers: FR152 (extends FR132). Epic 12 — fifth story (after 12.2); ships in v1.22.0.

## Story

As a librarian working with an agent,
I want the agent to be able to propose a correction of a title's main title,
so that spelling mistakes, and records that only carry their ISBN as a title, are fixed through the review workflow.

## Context (2026-10-30)

An agent asked to check the spelling of every title could not file one correction: `propose_title_metadata` refuses `title` ("cannot be proposed"). Epic 11 designed metadata proposals to *complete* a record.

## Interaction with Story 12.2 (decided here)

Story 12.2 records an **identity snapshot** (`title`, `isbn`, `media_type`) in every metadata/cover item; a title whose identity changed since filing outdates the proposal (`proposals::title_moved`). A title correction accepted through a proposal is a **sanctioned** change, not a re-identification: when a proposal writes `title`, the decision transaction **carries the new title forward** into the `identity.title` of every other pending item targeting that title (only where it still equals the old title). A title changed **by hand** keeps outdating pending proposals (conservative, as decided in 12.2's review).

## Acceptance Criteria

1. **AC1 — `title` is proposable.** `propose_title_metadata` accepts `title`: normal form like `subtitle` (single line, NFC, inner whitespace collapsed), non-blank, at most 500 characters (`titles.title VARCHAR(500)`); unchanged value dropped (existing rule); listed first in `PROPOSABLE_FIELDS`; the tool schema gains `title` (closed, described: "a correction of the main title — spelling, punctuation, capitalisation, or the real title of a record that only carries its ISBN; cite the source"); the registry test stays green.
2. **AC2 — never empty.** An empty/blank `title` is refused field by field (existing `FieldProblem` for blank / too long), English agent-facing message.
3. **AC3 — review.** The proposal detail shows a `title` line *Current / Proposed* with the *Changed* badge (`proposals.field_title`, 4 locales); the librarian can edit it (11-19 rules: blank refused) and remove the line like any metadata line.
4. **AC4 — accept.** Accepting writes `titles.title`, adds `title` to `manually_edited_fields` (a re-fetch never overwrites it — the mechanism already knows `title`), bumps the version once with the other fields; title page, search and lists show the new title.
5. **AC5 — stale.** A pending title proposal goes stale when the title was edited by hand meanwhile (`FieldChanged` on `title` *and* the identity snapshot).
6. **AC6 — carry forward.** Accepting a title correction does **not** outdate other pending metadata/cover proposals on that title: their `identity.title` is updated in the same transaction (only items whose `identity.title` equals the old title, pending proposals only).
7. **AC7 — resubmission.** `title` takes part in the per-field equivalence (12.2): a rejected title correction blocks another `title` proposal for the delay; other fields stay open.
8. **AC8 — tests.** Unit: validation (blank, too long, normal form), item shape, `field_changed` on `title`, edit of a `title` line. DB integration: file / accept (written, hand-edited, version) / stale after a hand edit / carry forward (another pending metadata proposal and a cover proposal stay acceptable after the title correction) / per-field rejection. E2E: an agent proposes a corrected title, the librarian accepts it, the title page shows it.

## Tasks / Subtasks

- [ ] T1 — `metadata_proposals.rs`: `title` in `PROPOSABLE_FIELDS` (first), `MAX_TITLE_CHARS = 500`, `ProposedFields::title`, `CurrentValues::title`, validation, `diff_against`
- [ ] T2 — `metadata_acceptance.rs`: `field_changed` live value, `ProposedFieldWrites::title` + the UPDATE (`models/title.rs`), manually-edited tracking, carry-forward (`ProposalModel::carry_identity_title`)
- [ ] T3 — `proposal_edit.rs` (edit a `title` line), `proposal_review.rs::field_label` + locale key `proposals.field_title` in en/fr/de/it
- [ ] T4 — MCP tool schema + description (`propose_title_metadata.rs`)
- [ ] T5 — tests (AC8), E2E
- [ ] T6 — manual ch. 13 (proposable fields list) EN + FR or note for 12.7; CLAUDE.md § Proposal lifecycle (carry forward)
- [ ] T7 — gates

## Dev Notes

- Mirror `subtitle` everywhere a proposable field is enumerated (`grep '"subtitle"'` in `src/services/{metadata_proposals,metadata_acceptance,proposal_edit,proposal_review}.rs`, `src/mcp/tools/propose_title_metadata.rs`, `models/title.rs::ProposedFieldWrites`).
- `titles.title` is `NOT NULL`: the write must never set it empty (validation guarantees it; assert in the write).
- Carry forward: `UPDATE proposal_items i JOIN proposals p ON p.id = i.proposal_id SET i.proposed_values = JSON_SET(i.proposed_values, '$.identity.title', ?) WHERE p.status = 'pending' AND p.deleted_at IS NULL AND i.deleted_at IS NULL AND p.id <> ? AND i.target_table = 'titles' AND i.target_id = ? AND JSON_UNQUOTE(JSON_EXTRACT(i.proposed_values, '$.identity.title')) = ?` — inside the decision transaction, after the title write. It takes row locks on other proposals' items after the title lock (same reverse order as 12.2's cover read; deadlocks retried by `in_retried_transaction`) — document it.
- Agent-facing English; web copy localized.
- Never `cargo fmt` the whole repository; heavy commands under `mem-guard`.

### References

- `_bmad-output/planning-artifacts/epics.md` § Story 12.5, FR152; issue #563
- Story 12.2 (identity snapshot, per-field staleness, equivalence); stories 11-13 (metadata tool), 11-18 (acceptance), 11-19 (edits)

## Dev Agent Record

### Agent Model Used

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created

### File List
