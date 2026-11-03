# Story 13.11: An owned title fills an expected tome by proposal

Status: ready-for-dev

GitHub issue: #588 (type:change-request). Covers: FR159. Epic 13 (added by the project lead on 2026-11-03); ships in v1.23.0. After Story 13.10 (both touch series positions).

## Story

As a librarian working with an agent,
I want the agent to propose attaching an owned title at a position the series already expects,
so that declaring expected tomes does not lock the series against its owned titles.

## Evidence (agent session, 2026-11-03)

About 51 `propose_series_attach` lines refused with "The series already expects the tome "…" at position N" (Astérix, Tintin, Gaston, Spirou, Durango, XIII Mystery, Wunderwaffen, Thorgal, Buck Danny — list in the agent's synthesis, quoted in #588).

## Existing-code reality check (code map, 2026-11-03)

| Place | Today |
|---|---|
| `src/services/series_proposals.rs::check_against_series` (~l. 740–790) | a title line whose position range covers an unsettled expected tome → `TomePositionTaken` — **decision D-11-12-10** ("an unsettled one names a gap a title line must not fill silently") |
| `src/services/series_acceptance.rs` (~l. 219, 694) | the same check re-run at acceptance → `AcceptConflict::TomePositionTaken` → stale/refused |
| `src/services/proposal_edit.rs` (~l. 287, 1580) | the same refusal when the librarian edits a line |
| `src/services/expected_tomes.rs` | settlement: `settle_for_title`, `settle_matching`, `settle_with_owned_title`, `settle_one` (~l. 651–790); a tome is settled when `settled_title_id` is live and holds the tome's position; lock order series → tomes |
| CLAUDE.md "Expected tomes" | settling **attaches** only on catalogue events; hand assignments **record** a settlement the series already shows |

## Acceptance Criteria

1. **AC1 — filing.** A title line at a position covered by an unsettled expected tome is accepted at filing when the tome has no ISBN, or its ISBN equals the title's ISBN (ISBN-13 comparison; title without ISBN + tome with ISBN → accepted, the librarian judges); refused with a new explicit English message when both ISBNs are known and differ (`TomeIsbnMismatch`, mapped to `ToolError::Refused`). An omnibus line (range) follows the same rule for every tome in its range.
2. **AC2 — acceptance settles.** Accepting such a proposal attaches the title at its position(s) and settles the expected tome(s) by it, inside the acceptance transaction, respecting the lock order (proposal → items → series → tomes → titles) and the series staleness rules (version + live state, #521).
3. **AC3 — staleness.** If the position became held by another title meanwhile, or the tome's ISBN changed to a different one, acceptance marks the proposal stale (existing reason copy or a new one).
4. **AC4 — edit.** The librarian's line edit (11-19) applies the same rule (AC1) instead of the old refusal.
5. **AC5 — decision record.** D-11-12-10 is superseded: update the module docs of `series_proposals.rs` and the CLAUDE.md "Expected tomes" / "Proposal lifecycle" notes accordingly; the `propose_series_attach` description says an owned title may fill an expected tome (keep the registry-test wording requirements).
6. **AC6 — tests.** Unit: `check_against_series` cases (no ISBN, same ISBN, different ISBN, omnibus over two tomes). DB integration (`tests/proposal_accept_series.rs` / `tests/mcp_series_proposals.rs`): file + accept settles the tome (series page shows it owned, `settled_title_id` set); ISBN mismatch refused at filing; position taken meanwhile → stale. E2E (`series-proposal-accept.spec.ts` or new specId `FT`): an agent proposes an owned title at an expected position; the librarian accepts; the series shows the tome as owned.

## Tasks / Subtasks

- [ ] T1 — rule change in `check_against_series` (+ new refusal variant, message, mapping)
- [ ] T2 — acceptance: attach + settle in the transaction (reuse `expected_tomes` settle helpers on the caller's connection)
- [ ] T3 — edit path
- [ ] T4 — docs (module docs, CLAUDE.md, tool description)
- [ ] T5 — tests; gates; full suites once before the single push
