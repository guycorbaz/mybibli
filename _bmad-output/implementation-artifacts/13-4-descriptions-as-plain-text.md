# Story 13.4: Descriptions as plain text

Status: ready-for-dev

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

- [ ] T1 — helper + unit tests (AC1)
- [ ] T2 — `normalize_empty_strings` (AC2)
- [ ] T3 — startup cleanup (AC3), called from `main.rs` after migrations
- [ ] T4 — template paragraphs (AC4); templates audit stays green
- [ ] T5 — E2E (new ISBN case in the mock server returning HTML description; specId `HD`)
- [ ] T6 — gates; full suites once before the single push
