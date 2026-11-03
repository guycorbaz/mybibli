# Story 13.10: The tome number

Status: ready-for-dev

GitHub issue: #579 (type:change-request). Covers: FR157, FR158. Epic 13; ships in v1.23.0. Before Story 13.11 (both touch series positions).

## Story

As a librarian,
I want the tome number shown and edited as the title's own, and never left inside the title text,
so that series read cleanly and titles sort correctly.

## Decisions (project lead, 2026-11-03 — recorded in #579)

- The number stays on `title_series.position_number` (omnibus titles hold several positions; a title may belong to several series; expected tomes and MCP series proposals rely on it), but behaves as the title's number.
- **The "Tome N" text never stays in `titles.title`.**
- A title not yet in any series keeps the stripped number in a **non-authoritative hint** on the title (e.g. `titles.detected_tome INT NULL`): displayed, pre-fills the position when a series is attached, then cleared. `title_series.position_number` remains the only authoritative value.
- Displayed as "Tome N" everywhere (title page, lists, search results); editable from the title page.
- Titles already catalogued are cleaned **through the MCP agent** (title correction via `propose_title_metadata`, position via `propose_series_attach`) — no bulk tool in this story.

## Existing-code reality check (to verify at story start)

| Place | Today |
|---|---|
| `title_series` (initial schema l. 190–205) | `position_number INT NOT NULL`, `is_omnibus`, unique `(title_id, series_id, position_number)`; an omnibus = several rows |
| `templates/pages/title_detail.html` l. 205–280 | lists series assignments `(#N)` / `(#N-M)`; `#assign-series` form with `#assign-position` (required) and `#assign-end-position` (omnibus) |
| `src/routes/series.rs` / `titles.rs` | assignment handlers (find the POST behind the form) |
| `src/tasks/metadata_fetch.rs::update_title_from_metadata` (l. 258) | writes `titles.title` from metadata with the `manually_edited_fields` guard |
| `src/metadata/provider.rs::MetadataResult::normalize_empty_strings` | single normalisation point of provider results (Story 13.4 adds HTML cleaning there) |
| `MetadataResult.collection_number` / `issue_number` | providers may already carry a number separately — prefer it when present |
| browse / search result rows, series page cards (`series_detail.html:85` "Vol. N — Title") | where "Tome N" must show |

## Acceptance Criteria

1. **AC1 — parser.** One pure function `utils::split_tome_marker(&str) -> (String, Option<i32>)` (or in a `services::tome` module) recognises a tome marker at the start or the end of a title, case-insensitive, FR/EN/DE/IT: `Tome 12`, `T12`, `T. 12`, `T.12`, `Vol. 12`, `Volume 12`, `Band 12`, `Bd. 12`, `n° 12`, `No 12`, `#12`, with separators ` - `, ` : `, `, `, `. ` around it; it returns the cleaned title (trimmed, separators removed) and the number (1..=10 000). It leaves alone titles where a number is part of the name ("1984", "Les 4 As", "Spirou et Fantasio 12" without marker, "Apollo 13", "Tome" alone, "Tomes et tomettes"). Unit-tested over a table of real comic titles.
2. **AC2 — ingestion.** When metadata sets the title (`update_title_from_metadata`, and title creation from a provider result) and `title` is not in `manually_edited_fields`, the marker is stripped from `titles.title`; the number (or a provider's explicit tome number if present) goes to: the series position when the title is in exactly one series and that position is free and unset… — otherwise to `titles.detected_tome`.
3. **AC3 — hint.** Migration adds `titles.detected_tome INT NULL` (+ `.sqlx`). The title page shows "Tome N (detected)" when the title is in no series; when a series is attached from the title page, `#assign-position` is pre-filled with the hint; after a successful attach the hint is cleared. The MCP `get_title` returns `detected_tome` (read-only).
4. **AC4 — display.** The title page header, the browse/search result rows and cards, the series page cards and the volume page show "Tome N" from `title_series.position_number` (for several series: per series; omnibus: "Tomes N–M"); localized label (4 locales).
5. **AC5 — edit from the title page.** The librarian can change a title's position in a series from the title page (inline form per assignment: new position, optimistic locking on `title_series.version`, same validations as assignment: position free, ≥ 1, closed-series total). Expected-tome settlement rules are re-applied after the change (settle/unsettle as the series rules say — reuse `expected_tomes` helpers).
6. **AC6 — manual entry.** A hand-typed title (manual creation or edit) is never rewritten; when it contains a marker, the form shows a hint suggesting to move the number to the series position (no automatic change).
7. **AC7 — tests.** Unit: parser table (≥ 30 cases, positive and negative). DB integration: ingestion strips + routes the number (in-series vs hint), hint cleared on attach, position edit with version conflict. E2E (specId `TN`): a scanned title whose mock metadata title carries "Tome 3" shows without the marker and with "Tome 3 (detected)"; attaching a series pre-fills 3; editing the position from the title page.

## Tasks / Subtasks

- [ ] T1 — parser + table tests (AC1)
- [ ] T2 — migration + model (`detected_tome`) (AC3)
- [ ] T3 — ingestion (AC2)
- [ ] T4 — title page: hint display, pre-fill, clear on attach, position edit (AC3, AC5)
- [ ] T5 — display everywhere (AC4)
- [ ] T6 — manual-entry hint (AC6)
- [ ] T7 — MCP `get_title` field (registry test stays green)
- [ ] T8 — i18n, tests, gates; full suites once before the single push
