# Story 20.7: Documentation (epic 20)

Status: ready-for-dev

No GitHub issue of its own (documents #714, #715, #716, #524, #700). Covers FR183–FR185 (documentation). Epic 20; ships in v1.29.0 (with Epic 19). Sprint-status key: `20-7-epic-20-documentation`.

**Shipped with story 19-11 in one change** (branch `story/19-11-20-7-documentation`, one PR): both stories edit `docs/manual/{en,fr}/*.tex`, and the PDFs are rebuilt once for both (epics.md, Epic 20 couplings, "Manual and PDFs" — "or the two are done as one story"). Story file of the other half: `19-11-epic-19-documentation.md`.

## Story

As a librarian reading the manual,
I want it to describe the new actions,
so that I find them without asking.

## Context

What epic 20 changed for the reader (stories 20-1 … 20-5, all merged), and what the manual says today:

- **20-1 — "Remove" next to "Bought"** on the wish list (row, mobile card, entry page), each with its own modal; neither can be undone (`wishlist_items` is not in the Trash). The manual (chapter 3, "Marking a book as bought") still says the entry "soft-deletes. The action is reversible from the Admin Trash panel for 30 days" — wrong since 20-1 D2. The scan banner keeps the "bought" modal.
- **20-2 — a gap sent to the wish list.** On a plain gap of the series grid (Librarian+), "Add to wish list" opens the expected-tome form in wish-list mode ("Add tome N to the wish list", title pre-filled "{series}, tome {position}", help line); saving records the expected tome **and** the wish list entry in one go, back on the series page with the success entry and its link. An ISBN already in the catalogue, or an entry already on the wish list, writes nothing. Chapter 3 (wish list) and chapter 13 (`sec:expected-tomes`) describe only the one-click "Add to wish list" of an existing expected tome.
- **20-3 — the total rule and the restore settlement.** The series edit refuses a closed total below a live, unsettled expected tome, next to the total field ("Total must be at least N: an expected tome is recorded at position N."); the other series-form refusals now show next to their fields too. Restoring a title from the Trash onto a position an expected tome took meanwhile: the tome is settled when it carries the title's ISBN (line "The expected tome at position N of “S” now counts as owned."), otherwise an information line with "Open the series" points to "Expected tomes to review". Chapter 13's "Expected tomes to review" bullet still says a lowered total puts a tome there.
- **20-4 — the location description.** Edit form only (`Description`, help line, 500 characters, plain text, blank = none); shown on `/location/{id}` only (not in the tree, not searched); `GET /api/v1/locations` returns `description` (string or `null`); MCP unchanged; the edit page's back link "Back to locations".
- **20-5 — the tree as a nested list.** Disclosure buttons ("Locations inside {name}", `aria-expanded`), plain Tab order (button → link → add → edit → delete), Enter / Space toggle, collapsed state remembered per browser, the parent updated at once after its last child is deleted. Chapter 2 ("Editing the tree") says "nested list with inline create / rename / move / delete affordances" — still true; the keyboard part is missing.

Where the sections live: the wish list and the series are in chapter 3 (`03-usage.tex`), expected tomes in chapter 13 (`13-agents-proposals.tex`, `sec:expected-tomes`), storage locations in chapter 2 (`02-configuration.tex`, `sec:locations`). The epic AC names `03-usage.tex` for all four; the location parts go where the locations are (chapter 2) and chapter 13's expected-tome bullets are corrected — see Validation.

## Acceptance Criteria

1. **AC1 — wish list.** Chapter 3 describes "Remove" versus "Bought" (where each appears, the two modals, neither restorable, the scan banner opens "bought"), and sending a gap to the wish list (form, pre-filled title, what is recorded, refusals).
2. **AC2 — total rule.** The manual says a closed series' total cannot go below a recorded expected tome (refusal next to the field), and what a Trash restore does to an expected tome on the title's position.
3. **AC3 — location description.** The manual describes the description field (edit form, 500 characters, where it shows, where it does not) and the tree's disclosure buttons and keyboard use.
4. **AC4 — API.** `11-api-integrations.tex` documents `description` in `GET /api/v1/locations` (string or `null`), with the fields the endpoint returns.
5. **AC5 — roles.** `06-roles-permissions.tex` is checked for the new actions (Librarian+): wish list rows (remove / mark bought, send a gap), and — for 19-11 — backup / restore / cover check (Admin).
6. **AC6 — CLAUDE.md.** Key Patterns: the "Expected tomes" bullet gains the hand-edit total rule and the restore settlement; a short note describes the location tree as a disclosure list (no ARIA tree).
7. **AC7 — French.** Same content in careful French, the French UI labels word for word ("wish list" kept, as in the UI).
8. **AC8 — PDFs.** Rebuilt once, together with 19-11.
9. **AC9 — not in this story.** Release documentation (Rule #19, v1.29.0).

## Tasks / Subtasks

- [ ] T1 — Chapter 3 EN/FR: wish list (Remove / Bought, a gap sent to the wish list); series (the total rule, the restore) (AC1, AC2).
- [ ] T2 — Chapter 13 EN/FR: `sec:expected-tomes` bullets corrected (gap → wish list, "to review" reasons) (AC1, AC2).
- [ ] T3 — Chapter 2 EN/FR: location description, disclosure tree (AC3).
- [ ] T4 — Chapter 11 EN/FR: `/api/v1/locations` fields (AC4).
- [ ] T5 — Chapter 6 EN/FR: rows for the new actions (AC5).
- [ ] T6 — `CLAUDE.md` Key Patterns (AC6).
- [ ] T7 — PDFs with 19-11 (AC8).

## Dev Notes

- Copy from `locales/en.yml` / `locales/fr.yml` (`wishlist.*`, `expected_tome.*`, `series.total_below_expected`, `admin.trash.restore_tome_*`, `location.description_*`, `location.back_to_tree`, `location.tree.toggle_named`).
- Do not touch version numbers, "What's new" chapters or the "Current version" line.

### References

- `_bmad-output/planning-artifacts/epics.md` (Epic 20, Story 20.7, approved copy, decisions D1–D11).
- `_bmad-output/implementation-artifacts/20-1-wishlist-remove.md` … `20-6-epic-20-smoke-test.md`.
- `src/routes/api_v1.rs` (`ApiLocation`).

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List

### Change Log

- 2026-11-26 — create-story (with 19-11).
