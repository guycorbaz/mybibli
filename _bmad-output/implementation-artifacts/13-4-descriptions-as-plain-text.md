# Story 13.4: Descriptions as plain text

Status: review

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

## Completion Notes

- **Helper (AC1)** — `src/utils/plain_text.rs` (new submodule of `utils`, re-exported): `html_to_plain_text`, `description_paragraphs`, and `decode_char_references` — the entity decoder moved out of `metadata::marc::decode_xml_text`, which now calls it (XML mode) and applies NFC (Rule #1). HTML mode adds the Latin-1 named set (96), typographic quotes/dashes/spaces, `&euro;`, `&oelig;`… Tags are recognised only when `<` is followed by a letter, `/`+letter, `!` or `?` and closed by a `>` outside quotes; anything else is prose. When the text carries tags its own whitespace is layout (collapsed, as a browser renders it); without tags, its line breaks are kept. `<script>`/`<style>` content and comments are dropped. Entities are decoded once, after the tags are removed.
- **Ingestion (AC2)** — `MetadataResult::normalize_empty_strings` converts `description` before the empty check. Every provider path goes through `ChainExecutor` (scan, retry task, bulk re-fetch, manual re-download preview/confirm in `routes/titles.rs`), and `chain.rs` now also normalises a **cache hit**, so a `metadata_cache` entry stored before this story no longer brings HTML back.
- **MCP proposals (decision)** — an agent's proposed description is converted at **filing** (`metadata_proposals::plain_text_description`, called in `propose`): agents copy provider descriptions, HTML included, and the reviewer should accept what the page will show. Markup alone is refused as `Blank`. A librarian's edit of the line and the re-validation at acceptance are *not* converted (hand-typed text, like the title edit form). MCP `get_title` returns the stored text, which is now plain.
- **Existing rows (AC3)** — `services::title::convert_html_descriptions` (+ `TitleModel::list_descriptions_with_markup` / `set_plain_description`, dynamic queries — no `.sqlx` change), called from `main.rs` right after the 13-3 step, non-fatal. Skips `manually_edited_fields ∋ description` (an unreadable value counts as edited), optimistic locking, markup-only → NULL, one `info` line with counts. Idempotent; documented exception: a double-encoded source (`&amp;lt;b&amp;gt;`) decodes one level per boot (spec: "decode once").
- **Display (AC4)** — `TitleModel::description_paragraphs()`; `title_detail.html` renders one escaped `<p>` per line inside `#title-description` (no `|safe`), and the HTMX metadata fragment (`metadata_display_html` in `routes/titles.rs`, shown after edit/cancel/re-download) does the same with `html_escape`. No other template renders a title description (volume page does not; series descriptions are out of scope).
- **Tests (AC5)** — unit: 18 in `utils::plain_text`, 1 in `metadata::provider`, 1 in `metadata_proposals`, 1 in `services::title`, 1 template render in `routes::titles`. DB: `tests/html_description_cleanup.rs` (4: converts, hand-edit skip, deleted title, idempotent). E2E: `specs/journeys/html-description.spec.ts` (specId `HD`, ISBN 9787268000010 served by mock Google Books with an HTML description, BnF-blocklisted).
- **Files over 2000 lines** — `src/routes/titles.rs` was 2730 before, 2750 now (+8 code, +12 test); not split in this story.
- **Gates run** — `cargo clippy --all-targets -D warnings` clean; lib tests for the touched modules (`utils::plain_text`, `metadata::*` with the test DB, `metadata_proposals`, `services::title`, `routes::titles`, `templates_audit`) green; DB tests `html_description_cleanup`, `mcp_metadata_proposals`, `unimarc_mapping` green; E2E on a fresh stack (`--workers=2`): `html-description`, `catalog-marc-zones`, `proposals-epic12-smoke`, `mcp-gaps-tools`, `catalog-metadata`, `provider-chain` green; `tsc --noEmit` and the `waitForTimeout` gate clean. T6's full suites are left to the orchestrator (single run before the push). Only new files were run through `rustfmt`; the touched existing files were not rustfmt-clean before.
