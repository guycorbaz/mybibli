# Story 13.4: Descriptions as plain text

Status: done

GitHub issue: #578 (type:bug, severity:medium). Covers: bug fix. Epic 13; ships in v1.23.0.

## Story

As a reader of a title page,
I want the description to read as text,
so that I never see `<p>`, `<b>` or `&lt;` in it.

## Evidence

Titeuf, *Bienvenue en adolescence*: the title page shows `&lt;p&gt;&lt;b&gt;Grandis un peu, Titeuf !&lt;/b&gt;&lt;/p&gt;&lt;p&gt;La vie de Titeuf …`. The agent session of 2026-11-03 also met Danish texts and "Bandes dessinees." as descriptions (out of scope: wrong content, not markup — #589).

## Existing-code reality check (code map, 2026-11-03)

| Place | Today |
|---|---|
| `src/metadata/google_books.rs:84`, `src/metadata/comic_vine.rs:113` | store the provider `description` verbatim (both APIs return HTML) |
| `src/metadata/library_of_congress.rs:477` | `decoded()` → `marc::decode_xml_text` (XML entities + NFC) |
| `src/metadata/provider.rs::MetadataResult::normalize_empty_strings` (l. 62) | **the single normalisation point**, called by `chain.rs` on every winner (l. 255) and every extra (l. 317) |
| `src/metadata/marc.rs::decode_xml_text` (l. 144) | numeric + 5 named XML entities, NFC; unknown entities kept verbatim |
| `templates/pages/title_detail.html:104–107` | `<p>{{ desc }}</p>` — escaped (good), one paragraph, line breaks lost |
| manual edit form / `manually_edited_fields` | hand-typed descriptions must never be rewritten |

## Acceptance Criteria

1. **AC1 — one helper (Rule #1).** `utils::html_to_plain_text(&str) -> String`: removes tags; turns `<br>`, `</p>`, `</div>`, `</li>` into line breaks (at most one blank line in a row); decodes entities once — the XML five, numeric, and the common HTML named ones (`&nbsp;`, `&eacute;`… — at least the Latin-1 set and typographic quotes/dashes); trims; NFC. Text without `<` or `&` is returned unchanged (byte-identical). It reuses `marc::decode_xml_text` for the numeric/XML part or moves the shared core to `utils`.
2. **AC2 — applied at ingestion.** `MetadataResult::normalize_empty_strings` applies it to `description` (so every provider and every chain merge is covered), before the empty check.
3. **AC3 — existing rows.** A one-shot startup step converts stored descriptions that contain `<` or `&…;`, except titles whose `manually_edited_fields` contains `description`; optimistic locking; idempotent; logs a count.
4. **AC4 — display.** The title page renders the description's paragraphs (split on blank lines / line breaks into `<p>` elements in the template, still escaped; no `|safe`; CSP unchanged). Same for any other place that shows the description (check the volume page, MCP `get_title` returns the plain text).
5. **AC5 — tests.** Unit: tags, nested and malformed tags (`<p>a <b>b</p>`), attributes containing `>` inside quotes, entities (named, numeric, double-encoded `&amp;lt;` → `&lt;` once), plain text unchanged, `<` in plain prose (`a < b`) kept. DB integration: the startup step converts, skips a hand-edited description, is idempotent. E2E: a title fed an HTML description by the mock metadata server (`tests/e2e/mock-metadata-server/server.py`) shows plain paragraphs on its page.

## Tasks / Subtasks

- [x] T1 — helper + unit tests (AC1)
- [x] T2 — `normalize_empty_strings` (AC2)
- [x] T3 — startup cleanup (AC3), called from `main.rs` after migrations
- [x] T4 — template paragraphs (AC4); templates audit stays green
- [x] T5 — E2E (new ISBN case in the mock server returning HTML description; specId `HD`)
- [x] T6 — gates; full suites once before the single push

## Dev Notes

Known limits (accepted, review round 3):

- A description in `manually_edited_fields` (typed by hand, or accepted through a proposal) keeps any markup it holds: the repair never converts a hand-typed text, by design. A librarian can edit it.
- A pending proposal item whose `current` was truncated at filing (`current_truncated`, 64 KiB cap) keeps its old text: it no longer equals the title's description, so the repair leaves it. That value is display-only in the review screen (staleness for such items relies on `version`, which the repair does not bump).
- Two processes booting at the same time: the marker's `FOR UPDATE` on an absent row takes a gap lock, so one of them may deadlock on the marker insert — its repair rolls back, the error is logged as non-fatal, and the other commits the repair once. Single-tenant: accepted.

## Completion Notes

- **Helper (AC1)** — `src/utils/plain_text.rs` (new submodule of `utils`, re-exported): `html_to_plain_text`, `description_paragraphs`, and `decode_char_references` — the entity decoder moved out of `metadata::marc::decode_xml_text`, which now calls it (XML mode) and applies NFC (Rule #1). HTML mode adds the Latin-1 named set (96), typographic quotes/dashes/spaces, `&euro;`, `&oelig;`… Tags are recognised only when `<` is followed by a letter, `/`+letter, `!` or `?` and closed by a `>` outside quotes; anything else is prose. When the text carries tags its own whitespace is layout (collapsed, as a browser renders it); without tags, its line breaks are kept. `<script>`/`<style>` content and comments are dropped. Entities are decoded once, after the tags are removed.
- **Ingestion (AC2)** — *revised in review round 1 (M1/L3)*: the conversion runs in the two providers that answer with HTML, Google Books and Comic Vine, not in `MetadataResult::normalize_empty_strings` — BnF / LoC text such as `<<Le>>` must not be mangled (deviation from AC2's wording, at the reviewer's request). The chain's empty check still drops a markup-only description. No cache-hit re-normalisation: the cache stores the already-cleaned winner; stale cache rows are purged by the repair.
- **MCP proposals (decision)** — an agent's proposed description is converted at **filing** (`metadata_proposals::plain_text_description`, called in `propose`): agents copy provider descriptions, HTML included, and the reviewer should accept what the page will show. Markup alone is refused as `Blank`. A librarian's edit of the line and the re-validation at acceptance are *not* converted (hand-typed text, like the title edit form). MCP `get_title` returns the stored text, which is now plain.
- **Existing rows (AC3)** — *module moved in review round 2 to `services::description_repair` (see below)*; originally `services::title::convert_html_descriptions` (+ `TitleModel::list_descriptions_with_markup` / `set_plain_description`, dynamic queries — no `.sqlx` change). Skips `manually_edited_fields ∋ description` (an unreadable value counts as edited), optimistic locking, markup-only → NULL, also soft-deletes `metadata_cache` rows whose description holds markup (a cache row does not name its provider; they expire after 24 h anyway), one `info` line with counts. *Review round 1 (M1)*: the conversion is not idempotent on its own output, so `main.rs` calls `repair_html_descriptions_once`, which runs it **once per database** via `services::data_repairs::run_once` and the new `data_repairs` table (migration `20261103150000_create_data_repairs.sql`; marker written after success, so a repair that fails as a whole re-runs next boot; individually failed rows are logged, not retried).
- **Display (AC4)** — `TitleModel::description_paragraphs()`; `title_detail.html` renders one escaped `<p>` per line inside `#title-description` (no `|safe`), and the HTMX metadata fragment (`metadata_display_html` in `routes/titles.rs`, shown after edit/cancel/re-download) does the same with `html_escape`. No other template renders a title description (volume page does not; series descriptions are out of scope).
- **Tests (AC5)** — unit: 18 in `utils::plain_text`, 1 in `metadata::provider`, 1 in `metadata_proposals`, 1 in `services::title`, 1 template render in `routes::titles`. DB: `tests/html_description_cleanup.rs` (6: converts incl. BnF `<<Le>>` untouched, hand-edit skip, deleted title, runs once / converted `<b>x</b>` left alone on later boots, cache purge, failing repair leaves no marker) and `tests/html_description_proposals.rs` (2: pending series_attach still acceptable; pending description proposal repaired and accepted). E2E: `specs/journeys/html-description.spec.ts` (specId `HD`, ISBN 9787268000010 served by mock Google Books with an HTML description, BnF-blocklisted).
- **Files over 2000 lines** — `src/routes/titles.rs` was 2730 before, 2750 now (+8 code, +12 test); not split in this story.
- **Gates run** — `cargo clippy --all-targets -D warnings` clean; lib tests for the touched modules (`utils::plain_text`, `metadata::*` with the test DB, `metadata_proposals`, `services::title`, `routes::titles`, `templates_audit`) green; DB tests `html_description_cleanup`, `mcp_metadata_proposals`, `unimarc_mapping` green; E2E on a fresh stack (`--workers=2`): `html-description`, `catalog-marc-zones`, `proposals-epic12-smoke`, `mcp-gaps-tools`, `catalog-metadata`, `provider-chain` green; `tsc --noEmit` and the `waitForTimeout` gate clean. T6's full suites are left to the orchestrator (single run before the push). Only new files were run through `rustfmt`; the touched existing files were not rustfmt-clean before.

### Review Findings

Round 1 (7e39054): 0 High, 1 Medium, 6 Low.

- [x] [Review][Patch] M1 — conversion unstable on its own output; boot repair and cache-hit re-normalisation would rewrite healthy text. Fixed: run-once marker (`data_repairs` table, `services::data_repairs::run_once`, `repair_html_descriptions_once`); HTML cleaning moved into Google Books / Comic Vine parsers; cache-hit re-normalisation removed; markup cache rows purged by the repair; regression test `the_repair_runs_once_and_leaves_converted_text_alone`.
- [x] [Review][Patch] L1 — quadratic scans on repeated `<!--` / unterminated tags / `&` without `;`. Fixed: an unterminated tag or comment ends tag recognition (rest is text), the `;` of an entity is looked for in the next 32 bytes only; test `hostile_inputs_are_scanned_linearly` (200k chars).
- [x] [Review][Patch] L2 — HTML numeric references 128–159 now map through the HTML5 Windows-1252 table (`&#146;` → ’); XML (MARC) mode unchanged; test `c1_references_are_windows_1252_in_html_mode`.
- [x] [Review][Patch] L3 — covered by M1 (cleaning only in the HTML providers; test `metadata_result_normalize_does_not_strip_markup_like_text`).
- [x] [Review][Dismiss] L4 — CDATA sections: not produced by the providers, no impact.
- [x] [Review][Patch] L5 — `description_paragraphs` moved above the doc comment of `parsed_manually_edited_fields`, which keeps it.
- [x] [Review][Dismiss] L6 — `src/routes/titles.rs` over 2000 lines: pre-existing Rule #12 debt; the coordinator opens an issue for splitting `titles.rs` / `catalog.rs`.

Round 2: 1 Medium, 6 Low.

- [x] [Review][Patch] M1 — the repair bumped `titles.version`, outdating pending series proposals, and left HTML `current`/`proposed` in pending description items (`FieldChanged`). Fixed: `TitleModel::set_plain_description` is a non-edit write (guarded by `version` and the old value, `version`/`updated_at` unchanged, as #521); in the same transaction the repair rewrites pending metadata `description` items — a recorded `current` equal to the converted text becomes the new text, an agent `proposed` value (not librarian-edited) is converted like a new filing (`description_repair::repair_item`, pure + unit-tested). Item ids come from a plain non-locking read; writes by primary key (lock-scope rule). Tests in `tests/html_description_proposals.rs`. The repair moved to its own module, `src/services/description_repair.rs` (services/title.rs back to its pre-13-4 state).
- [x] [Review][Patch] L1 — tags are recognised only for known HTML element names (+ comments, doctype); `<<Le>> Petit Prince`, `<Astérix>`, `<bible>` stay text; `a <b> c` → `a c` (a known element is a tag even unbalanced), `<b>c</b>` → `c`.
- [x] [Review][Patch] L2 — order: cache purge first (outside, idempotent), then titles + proposal items + marker in one transaction (`run_once` now passes the transaction's connection to the repair and writes the marker in it; a locking marker read serialises concurrent boots).
- [x] [Review][Patch] L3 — the converter drops control characters other than tab / line feed from its output (`&#127;`, raw C0/C1), so a converted agent description cannot bypass the `ControlCharacter` rule.
- [x] [Review][Patch] L4 — covered by M1 (agent `proposed` descriptions filed before the deploy are converted by the repair).
- [x] [Review][Patch] L5 — module doc of `src/utils/plain_text.rs` corrected.
- [x] [Review][Patch] L6 — `a_failing_repair_writes_no_marker`: no marker, the repair's writes rolled back, it runs on the next call.

Round 3: 0 Medium+, 5 Low.

- [x] [Review][Patch] L1 — `DataRepairModel::is_applied_in` doc now states the gap-lock behaviour of two simultaneous boots (one deadlocks, logged non-fatal, the other commits); accepted, single-tenant.
- [x] [Review][Patch] L2 — Dev Notes: a hand-edited / proposal-accepted description keeps its markup, by design.
- [x] [Review][Patch] L3 — Dev Notes: a truncated `current` keeps its old text (display-only).
- [x] [Review][Patch] L4 — DB tests added in `tests/html_description_proposals.rs`: description + publisher in one proposal accepted after the repair; a librarian-edited line keeps its proposed value (and is accepted); a pending cover proposal on a converted title is accepted.
- [x] [Review][Patch] L5 — `TagScan::NotATag` doc aligned with `tag_end` (`<?…?>` and CDATA are text; comments and doctype are the only `<!` tags).
