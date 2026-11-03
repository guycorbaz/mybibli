# Story 13.3: ISBN-10 titles stored as UPC are converted

Status: done

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

## Dev Notes

- **Give-up.** A converted title whose re-fetch stays throttled through the whole 13-2 schedule (15 min … 24 h) is dequeued with a `warn` only: it keeps its ISBN-13 and its old metadata. Nothing else is lost — a librarian can still run a manual re-download from the title page, which now looks it up by ISBN.
- **Queue meaning.** A live `metadata_fetch_retries` row means "this title still needs a lookup" — a throttled latest lookup, or a re-fetch pending after this identifier change (module docs of `tasks::metadata_retry`).

## Completion Notes

- **Model** (`src/models/title.rs`): `Isbn10UpcCandidate`, `TitleModel::list_isbn10_shaped_upc` (`isbn IS NULL AND upc IS NOT NULL AND CHAR_LENGTH(upc) = 10 AND deleted_at IS NULL`, oldest first) and `TitleModel::convert_upc_to_isbn` (`isbn = ?, upc = NULL, version = version + 1` guarded by `id`, `version`, `isbn IS NULL`, the read `upc` and `deleted_at IS NULL`; returns `false` when superseded). Dynamic `sqlx::query`, so no `.sqlx/` change.
- **Model** also: candidates restricted to print media types (`book`, `bd`, `magazine`, `report`) — review L2.
- **Service** (`src/services/title.rs`): `isbn10_upc_target` (selection predicate, wraps `utils::isbn10_to_isbn13`) and `convert_isbn10_upc_titles(pool) -> Isbn10UpcConversion` (converted `Vec<ConvertedTitle { title_id, isbn13, media_type }>` plus collision / not-ISBN-10 / superseded / error counts). The spec's `Vec<(title_id, isbn13)>` is extended with the media type (the re-fetch needs it) and the skip counts (logged). Collision = `TitleModel::find_by_isbn` finds a live holder → `warn` with `title_id` and `holder_title_id`, title unchanged. A per-title DB error is logged and the run continues. One `info` line with all counts per run (AC4). `manually_edited_fields` is not touched (AC5).
- **Re-fetch** (revised in review round 1, M1): each converted title is put on the story 13-2 retry queue (`MetadataRetryModel::enqueue`, delay 0 = due now) in the same transaction as its conversion UPDATE. The retry task (`tasks::metadata_retry`) drains it: `prepare` re-reads the title, so the lookup uses its ISBN (`CodeType::Isbn`) with `force_refresh = true`, paced by `bulk_refetch_delay_ms`, isolated per row, skipped during bulk runs, and a throttled lookup is rescheduled. A stop before the lookup loses nothing. The first-cut `src/tasks/isbn10_upc_refetch.rs` and its spawn are removed. A title already on the queue (an earlier throttled UPC lookup) is reset to a fresh schedule, due now (`MetadataRetryModel::enqueue_fresh`, review round 2).
- **`src/main.rs`**: the data step runs after migrations, the seed gate, the startup purge and the legacy env-var migration, before the settings load; non-fatal (`error` log, boot continues). No spawn of its own — the retry task (warm-up 2 min, tick 5 min, 20 titles per tick) does the re-fetch.
- **`upc` readers checked**: the title edit form, title detail, MCP `get_title` / `title_lifecycle` JSON and the `has_code` guards all read the three identifier columns together — the title keeps a code (its ISBN). `TitleModel::find_by_upc` (scan of a UPC) would no longer match the 10-character code, but since v1.21.0 that code is classified `isbn` and looked up by its ISBN-13, which now matches; a home search by the ISBN-10 also resolves to the ISBN-13 (story 11-1, `tests/search_isbn10.rs`).
- **Tests**: unit `services::title::tests::isbn10_upc_target_selects_valid_isbn10_only` (production codes, title 478, UPC-A, EAN-13, other lengths). DB integration `tests/upc_isbn10_conversion.rs` (14 tests): conversion (+ `X` check digit, version bump, hand-edit list unchanged), collision, deleted holder is no collision, two titles with the same ISBN-10 (second one collides), left alone (bad checksum, UPC-A, short code, already-ISBN, deleted, CD / DVD), idempotence, version / UPC guard of the UPDATE; queued due now then one retry `tick()` looks the titles up by ISBN-13 and dequeues them; throttled re-fetch stays queued (rescheduled); hand-edited title survives.
- **E2E**: none. No UI change; the conversion is a boot-time data repair, and the E2E stack cannot hold a pre-v1.21.0 UPC title before boot without seeding the DB behind the app (a dedicated migration or a manual SQL step before the container starts). The DB integration tests are the proof.
- **Note**: a collided title stays a candidate, so its `warn` repeats at every boot until #591 (merge) resolves it — intended, it keeps the case visible.
- No file over 2000 lines (`src/models/title.rs` 1766).

### Review Findings

Round 1 (f6f956e): 0 High, 1 Medium, 4 Low.

- [x] [Review][Patch] M1 — re-fetch list only in memory, lost on a stop before the loop ends → each converted title is enqueued (due now) on the 13-2 retry queue in the conversion transaction; `isbn10_upc_refetch.rs` and its spawn removed; tests drive one `tick()`.
- [x] [Review][Patch] L2 — selection ignored the media type → candidates limited to print types (book, bd, magazine, report); media type logged on the conversion and collision lines; test with a CD and a DVD.
- [x] [Review][Dismiss] L3 — codes with separators: the pre-v1.21 scan stored a UPC only when all-digit, 8–13 chars (`v1.20.0:src/routes/catalog.rs:420`) and the edit form stripped whitespace and `-` (`v1.20.0:src/routes/titles.rs:752`); a separator left by the manual create form makes the code longer than 10, so it is not selected — untouched, no harm.
- [x] [Review][Patch] L4 — comment at the collision check: exact-string match on `isbn` among live titles; a soft-deleted holder is no collision (a restore can leave two live titles with one ISBN), see #591.
- [x] [Review][Dismiss] L5 — the old metadata-cache row under the 10-digit code is harmless: nothing looks that code up any more, and the re-fetch invalidates the ISBN-13 cache (`force_refresh`).

Round 2 (44a7990): 0 Medium+, 4 Low.

- [x] [Review][Patch] L1 — an already-queued title kept its old attempts / schedule → new `MetadataRetryModel::enqueue_fresh` (revives or resets to `attempts = 0`, due now, version + 1), used only by the conversion; test `an_already_queued_title_gets_a_fresh_schedule`.
- [x] [Review][Patch] L2 — retry-task module docs now say a live row means "still needs a lookup" (throttled, or pending re-fetch after an identifier change); the reschedule log reads "throttled" (not "throttled again") when `attempts == 0`. Schedule unchanged.
- [x] [Review][Patch] L3 — give-up behaviour documented in Dev Notes (old metadata kept, warn only, manual re-download still possible).
- [x] [Review][Patch] L4 — tests: atomicity (`a_failed_enqueue_rolls_the_conversion_back`, queue table dropped in the per-test DB → title unchanged, counted as error) and `bd` / `report` / `book` kept as candidates.
