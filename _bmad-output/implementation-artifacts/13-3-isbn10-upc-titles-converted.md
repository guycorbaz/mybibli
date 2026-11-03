# Story 13.3: ISBN-10 titles stored as UPC are converted

Status: ready-for-dev

GitHub issue: #584 (type:bug, severity:medium). Covers: bug fix (data). Epic 13; ships in v1.23.0.

## Story

As a librarian,
I want titles catalogued before v1.21.0 with an ISBN-10 to carry their ISBN,
so that metadata providers can find them.

## Evidence (production logs, 2026-11-02 bulk re-download)

Titles 99 (`2862270164`), 174 (`2604000288`), 271 (`2205001213`), 282 (`2205011502`) are stored with a valid ISBN-10 in `titles.upc` (`code_type upc` in the fetch logs); every provider misses them. Title 478 (`2205025062`) fails the ISBN-10 checksum.

## Existing-code reality check (code map, 2026-11-03)

| Place | Today |
|---|---|
| `titles.isbn VARCHAR(13)`, `titles.issn`, `titles.upc` | one identifier column per kind |
| `src/routes/catalog.rs::detect_code_type` | since v1.21.0 (11-1, #497) a valid ISBN-10 is classified `isbn` and carried as its ISBN-13 — new scans are fine |
| `src/utils.rs::isbn10_to_isbn13` | checksum + conversion |
| `src/config.rs::migrate_legacy_env_vars` (called from `main.rs` after migrations) | precedent for a one-shot startup data step |
| `TitleModel::find_by_isbn` | used to detect a collision |
| fetch spawn | `tasks::metadata_fetch::fetch_metadata_chain` (code, `CodeType::Isbn`, media type) |

## Acceptance Criteria

1. **AC1 — conversion.** At startup, after migrations (a one-shot step in Rust, not SQL, because it needs the ISBN-10 checksum and a fetch), every live title with `isbn IS NULL` and a `upc` that `utils::isbn10_to_isbn13` accepts gets `isbn = <ISBN-13>` and `upc = NULL`, under optimistic locking (`version + 1`), and its metadata is fetched again (`force_refresh = true`) in the background, sequentially with a pause between titles.
2. **AC2 — collision.** When another live title already holds that ISBN-13, the title is left unchanged and a `warn` names both ids (no merge — see #591).
3. **AC3 — left alone.** UPC values failing the checksum (`2205025062`), other UPC lengths, titles that already have an ISBN, and deleted titles are untouched.
4. **AC4 — idempotent.** A second boot converts nothing (nothing matches any more); the step logs one `info` line with the count converted / skipped.
5. **AC5 — fields protected.** `manually_edited_fields` is respected by the re-fetch as usual; the conversion itself does not mark `isbn` as hand-edited.
6. **AC6 — tests.** Unit: the selection predicate on sample codes. DB integration (`tests/upc_isbn10_conversion.rs`): conversion, collision, bad checksum, already-ISBN, deleted, idempotence (the fetch is spawned by the caller — test the data step separately from the spawn).

## Tasks / Subtasks

- [ ] T1 — model query: candidate rows (`isbn IS NULL AND upc IS NOT NULL AND CHAR_LENGTH(upc) = 10 AND deleted_at IS NULL`), conversion UPDATE with version check
- [ ] T2 — service `services::title::convert_isbn10_upc_titles(pool) -> Vec<(title_id, isbn13)>` (pure selection in Rust via `isbn10_to_isbn13`)
- [ ] T3 — `main.rs`: call after migrations, then spawn the re-fetches sequentially (one task looping over the list)
- [ ] T4 — tests (AC6); `cargo sqlx prepare`
- [ ] T5 — gates; full suites once on the final code before the single push
