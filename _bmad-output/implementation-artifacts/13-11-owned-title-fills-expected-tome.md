# Story 13.11: An owned title fills an expected tome by proposal

Status: review

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

- [x] T1 — rule change in `check_against_series` (+ new refusal variant, message, mapping)
- [x] T2 — acceptance: attach + settle in the transaction (reuse `expected_tomes` settle helpers on the caller's connection)
- [x] T3 — edit path
- [x] T4 — docs (module docs, CLAUDE.md, tool description)
- [x] T5 — tests; gates; full suites once before the single push

## Completion Notes

- **Rule (T1).** `series_proposals::check_against_series` takes a new `title_isbns: &HashMap<u64, String>` (ISBN-13 of each title line's title, built by the new `title_isbns_in(conn, lines)` over the new `ExpectedTomeModel::live_title_isbns_in`; both sides normalised with `expected_tomes::normalize_isbn`, so an ISBN-10 spelling is the same book; an unparsable stored title ISBN counts as unknown). A title line covering a live tome is now accepted unless both ISBNs are known and differ → new `SeriesProposalError::TomeIsbnMismatch` (class `Conflict` → `ToolError::Refused`, English agent message). Omnibus lines check every tome of their range. Tome lines on a tome's position are still refused (`TomePositionTaken`). `TomeSlot` gained the tome `id`.
- **Acceptance (T2).** `series_acceptance::accept_in` re-runs the rule on the live state (share-locked tomes under the series lock); a mismatch maps to the new `AcceptConflict::TomeIsbnMismatch` (stale, new copy `proposals.stale_why_tome_isbn_mismatch`); a position taken meanwhile stays `PositionHeld` (stale). After `assign_in` / `assign_omnibus_in` (which clear `detected_tome`, 13-10), each filled tome (`filled_tomes`, pure) is settled with `ExpectedTomeModel::mark_settled_in` in the same transaction. Lock order unchanged: proposal → items → series → targets/tomes; no status-range scan. `AppliedSeries.tomes_filled` is reported in the success entry (`proposals.applied_filled`). Existing pending proposals whose lines touch no tome position apply exactly as before.
- **Edit (T3).** `proposal_edit` passes the title ISBNs to the same check; new localized copy `proposals.edit_error_tome_isbn_mismatch`.
- **Docs (T4).** Module docs of `series_proposals.rs` (new section superseding D-11-12-10) and `series_acceptance.rs`; CLAUDE.md "Expected tomes" and "Proposal lifecycle"; `propose_series_attach` description and the `titles` schema description (registry wording kept).
- **Tests (T5).** Unit: `series_proposals` (no ISBN, same ISBN incl. ISBN-10 spelling, different ISBN, omnibus over two tomes, tome line still refused), `series_acceptance::conflicts_are_named_from_the_catalog`, `proposal_accept::every_outcome_is_localized`, `proposal_edit::errors_are_localized_in_every_locale`. DB: `tests/proposal_accept_series.rs` (+4: file + accept fills four tomes incl. omnibus and series page shows none expected; position taken → stale; tome ISBN changed → stale; mismatch refused at filing), `tests/mcp_series_proposals.rs` (refusal cases updated), `tests/proposal_edit.rs` (+1). E2E: third journey in `series-proposal-accept.spec.ts` (specId SA, seq 3). Gates run: clippy --all-targets -D warnings (clean); lib unit tests of the touched modules; DB tests proposal_accept_series, mcp_series_proposals, proposal_edit, expected_tomes, locale_parity (all green); Playwright series-proposal-accept, series-proposal-edit, mcp-series-proposals, series-expected-tomes at --workers=2 (11 passed); tsc --noEmit clean; no waitForTimeout.
- New locale keys in en/fr/de/it: `proposals.applied_filled`, `proposals.stale_why_tome_isbn_mismatch`, `proposals.edit_error_tome_isbn_mismatch`.
- Files touched: `src/models/expected_tome.rs`, `src/services/{series_proposals,series_acceptance,acceptance,proposal_accept,proposal_edit}.rs`, `src/mcp/tools/{propose_series_attach,series_proposal_args}.rs`, `locales/*.yml`, `CLAUDE.md`, `tests/{proposal_accept_series,mcp_series_proposals,proposal_edit}.rs`, `tests/e2e/specs/journeys/series-proposal-accept.spec.ts`. No file above 2000 lines (largest touched: `proposal_edit.rs` 1705). `proposal_edit.rs` was not rustfmt-clean before, so it was left unformatted. No `sqlx::query!` macro changed (no `.sqlx/` update).
- Deviation: none of substance; the success entry's filled-tome count (`applied_filled`) is an addition for the librarian's feedback.

### Review Findings

Round 1 (of 2031d47) — 1 Medium, 3 Low, all fixed:

- [x] **M1** — an omnibus edition carries its own ISBN, never a single tome's, so an omnibus line over tomes with ISBNs was always refused. Decision (orchestrator, spirit of #588): `check_against_series` compares ISBNs for single-position title lines only; an omnibus line fills every tome of its range with no comparison (the librarian judges with L2). Filing, acceptance and edit share the check, so all three follow. Module docs, CLAUDE.md, the `propose_series_attach` description and the `titles` schema description updated. Tests: unit (`an_omnibus_fills_every_tome_in_its_range`: own ISBN accepted, single line of another ISBN refused), DB `an_omnibus_with_its_own_isbn_fills_every_tome_of_its_range`; the omnibus refusal case removed from `mcp_series_proposals::attach_refusals_write_nothing`.
- [x] **L1** — `ExpectedTomeModel::live_title_isbns_in` now reads with `LOCK IN SHARE MODE` (title rows by primary key, after the series and tome locks: series → tomes → titles); a plain read on filing's autocommit connection. Doc comments fixed (`expected_tome.rs`, `title_isbns_in`, `series_acceptance` module doc).
- [x] **L2** — the review screen of a pending `series_attach` names the expected tomes each title line fills ("Fills the expected tome 5 “Title” (ISBN …)." / "(no ISBN)."), computed from the live tomes (`proposal_review::with_fills` / pure `fills_of`, `routes::proposals::fill_label`, `data-fills-tome` paragraph, Askama-escaped; keys `proposals.fills_tome` / `fills_tome_no_isbn` in en/fr/de/it). Tests: unit `a_title_line_names_the_tomes_it_fills`, `fill_labels_name_each_tome_in_every_locale`; DB render in `an_omnibus_with_its_own_isbn_fills_every_tome_of_its_range` (shown while pending, gone once decided).
- [x] **L3** — DB test `a_pending_attach_touching_no_expected_position_applies_unchanged` (item shapes are unchanged, so a proposal filed before 13-11 is identical to one filed now): title + tome lines on free positions of a series expecting another tome apply as before, nothing filled, no fill notice.
