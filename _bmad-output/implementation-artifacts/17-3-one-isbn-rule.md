# Story 17.3: One ISBN rule for every form

Status: ready-for-dev

GitHub issue: #565 (type:code-review-finding, deferred from story 12-1). Covers: FR174. Epic 17; ships in v1.27.0.

## Story

As a librarian typing an ISBN by hand,
I want it stored the way a scan stores it,
so that a later scan finds the title instead of creating a duplicate.

## Where ISBN input is read today (inventory)

| Entry point | Code | Separators | `ISBN` prefix | ISBN-10 → 13 | Checksum |
|---|---|---|---|---|---|
| `/catalog/scan`, home scan `/scan` | `routes/catalog_code.rs::detect_code_type` (`utils::compact_isbn`, `isbn10_to_isbn13`) | yes | no | yes | downstream (`create_from_isbn`) |
| Web search, MCP `search_titles` | `services/search.rs` (`compact_isbn` → `detect_code`) | yes | no | yes | no (falls back to text) |
| Manual title creation | `services/title.rs::create_manual` (`non_empty_option`) | **no** | **no** | **no** | **no** — stored as typed; the field has `maxlength="13"` |
| Title edit (CR #272) | `routes/titles.rs` (`identifier_without_separators`) | yes | no | **no** ("must be 13 digits") | yes; `pattern="[0-9X]{0,13}" maxlength="13"` blocks a hyphenated value in the browser |
| Wish list "By ISBN" (preview + create) | `routes/wishlist.rs` (`validate_isbn13_checksum(trim)`) | **no** | **no** | **no** | yes; `pattern="[0-9]{13}"` |
| Expected tomes (form, proposals) | `services/expected_tomes.rs::normalize_isbn` | yes | no | yes | yes, 978/979 only |
| Expected-tome settling | `models/expected_tome.rs::live_titles_with_isbn_in` — SQL `UPPER(REPLACE(REPLACE(isbn,'-',''),' ',''))` + the ISBN-10 form | SQL copy | – | – | – |
| Volume move picker | `models/volume_move.rs::search_candidates` (`strip_isbn_separators`) | yes | no | no | – |
| Boot repair 13-3 | `services/title.rs::convert_isbn10_upc_titles` (`isbn10_to_isbn13`) | – | – | yes | yes |

## Production logs (2026-10-10 → 2026-11-10, read 2026-11-10)

- `Creating new title` logs the stored ISBN of every new title: 743 creations with an ISBN, **all 13 digits `978`/`979`**; 55 without ISBN. The 39 `Manual title created` lines all fall among those without ISBN.
- 782 `Processing scan` lines: none with a separator or an `ISBN` prefix (only `V,/+`, story 17-1).
- `Title metadata updated manually` (784) does not log the ISBN.
- The first `Creating new title` is on 2026-10-26; titles up to ~id 280 predate the logs, so the stored values of older titles can only be read from a database export (`id, isbn, deleted`), which the project lead was asked for. Until it is read, the repair's effect on production is unknown (Rule #20).

## Decisions

- **D1 — one rule, positive shapes (Rule #20).** `utils::isbn::parse_isbn(input) -> Result<String, IsbnError>`: optional prefix `ISBN`, `ISBN-10`, `ISBN-13`, `ISBN10`, `ISBN13` (any case), optional `:`; the separators of `strip_isbn_separators` anywhere; what remains must be 13 digits starting `978`/`979` with a valid check digit (kept), or a valid ISBN-10 (converted to its ISBN-13). Anything else is refused (`Format` or `Checksum`), never rewritten. The `10`/`13` qualifier is read both as qualifier and as digits; the two readings differ by two characters, so at most one has a valid length — `ISBN-13 1312345678` keeps all its digits.
- **D2 — 978/979 only.** As the scan and the expected tomes already do: another EAN-13 is not an ISBN. The title edit accepted any 13 digits with a valid EAN check digit; a stored value that is left unchanged still round-trips (CR #272 behaviour kept).
- **D3 — forms.** `normalize_isbn_input(input) -> Result<Option<String>, IsbnError>`: blank (or separators only) → `None`. Manual creation, title edit, wish list (preview and create) and expected tomes use it. Two terse field hints (Rule #9): `error.isbn.invalid_format` (reworded: 10 or 13 digits, an example) and `error.isbn.invalid_checksum` (reworded: check digit, look for a typo); `wishlist.isbn_invalid` is dropped for them (DRY).
- **D4 — scan and search.** `isbn_lookup_form(input)`: the canonical ISBN-13 when `parse_isbn` accepts the input, otherwise the separator compaction as before — a mistyped check digit still reaches the scan's checksum refusal, and a search still falls back to text.
- **D5 — manual creation refuses an ISBN another live title holds** (`error.isbn.already_used`, the edit form's guard): the story's goal is that a typed ISBN never makes a duplicate.
- **D6 — the repair** (`services::isbn_repair`, `17-3-stored-isbn-one-rule`, `data_repairs::run_once`, at boot **before** the 13-3 conversion so its collision check sees canonical values). Live titles only. Each stored ISBN goes through `parse_isbn`; it is rewritten only when accepted and different (separators, prefix, bare ISBN-10 from the old manual form). Values grouped by their canonical form among live titles: a group of more than one title that involves a rewrite is left untouched and logged (`warn`, every id). Write guarded by `id`, `version` and the old value; `version` not bumped (#521, 13-4). Pending metadata/cover items' `identity.isbn` follows the rewrite (as `carry_identity_title`), so the repair does not outdate them. The classification is pure (`plan_isbn_repair`) and runs over a CSV export through an ignored test (`MYBIBLI_ISBN_CORPUS`), never committed data.
- **D7 — the SQL copy goes.** After the repair every live title's ISBN is canonical or not an ISBN, so `live_titles_with_isbn_in` compares `isbn = ?` with the ISBN-13 only; the ISBN-10 form and the SQL `REPLACE` are removed.
- **D8 — fields.** The browser constraints that refused a hyphenated value go: manual ISBN `maxlength="13"` → 40, edit `pattern`/`maxlength="13"` → 40, wish list `pattern="[0-9]{13}"` removed.
- **D9 — volume move picker** puts an exact ISBN match first through `isbn_lookup_form`; its partial `LIKE` keeps the separator compaction.

## Acceptance Criteria

1. **AC1.** Manual creation, title edit, wish list "By ISBN", scan, search and expected tomes go through `utils::isbn` (separators, prefix, ISBN-10 → 13, checksum); an invalid ISBN in a form is refused with a terse hint, in all four locales.
2. **AC2.** The SQL separator rule of `models/expected_tome.rs` is removed (D7).
3. **AC3.** Stored ISBNs are repaired once per database on positive shapes, without bumping `titles.version`; colliding titles left untouched and logged.
4. **AC4.** The repair's classification is run over the production export and its changes recorded here before review.
5. **AC5.** Unit (parser shapes, plan), DB integration (manual creation, edit, wish list, repair: rewrite, collision, version, identity, run once) and E2E (a hyphenated ISBN typed by hand, then scanned bare, reaches the same title; an invalid ISBN gets the hint).

## Tasks

- [ ] `src/utils/isbn.rs`: `parse_isbn`, `normalize_isbn_input`, `isbn_lookup_form`, `IsbnError::message` + unit tests; ISBN helpers moved there.
- [ ] Scan, search, manual creation (+ duplicate guard), edit, wish list, expected tomes, volume move: use it.
- [ ] Templates: field constraints (D8).
- [ ] Locales: reword the two hints, drop `wishlist.isbn_invalid` (en/fr/de/it).
- [ ] `models/expected_tome.rs`: drop the SQL copy and the ISBN-10 form.
- [ ] Repair service + model writes + `main.rs` call + DB integration + corpus test.
- [ ] E2E spec.
- [ ] Corpus run over the export (AC4).

## Dev Notes

- Lock scope (Rule #22): the repair takes no `FOR UPDATE`; it runs at boot before the listener, in the `run_once` transaction, with guarded writes by primary key; the identity carry reads items from `idx_proposal_items_target` like `carry_identity_title`.
- `cargo fmt` is not run (repo-wide reformat); write in the neighbouring style.
- References: `_bmad-output/planning-artifacts/epics.md` § Story 17.3; issue #565; stories 11-1, 12-1, 13-3, 13-4, 17-1.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5

### Completion Notes List

### File List
