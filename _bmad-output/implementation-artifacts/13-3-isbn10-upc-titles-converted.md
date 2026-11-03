# Story 13.3: ISBN-10 titles stored as UPC are converted

Status: review

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

- [x] T1 — model query: candidate rows (`isbn IS NULL AND upc IS NOT NULL AND CHAR_LENGTH(upc) = 10 AND deleted_at IS NULL`), conversion UPDATE with version check
- [x] T2 — service `services::title::convert_isbn10_upc_titles(pool) -> Vec<(title_id, isbn13)>` (pure selection in Rust via `isbn10_to_isbn13`)
- [x] T3 — `main.rs`: call after migrations, then spawn the re-fetches sequentially (one task looping over the list)
- [x] T4 — tests (AC6); `cargo sqlx prepare`
- [x] T5 — gates; full suites once on the final code before the single push

## Completion Notes

- **Model** (`src/models/title.rs`): `Isbn10UpcCandidate`, `TitleModel::list_isbn10_shaped_upc` (`isbn IS NULL AND upc IS NOT NULL AND CHAR_LENGTH(upc) = 10 AND deleted_at IS NULL`, oldest first) and `TitleModel::convert_upc_to_isbn` (`isbn = ?, upc = NULL, version = version + 1` guarded by `id`, `version`, `isbn IS NULL`, the read `upc` and `deleted_at IS NULL`; returns `false` when superseded). Dynamic `sqlx::query`, so no `.sqlx/` change.
- **Service** (`src/services/title.rs`): `isbn10_upc_target` (selection predicate, wraps `utils::isbn10_to_isbn13`) and `convert_isbn10_upc_titles(pool) -> Isbn10UpcConversion` (converted `Vec<ConvertedTitle { title_id, isbn13, media_type }>` plus collision / not-ISBN-10 / superseded / error counts). The spec's `Vec<(title_id, isbn13)>` is extended with the media type (the re-fetch needs it) and the skip counts (logged). Collision = `TitleModel::find_by_isbn` finds a live holder → `warn` with `title_id` and `holder_title_id`, title unchanged. A per-title DB error is logged and the run continues. One `info` line with all counts per run (AC4). `manually_edited_fields` is not touched (AC5).
- **Re-fetch task** (new `src/tasks/isbn10_upc_refetch.rs`): `spawn(state, titles)` (no-op for an empty list; 30 s warm-up) and `run(state, titles)` — sequential lookups through the scan wrapper `tasks::metadata_fetch::fetch_metadata_chain` with `CodeType::Isbn` and `force_refresh = true`, paced by `bulk_refetch_delay_ms` between titles; a throttled lookup is queued by story 13-2's retry queue.
- **`src/main.rs`**: the data step runs after migrations, the seed gate, the startup purge and the legacy env-var migration, before the settings load; non-fatal (`error` log, empty list). The re-fetch is spawned right after `TcpListener::bind`, so startup is never delayed.
- **`upc` readers checked**: the title edit form, title detail, MCP `get_title` / `title_lifecycle` JSON and the `has_code` guards all read the three identifier columns together — the title keeps a code (its ISBN). `TitleModel::find_by_upc` (scan of a UPC) would no longer match the 10-character code, but since v1.21.0 that code is classified `isbn` and looked up by its ISBN-13, which now matches; a home search by the ISBN-10 also resolves to the ISBN-13 (story 11-1, `tests/search_isbn10.rs`).
- **Tests**: unit `services::title::tests::isbn10_upc_target_selects_valid_isbn10_only` (production codes, title 478, UPC-A, EAN-13, other lengths). DB integration `tests/upc_isbn10_conversion.rs` (10 tests): conversion (+ `X` check digit, version bump, hand-edit list unchanged), collision, deleted holder is no collision, two titles with the same ISBN-10 (second one collides), left alone (bad checksum, UPC-A, short code, already-ISBN, deleted), idempotence, version / UPC guard of the UPDATE; plus the re-fetch `run` with a stub provider: looked up by ISBN-13 in order, throttled → queued for retry, hand-edited title survives.
- **E2E**: none. No UI change; the conversion is a boot-time data repair, and the E2E stack cannot hold a pre-v1.21.0 UPC title before boot without seeding the DB behind the app (a dedicated migration or a manual SQL step before the container starts). The DB integration tests are the proof.
- **Note**: a collided title stays a candidate, so its `warn` repeats at every boot until #591 (merge) resolves it — intended, it keeps the case visible.
- No file over 2000 lines (`src/models/title.rs` 1766).
