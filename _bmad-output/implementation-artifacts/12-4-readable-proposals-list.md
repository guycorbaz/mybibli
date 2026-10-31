# Story 12.4: A readable proposals list

Status: ready-for-dev

GitHub issue: #561 (type:change-request) — closes it. Covers: FR151, UX-DR35. Epic 12 — fourth story; ships in v1.22.0.

## Story

As a librarian,
I want each row of the proposals list to take me to the proposal and show what it proposes,
so that I review quickly and never land on the title page by mistake.

## UX decision (project lead, 2026-10-31, recorded in #561)

The **subject** (title or series name) becomes the link to the proposal; a small secondary link "View title" / "View series" stays next to it; the "Open" column goes. Under the subject, a "Proposed:" line lists the proposed fields (metadata) or shows a thumbnail of the proposed image (pending cover).

Observed on 2026-10-30: the librarian repeatedly clicked the subject (then a link to `/title/{id}`), landed on the title page and found "nothing proposed, no Accept button" — worse when the title is a bare ISBN (`2205001213`).

## Existing code (2026-10-31)

- `src/routes/proposals.rs::ListRow` (l. 56) + `proposals_page` (l. 98) build rows from `proposal_review::list_page` (`ListEntry { summary, target }`); `ListEntry` drops the `ProposalSubject` it computed (fields of a metadata proposal).
- `templates/pages/proposals.html` (l. 45–75): table `#proposals-table`, columns Kind / Target (link to `/title|/series`) / Filed (sm+) / Key (sm+) / Status / Items (sm+) / Open (`a[href="/proposals/{id}"]`, `open_aria`).
- `GET /proposals/{id}/cover` (`routes/proposals.rs` l. 500) serves the staged JPEG, librarian-only, `no-store`; 404 once promoted or discarded.
- `proposal_review::field_label(field, loc)` gives localized field names (incl. `title` since 12-5).
- Tests select rows by `tr[data-proposal-id]` and the proposal link by `a[href="/proposals/{id}"]` (`proposals-review.spec.ts`, `proposals-access.spec.ts`, `series-epic11-smoke.spec.ts`, `tests/proposals_page.rs`).

## Acceptance Criteria

1. **AC1 — one primary link.** In each row, the subject text is the link to `/proposals/{id}` (the only `a[href="/proposals/{id}"]` of the row; its accessible name is the subject, plus a visually hidden "proposal #N" so two rows on the same title stay distinguishable). The "Open" column is removed.
2. **AC2 — secondary catalog link.** When the target is a live title or series, a small, clearly labelled link "View title" / "View series" (localized) follows the subject, pointing at `/title/{id}` / `/series/{id}`. No nested interactive elements.
3. **AC3 — "Proposed:" line.** Under the subject: for a metadata proposal, "Proposed: <field labels, comma-separated>" (live lines, localized labels); for a pending cover proposal, a thumbnail (`<img src="/proposals/{id}/cover" loading="lazy" alt="Proposed cover for <subject>" width=… height=…>`, small, CSP-clean via Tailwind classes); for decided cover proposals no thumbnail (the staged file is gone); series proposals: nothing new.
4. **AC4 — layout.** Works at phone width (the existing `sm:` column dropping kept; no horizontal scroll at 360 px); dark mode; axe-core passes on `/proposals`; no inline style/script (templates audit green).
5. **AC5 — i18n.** New keys in en/fr/de/it (`proposals.view_title`, `proposals.view_series`, `proposals.row_proposed`, `proposals.thumb_alt`, `proposals.row_link_sr`); `locale_parity` green; unused `open` / `open_aria` keys removed if nothing else uses them.
6. **AC6 — tests.** Unit: row view model (fields line, thumbnail only for pending cover, view link per target kind). DB/integration (`tests/proposals_page.rs`): the list HTML has the subject link to `/proposals/{id}`, the "View title" link, the "Proposed:" line, the thumbnail for a pending cover and none once rejected, no "Open" column. E2E: from `/proposals`, clicking the subject opens the detail page; a cover row shows its thumbnail; existing specs keep passing.
7. **AC7 — docs.** Manual ch. 13 "The list" (EN + FR) describes the row; PDFs rebuilt.

## Tasks / Subtasks

- [ ] T1 — `ListEntry` keeps the subject; `ListRow` gains `proposed_fields: Option<String>`, `thumb: bool`, `view: Option<(String, String)>` (href, label), `link_sr`
- [ ] T2 — template rows (AC1–AC4)
- [ ] T3 — locales (AC5)
- [ ] T4 — tests (AC6), E2E
- [ ] T5 — manual (AC7)
- [ ] T6 — gates

## Dev Notes

- Keep `tr[data-proposal-id]` and a single `a[href="/proposals/{id}"]` per row so the existing specs keep working.
- The thumbnail must not trigger a broken image for decided proposals: render it only when `kind == cover && status == pending`.
- Escape everything through Askama (no `|safe` on agent-supplied names).

### References

- `_bmad-output/planning-artifacts/epics.md` § Story 12.4, FR151, UX-DR35; issue #561 (UX decision comment)

## Dev Agent Record

### Agent Model Used

### Completion Notes List

- Ultimate context engine analysis completed - comprehensive developer guide created

### File List
