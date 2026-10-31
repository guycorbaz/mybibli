# Story 12.7: Documentation

Status: done

Covers: FR148–FR152 (documentation). Epic 12 — seventh story; ships in v1.22.0.

## Story

As an operator,
I want the manual to describe the new behaviour,
so that the proposal workflow is documented as it now works.

## Acceptance Criteria

1. Chapter 13 (EN + FR) describes per-field staleness, what the agent is told about providers, title corrections and the new list — **done in the stories themselves** (12.2, 12.3, 12.5, 12.4 each updated its section and rebuilt the PDFs); re-read here for consistency.
2. The scan chapter mentions typed ISBNs with separators (12.1) — done here (chapter 3, scan field; EN + FR).
3. PDFs rebuilt.
4. `CLAUDE.md` reflects the new rules — staleness/equivalence/carry-forward (12.2, 12.5, done there), the MCP instructions builder (12.3, added here).
5. Release documentation (Foundation Rule #19, v1.22.0, replacing the v1.21.1 wording already on `main`) is done at release time, not in this story. Release-note items to carry over (from stories 12-2 and 12-5): proposals pending at the upgrade keep the old staleness rule (re-file them to benefit); pre-upgrade metadata rejections now block per field; pre-upgrade cover rejections block nothing new; a title correction accepted from a proposal keeps the other proposals valid, a hand edit outdates them; cover URLs now carry a cache-buster on upload and fetch; the 12-6 CI deadlock fix.

## Dev Agent Record

### Agent Model Used

claude-opus-5-5

### Completion Notes List

- Chapter 3 (EN + FR): hyphenated / spaced ISBN entry, at scan and in search.
- Chapter 13 re-read: the sections written by 12.2 / 12.3 / 12.4 / 12.5 are consistent (staleness rule and its exceptions, providers told to the agent with the stop rule, title among the proposable fields and in the tool reference, the list row).
- `CLAUDE.md` § MCP layer: the instructions builder.
- PDFs rebuilt under `mem-guard`. Docs only — no code change.

### File List

- `docs/manual/{en,fr}/03-usage.tex`, `docs/manual/mybibli-manual-{en,fr}.pdf`
- `CLAUDE.md`
- `_bmad-output/implementation-artifacts/12-7-epic-12-documentation.md` (new)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — 12-7 line
