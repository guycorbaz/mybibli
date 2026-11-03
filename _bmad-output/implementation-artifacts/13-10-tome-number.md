# Story 13.10: The tome number

Status: done

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

## Dev Notes

- **Shared positions (review L6, dismissed):** like `SeriesService::assign_title`, the position edit refuses only a position the *same* title already holds in the series; another title may hold the same position (existing behaviour, #524 "rendered, not prevented").
- **Lock order:** the position edit locks the series row, then the `title_series` rows, then (in the settle after commit) the tomes. The hint is cleared inside `TitleSeriesModel::assign_in` / `assign_omnibus_in`, i.e. last in every attach transaction (hand assignment, scan auto-settle: series → tomes → `title_series`; series proposal acceptance: proposal → items → series → targets incl. the title row → apply), after a plain read so the title row is write-locked only when a hint is set.

- **Known limits of the parser (accepted, review round 4)** — also in the `services::tome` module docs:
  - comic (`bd`) titles whose own name has the shape of a real tome marker are split: "L'Ennemi public n°1", "Le Prisonnier n°6", "Public Enemy #1", "Opération T4 : …", "Aktion T4 - …", "Panzer T34 : …", "Chanel N°5 : …". **Remedy:** the librarian edits the title by hand; `title` is then in `manually_edited_fields` and no later metadata download rewrites it;
  - false negatives by design: anything outside the three accepted shapes keeps its marker (ranges, lists, a number right after the marker's number, year-shaped numbers ≥ 1000 in a book, a trailing bare "T19", a marker right after a preposition / connector / article such as "Le Tome 2 : …");
  - cosmetic losses: in the middle shape the right-hand separator is kept and spacing normalised ("Harry Potter, tome 1 : …" → "Harry Potter : …"; "Thorgal. Tome 3 : …" → "Thorgal : …").
- **Deadlock (review round 4 L5):** the guarded hint UPDATE (`NOT EXISTS` on `title_series`) can, rarely, deadlock with a concurrent hand assignment; the hand assignment (`TitleSeriesModel::assign`) retries once on SQLSTATE 40001, and the hint write is best effort (logged).

### Review Findings

Round 1 (on 306fd8a): 1 Medium, 7 Low — all addressed.

- [x] **M1** — the parser ran for every media type and truncated real titles ("Symphonie n° 9", "Chanel N°5", "Kill Bill Vol. 1", "Aktion T4"…). Now gated by `MarkerSet`: `bd` knows every marker; `book` only Tome / Tomo / Volume / Vol. / Band; cd / dvd / magazine / report are never parsed (a magazine number never reaches `detected_tome`). Also for every media: `Vol` needs its dot ("Vol 93 : le récit"), `T` must be compact or dotted ("T 34 : un char de légende"). Per-media test table (`MEDIA_CASES`) with every reported case, plus an ingestion DB test.
- [x] **L1** — determiner list extended (ce, cet, cette, son, sa, ses, ma, mes, ton, ta, tes, notre, votre, leur, leurs, this, that, his, her, their, dem, den; der/die/das/il/lo/la already there); "Ce tome 2 : x" untouched (tested).
- [x] **L2** — the hint is cleared in `TitleSeriesModel::assign_in` / `assign_omnibus_in` (every attach path; service-level clear removed); `get_title` returns `detected_tome: null` while the title is in a series. Tests: attach by proposal acceptance, scan auto-settle, `get_title` in a series.
- [x] **L3** — re-download: the preview only strips; the number travels in a hidden `new_detected_tome` field and is recorded at confirmation when the saved title is the proposed cleaned title (`confirmed_tome`), also for a hand-edited title whose librarian accepts it; the direct-apply and no-change branches record after the write. Tests: unit `confirmed_tome`, DB preview→confirm (kept hand title / accepted clean title / no number).
- [x] **L4** — `change_position` locks the series row first (`ExpectedTomeModel::lock_series_in`) and reads the closed total from it.
- [x] **L5** — list / volume labels group per series: one series "Tome 3" / "Tomes 3–5"; several "Tome 3 · A, Tome 4 · B" (`TomeMembership`, `MEMBERSHIPS_SQL`). Unit + DB test.
- [x] **L6** — dismissed, documented in Dev Notes.
- [x] **L7** — tests added: re-download path, a left position becomes a gap named by its tome again, media-type false positives. E2E manual-title case now uses a book marker ("Tome 5") since "T5" is a comic-only marker.

Round 2 (on 1996042): 1 Medium, 5 Low — all addressed.

- [x] **M1** — a title with more than one marker of the active set ("Intégrale T1 à T3", "T1 & T2", "T1, T2, T3", "Thorgal - Tome 1 - Tome 2", "Le Scorpion T1 - T2", book "Volume 1 - Volume 2", "Tome 1, Tome 2 : x") is left untouched with no hint; a connector right before the marker (&, à, a, et, and, to, und, bis, au) refuses the split ("Intégrale et T3 : fin"). The dash is not a connector (it is a separator: "Thorgal - Tome 29 - …" must still split); "T1 - T2" is caught by the two-marker rule. A connector right after the marker already failed (no separator). All cases in the tables, per media type.
- [x] **L1** — "Band" / "Bd." only at the start of the title or after a separator: "Rock Band 2", "Boy Band 3 : x", "Jazz Band 1920 : x", "Die Schlümpfe Bd. 5" untouched; "Die Chroniken, Band 2", "Die Schlümpfe, Bd. 5", "Band 3 : …" stripped.
- [x] **L2** — `set_detected_tome` is a single UPDATE on the title row, no subquery; the memberships are read first with a plain query (`record_detected_tome`). A racing attach clears the hint after it or leaves it hidden while in a series (documented on the model).
- [x] **L3** — `TitleSeriesModel::assign` runs the insert and the hint clear in one transaction.
- [x] **L4** — documented on `clear_detected_tome_in`: the plain read sees the REPEATABLE READ snapshot and may miss a hint committed meanwhile; it stays hidden while the title is in a series.
- [x] **L5** — tests assert `titles.version` and `updated_at` unchanged when the hint is written (`record_detected_tome`, the fetch path's routing step) and cleared (proposal acceptance, hand assignment).

Round 3 (on fee9e45): 1 Medium, 3 Low — addressed.

- [x] **M1** — the parser now accepts known shapes only (`services::tome::shape`), with the title's single marker `MARKER N`: (A) leading `MARKER N SEP REST`; (B) trailing `LEFT [SEP] MARKER N`; (C) middle `LEFT [SEP] MARKER N SEP REST`. `SEP` ∈ `:` `-` `–` `.` `,` (dashes between spaces, the others followed by a space); `REST` starts with a letter (after optional opening quotes); `LEFT` holds a letter, ends with a letter or a closing quote / bracket, and its last word is no article / connector. Anything else — a number or a list right after the marker's number, a range, an ellipsis, several markers — leaves the title untouched with no hint. All the reported cases ("L'Intégrale T1 - 3", "Thorgal Intégrale Tome 1 - 3", "Coffret Tome 1, 2, 3", "Spirou T1, 2 et 3", "XIII T1 – 2", "Astérix T1 - 2 - 3", "Tome 1 - 2", book "Volume 1 - 2 : x") are in the tables. **Deviation:** the left separator stays optional in (C) so the issue's own example shape "Titeuf T12 : …" still splits. **Bare "T":** a trailing "T4" without a separator before it is refused ("Opération T4", "Aktion T4" untouched).
  Previous cases adjusted, with reasons: "L'Homme de l'année T. 3 - 1917" is now untouched (a REST starting with a digit is the range shape "T1 - 3" and cannot be told apart from a year); "Gaston T19." is now untouched (bare trailing "T" without a separator — the "Opération T4" trade-off); "Gaston - T19." still splits.
- [x] **L1** — a `.` before the marker is a separator only after a word of three letters or more and never after another `.`: "Le Spirou de... T4 : …", "Mademoiselle J. T.3 - …", "Dr. No 1" untouched (tested); "Thorgal. Tome 3 : …" still splits.
- [x] **L3** — the hint UPDATE is guarded again by `NOT EXISTS (SELECT 1 FROM title_series ts WHERE ts.title_id = titles.id AND ts.deleted_at IS NULL)` — `title_series` only, no join to `series`.

Round 4 (on 4be643d): no Medium+ left — closed.

- [x] **L1** — for `book`, numbers ≥ 1000 are years, not tomes: "Tome 1789 : la Révolution", "Vol. 1968 : la révolte" untouched; comics keep the whole range ("Batman #1000"). Tested.
- [x] **L2 / L3 / L4** — documented as known limits (prepositions / articles before a marker, cosmetic separator losses, false negatives) in Dev Notes and the parser module docs, together with the comic false positives ("L'Ennemi public n°1", "Public Enemy #1", "Opération T4 : …", "Panzer T34 : …", "Chanel N°5 : …") and their remedy (hand edit → protected by `manually_edited_fields`).
- [x] **L5** — `TitleSeriesModel::assign` retries its transaction once on a deadlock (`acceptance::is_deadlock`).
