# Story 11.20: Side-by-side view for metadata and covers

Status: ready-for-dev

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR137 (a proposed cover is shown next to the current one), NFR47, UX-DR31 (side-by-side current vs proposed values). Epic 11 — twentieth story. Not here: nav badge and dashboard indicator (11-21), cross-kind staleness (#542 stays open for the user — no behaviour change).

## Story

As a librarian,
I want to compare what exists with what is proposed,
so that I can decide at a glance.

## Existing-code reality check (code map, 2026-10-29, `main` ab06bc6)

| Place | Today | Consequence |
|---|---|---|
| `templates/fragments/proposal_detail_body.html` | one card (`<li data-line>`) per item; a metadata field shows a stacked `<dl>` "Current / Proposed" (`data-proposed` on the proposed `<dd>`); contributors "Current / To add"; a pending cover shows *proposed* then *current* figures (`#proposal-staged-cover`), "No cover" text when the title has none; an accepted cover shows the title's live cover (`#proposal-accepted-cover`, 11-18) | the cards, their Edit / Remove buttons and the inline edit slot (11-19) stay; only the inside of `field`, `contributors` and pending `cover` lines changes |
| `services/proposal_review.rs` | `LineView` (pure `parse_line`), `ReviewDetail`, `TitleBrief` names (`title`, `cover_image_url`) | the change classification is computed there (pure, unit-tested); `TitleBrief` gains `media_type` for the placeholder icon |
| `routes/proposals.rs::DetailBody` | index-aligned label vectors (`edit_notes`, `current_date_labels`, `cover_dims`, …); `edit_note` = "Edited by X on D. The assistant proposed: …" for every edited line | compared lines show the agent's original **in** the comparison; their note keeps who / when only |
| `GET /proposals/{id}/cover` (11-16) | Librarian+, `no-store`, pending only, re-validated staged name; router tests: anonymous → login, Librarian 200, 404 for non-cover / unknown / `/covers/.staging/…` | the AC's staged-image route already exists and is tested; this story re-asserts it from the side-by-side view |
| `templates/components/cover.html` | `cover` macro: image, or a placeholder (media-type icon, `role="img"`) whose `alt` is hard-coded English | the placeholder markup is reused with a localized label |
| E2E | `metadata-cover-proposal-accept.spec.ts` (`MA`), `series-proposal-edit.spec.ts` (`PE`, asserts `[data-edit-note]` contains the agent's original subtitle and `[data-proposed]` has the edited value) | `PE`'s note assertion moves to the comparison's original value; `[data-proposed]` keeps holding exactly the proposed value |

## Acceptance Criteria

1. **AC1 — metadata side by side (FR137, UX-DR31).** Each metadata line (scalar field or contributors) of a proposal's detail shows a two-column comparison — **Current** and **Proposed** (contributors: **To add**) — as a real `<table>` (caption = field name, `<th scope="col">` headers), side by side at every width including phone width (≥ 320 px, no horizontal page scroll; long values wrap).
2. **AC2 — changes marked by more than colour (WCAG 1.4.1).** Each compared line carries a text badge in its caption: **New** (the field is empty today, or contributors to add), **Changed** (the value differs), **Unchanged** (same value — reachable only through odd data, kept honest). A changed / new proposed cell is also emphasized (border + weight), never by colour alone. The badge is a pure function of the line (unit-tested).
3. **AC3 — edited lines (11-19).** When a librarian edited a compared line, the Proposed cell shows the edited value (the one acceptance applies) and, below it, the assistant's original value labelled as such; the line's note says who edited it and when. `current` stays the value recorded at filing (the staleness baseline, D-11-13-2) — the comparison never re-reads the live title for metadata.
4. **AC4 — cover side by side.** A pending cover proposal shows **Current cover** then **Proposed cover** side by side (two columns at every width, images shrink): the title's live cover, or the media-type placeholder with a localized accessible name when it has none; the proposed image from `/proposals/{id}/cover`. A text flag says whether accepting **replaces** the current cover or **adds** one. A deleted title / missing staged file keeps today's texts. The accepted-cover view (11-18) is unchanged.
5. **AC5 — the staged image is never public.** The proposed image is only ever referenced as `/proposals/{id}/cover` (Librarian / Admin; anonymous → login), never under `/covers/`, and the staged file name never reaches the page (11-16 tests stay; one more router assertion from the side-by-side view).
6. **AC6 — escaped, CSP-clean, four locales.** Proposed and original values are escaped text (NFR47: no `|safe`); no inline script / style; new labels in `locales/{en,fr,de,it}.yml` (`locale_parity`); router tests check no raw `proposals.` key leaks.
7. **AC7 — tests.** Unit: change classification (new / changed / unchanged, year, truncated, contributors), the cover flag, placeholder icon fallback. DB integration (`tests/proposals_page.rs`): the metadata table with headers and badges, an edited line's original in the comparison with the short note, `<script>` in a proposed value escaped, the cover side by side with the replace / add flag and the placeholder, staged route anonymous → login and Librarian 200. E2E (new spec, blank browser, `loginAs`): a metadata proposal compared side by side (new and changed badges), an edited line showing the agent's original; a cover proposal for a title without a cover (placeholder + "adds") and, after accepting it, a second cover proposal showing current vs proposed images and "replaces"; axe passes on both views; phone width has no horizontal scroll.

## Tasks / Subtasks

- [ ] `proposal_review.rs`: `LineView.change` + pure `change_of`; `TitleBrief.media_type` → `LineView.current_media_type` (AC1, AC2, AC4)
- [ ] `routes/proposals.rs::DetailBody`: change labels, cover flag, short edit note for compared lines (AC2–AC4)
- [ ] Template: comparison table for field / contributors lines, cover side by side with flag and placeholder (AC1–AC4, AC6)
- [ ] Locale keys ×4 (AC6)
- [ ] Tests: unit, `tests/proposals_page.rs`, E2E `proposal-side-by-side.spec.ts`, `series-proposal-edit.spec.ts` note assertion (AC7)
- [ ] Decisions D-11-20-*

## Dev Notes

- Reuse: `parse_line`, `LineView.edit.original` (11-19), `proposal_edit::line_summary`, `ProposalLabels::t`, the 11-16 cover route and its `servable_staged_name` predicate, `components/cover.html` placeholder look (icon `/static/icons/{media_type}.svg`; unknown media type → `book`).
- `live_title_briefs` is a dynamic query: read `CAST(media_type AS CHAR)` (CI charset pitfall). No `sqlx::query!` added → `.sqlx` unchanged.
- Keep selectors other specs use: `#proposal-items > li`, `[data-line]`, `[data-proposed]` (exactly the proposed value), `[data-edit-note]`, `#proposal-staged-cover`, `#proposal-accepted-cover`.
- E2E: spec id `SB`, file `proposal-side-by-side.spec.ts`; creates no series (#528); wait for the metadata fetch to settle before filing (it bumps the title version, which makes earlier proposals stale); accepting the first cover bumps the version, so the second cover is filed after it (#542 unchanged).
- Pitfalls: French copy in router tests, Askama escaping (`&#60;`), `mem-guard` + `-j 2`, no repo-wide `cargo fmt`.

### Validation notes (create-story checklist, 2026-10-29)

- Reuse verified: `GET /proposals/{id}/cover` and its router tests (`anonymous_visitors_are_sent_to_login` covers `/proposals/{id}/cover`; `the_staged_cover_is_served_to_librarians_only_by_proposal`), `LineView.edit.original` built by `review_detail` from `original_values`, `titles.media_type` ENUM (`book`, `bd`, `cd`, `dvd`, `magazine`, `report`) = the six icons in `static/icons/`, the `components/cover.html` placeholder look. No migration, no `sqlx::query!` (the brief query is dynamic).
- Regression watch: `tests/proposals_page.rs`, `tests/proposal_edit.rs`, `tests/proposal_accept_metadata_cover.rs`, `templates_audit` (no inline style / script, no `hx-confirm`), `locale_parity`, E2E `series-proposal-edit`, `metadata-cover-proposal-accept`, `mcp-cover-proposal`, `proposals-review`.
- Disaster prevention: never `|safe` an agent value; never put the staged file name or a `/covers/.staging` URL in the page; do not touch acceptance, staleness or the edit flow (#542 unchanged).

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.20] — FR137, NFR47, UX-DR31.
- [Source: epic-11-autonomous-decisions.md] — D-11-13-2, D-11-14-2, D-11-16-4/-7, D-11-18-8, D-11-19-3/-7.
- [Source: module docs of services/metadata_proposals.rs, services/cover_proposals.rs].

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

### File List
