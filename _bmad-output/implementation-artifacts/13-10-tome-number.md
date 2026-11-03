# Story 13.10: The tome number

Status: review

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

- [x] T1 — parser + table tests (AC1)
- [x] T2 — migration + model (`detected_tome`) (AC3)
- [x] T3 — ingestion (AC2)
- [x] T4 — title page: hint display, pre-fill, clear on attach, position edit (AC3, AC5)
- [x] T5 — display everywhere (AC4)
- [x] T6 — manual-entry hint (AC6)
- [x] T7 — MCP `get_title` field (registry test stays green)
- [x] T8 — i18n, tests, gates; full suites once before the single push

## Completion Notes

**What was done**

- **Parser (AC1)** — `services::tome::split_tome_marker` (new module `src/services/tome.rs`): Tome / Tomo / T / T. / Vol. / Volume / Band / Bd. / n° / nº / No / No. / #, case-insensitive, also between `()` / `[]`, number 1..=10 000. Conservative: the marker must be a whole token followed by the end of the title or a separator (` - `, ` : `, `, `, `. `, ` ; `, dashes) itself followed by a space; the word before a mid-title marker cannot be an article ("Le Tome 2 : …"); a leading bare "12." is never a marker. Only the marker and adjacent separators are removed ("Thorgal - Tome 29 - Le sacrifice" → "Thorgal - Le sacrifice"; when both sides carry a separator, the right one is kept). Table of 56 cases (29 positive, 27 negative, incl. "Vol 714 pour Sydney", "Tome et Janry", "XIII", "Apollo 13", "Tome 0 : Prologue").
- **Migration + model (AC3)** — `titles.detected_tome INT NULL` (`migrations/20261103180000_titles_detected_tome.sql`); SQL in the new `src/models/title_tome.rs` (dynamic queries only — no `.sqlx` change). Hint writes bump neither `version` nor `updated_at`.
- **Ingestion (AC2, orchestrator routing)** — `prepare_metadata_title` + `record_detected_tome`, called from `tasks::metadata_fetch::update_title_from_metadata` (scan / retry / admin refetch) and from the re-download handler. The marker is always stripped from the provider value; nothing is routed when `title` is hand-edited. In no series → `detected_tome`; in one or more series → no position changes, debug log when the number differs. **Provider-number choice:** a marker in the title wins; without one, a comic's (`bd`) plain-integer `collection_number` (UNIMARC 225$v / MARC 490$v) is used — never for books (that zone is a publisher's collection number, e.g. Folio), never `issue_number` (a magazine's own field). No boot-time repair of stored titles.
- **Title page (AC3, AC5)** — "Tome N · Series" line under the heading per membership (series name as plain text — the Series section below keeps the only link, so existing `a[href^="/series/"]` locators stay unique) ("Tomes N–M" for an omnibus), or "Tome N (detected)" when in no series; `#assign-position` pre-filled with the hint; the hint is cleared by `SeriesService::assign_title` / `assign_omnibus` (hand attach). Inline `<details>` position editor per non-omnibus membership → `POST /title/{id}/series/{assignment_id}/position` (`src/routes/title_tome.rs`, HTMX `HX-Redirect`, errors in `#feedback-list`), `services::tome::change_position`: optimistic lock on `title_series.version`, ≥ 1, closed-series total, not a position the title already holds in the series (restores a soft-deleted twin row instead of hitting the unique key), then `ExpectedTomeService::settle_for_title_id`.
- **Display (AC4)** — browse table rows and cards (`SearchResult.tome_positions` via a `GROUP_CONCAT` subquery in `search_projection`), series page cards (label now `tome.word`; `series.volume_short` removed from the 4 locales), volume page title row (also its OOB update after a move).
- **Manual entry (AC6)** — a hand-typed title is never rewritten: the edit form shows a hint under the title field when the stored title holds a marker; saving an edit or creating a title manually adds an info feedback entry with the same hint.
- **MCP (T7)** — `get_title` returns `detected_tome` (read-only), description updated; series positions were already returned.
- **i18n** — `tome.*` keys in en/fr/de/it (Volume / Tome / Band / Tomo).

**Tests** — unit: `services::tome` (12 tests, parser table ≥ 30), `volume_move` row test. DB integration: new `tests/tome_number.rs` (15 tests: ingestion strip + routing in/out of series, hand-edited title, comic collection-number fallback, hint display / pre-fill / clear on attach, position edit with version conflict, validations, soft-deleted twin, expected-tome settlement, anonymous refused, list / series / volume display, `get_title`). E2E: new `tests/e2e/specs/journeys/tome-number.spec.ts` (specId `TN`; the mock BnF answers every `specIsbn("TN", seq ≥ 1000)` with "Tome 3 : Le Mystère des tomes", and each attempt scans a fresh seq so retries never meet an earlier attempt's state); `series.spec.ts` assertions updated from "Vol. N" / "#3-5" to "Tome N" / "Tomes 3–5".

**Files above 2000 lines** — `src/routes/titles.rs` 2782 (was 2768: +14, template fields and two call lines; all new logic lives in new modules), `src/routes/catalog.rs` 3226 (was 3220: +6). Both were already over the limit (#597).

**Deviations**

- AC2's "series position when in exactly one series and free" was replaced by the orchestrator's rule: never change a position at ingestion.
- Position edit "position free": as in the existing assignment handler, only a position the same title already holds is refused — two titles sharing a position stays allowed (#524: rendered, not prevented).
- An omnibus range is not editable inline (remove and re-assign); a localized message says so.
- The hint is only cleared by hand attaches from the title page/service; series attaches through proposal acceptance or scan auto-settle leave it in place (it is hidden while the title is in a series), to keep the epic-12 lock scope of those transactions untouched.
- T8's full suites are left to the orchestrator's final gate.
