# Story 13.2: A throttled metadata fetch is retried

Status: review

GitHub issue: #583 (type:bug, severity:medium). Covers: bug fix. Epic 13; ships in v1.23.0.

## Story

As a librarian,
I want a title whose lookup failed only because a provider was saturated to be looked up again later,
so that it does not stay without metadata for a transient reason.

## Evidence (production, 2026-10-31 → 2026-11-02)

Google Books answered HTTP 503 several hundred times; eight titles (335, 337, 89, 288, 79, 462, 572, 591) ended with `Metadata chain throttled (429/503), no result` and were never looked up again.

## Existing-code reality check (code map, 2026-11-03)

| Place | Today |
|---|---|
| `src/tasks/metadata_fetch.rs::fetch_metadata_chain_outcome` (l. 133–248) | `None` + `chain_outcome.throttled` → `mark_failed` + `FetchOutcome::Throttled` (l. 237–241): same terminal state as "no result" |
| `pending_metadata_updates` | per-session delivery row (status `pending/resolved/failed`), **soft-deleted by `middleware/pending_updates.rs` once delivered** — not usable as a retry queue |
| `src/routes/admin*.rs` bulk cover-refetch (#419) | retries throttled titles inside its own run with back-off |
| `src/tasks/*` | background-task patterns: `provider_health.rs`, `auto_purge_scheduler.rs` (`tokio::time::interval`, `MissedTickBehavior::Skip`) |
| `src/routes/home.rs:751` | dashboard counts `status = 'failed'` rows |

## Acceptance Criteria

1. **AC1 — enqueue.** When a scan-time (or manual re-download) fetch ends with `FetchOutcome::Throttled`, the title is recorded in a retry queue (new table, e.g. `metadata_fetch_retries`: `title_id` unique, `attempts`, `next_attempt_at`, timestamps; migration + `.sqlx`) with the first delay. Re-enqueueing a title already queued keeps one row. The delivery row is still marked as today (the librarian sees the "no metadata yet" outcome).
2. **AC2 — not for genuine misses.** A chain that returns nothing without throttling is not enqueued (final, as today).
3. **AC3 — retry task.** A background task (new `src/tasks/metadata_retry.rs`, spawned in `main.rs` like the others, `MissedTickBehavior::Skip`, tick every few minutes) takes due rows (a bounded batch, oldest first), re-reads the title (skip and dequeue a deleted title or one that already has metadata), and runs `fetch_metadata_chain_outcome` with `force_refresh = true`. Outcome: resolved / not found → dequeue; throttled again → `attempts + 1`, next delay from a pure schedule (e.g. 15 min, 1 h, 4 h, 12 h, 24 h), dequeue after the last attempt and log a warning.
4. **AC4 — metadata applied as at scan.** A successful retry updates the title through the existing path (`update_title_from_metadata`, `manually_edited_fields` respected, cover download).
5. **AC5 — no burst.** The retry batch runs titles sequentially with a short pause between them (reuse the #419 inter-request delay setting if one exists), so a retry wave cannot itself trigger a 503 storm.
6. **AC6 — logging.** Enqueue, each retry outcome and the final give-up are logged at `info`/`warn` with `title_id` and `attempts`.
7. **AC7 — tests.** Unit: the back-off schedule and the enqueue decision. DB integration (`tests/metadata_retry.rs`): a throttled outcome enqueues once; a due row resolved by a (mock) chain is dequeued and the title updated; a row past its last attempt is dequeued; a deleted title is skipped. The mock metadata server can answer 503 for a given ISBN if needed (check `tests/e2e/mock-metadata-server/server.py`); E2E optional — a DB test is the proof here.

## Tasks / Subtasks

- [x] T1 — migration + model for the queue (enqueue, due batch, bump, dequeue)
- [x] T2 — enqueue on `Throttled` (scan path and manual re-download path; the bulk loop keeps its own retries and does not enqueue unless its own retries are exhausted)
- [x] T3 — retry task + schedule (pure function) + spawn in `main.rs`
- [x] T4 — tests (AC7)
- [x] T5 — gates (targeted; the full suites run once by the orchestrator); `cargo sqlx prepare`; full suites once on the final code before the single push

## Dev Notes

- The task needs the same inputs as the scan path (`ProviderRegistry`, timeouts, `reqwest::Client`, `covers_dir`): take them from `AppState` exactly as `routes/catalog.rs` does when it spawns `fetch_metadata_chain`.
- Trash restore (review L1, accepted): a title soft-deleted while queued has its row dequeued by the next tick; restoring the title from Trash does not re-enqueue it — a re-download does if the provider is still saturated.
- Deployment follow-up (not code): after v1.23.0, the eight production titles of #583 can simply be re-downloaded by hand.

## Completion Notes

**What was done**

- **Queue** — migration `migrations/20261103120000_create_metadata_fetch_retries.sql` (`title_id` unique, `attempts`, `next_attempt_at`, AR24 quartet, FK `ON DELETE CASCADE` to `titles` so the 30-day purge takes the row along). Model `src/models/metadata_retry.rs` (`enqueue`, `due_batch`, `reschedule`, `dequeue`; dequeue = soft delete). Enqueueing a live row leaves it untouched (a rescan cannot postpone a retry); enqueueing a soft-deleted row revives it with `attempts = 0`. Table added to `tests/seed_drift.rs`.
- **Enqueue** (`src/tasks/metadata_retry.rs`): the scan wrapper `fetch_metadata_chain` (all scan-time call sites, unchanged) enqueues on `FetchOutcome::Throttled`; the manual re-download (`routes/titles.rs::redownload_metadata`) enqueues on a throttled miss and *dequeues* on a found result (no repeat of a lookup the librarian just made). Genuine misses are never queued. Delivery rows are still marked as before.
- **Bulk loop (#419), decision**: it calls `fetch_metadata_chain_outcome` directly, so its in-run attempts never enqueue; only a title still throttled after `THROTTLE_RETRY_BACKOFF` is exhausted is handed to the queue (`routes/admin.rs`). No doubling: the retry task likewise calls `_outcome` directly and owns its reschedule.
- **Retry task**: `tick(&AppState)` (separately callable) takes ≤ 20 due rows oldest first, re-reads each title (deleted / `metadata_source` already set / no code → dequeued without lookup), runs `fetch_metadata_chain_outcome(force_refresh = true)` (same path as a scan: `manually_edited_fields`, provenance, cover download), pauses `bulk_refetch_delay_ms` between lookups. Pure `verdict`: not throttled → dequeue; throttled → `attempts + 1` and next delay from `RETRY_SCHEDULE_SECS` (15 min, 1 h, 4 h, 12 h, 24 h), give up with a `warn` after the 5th retry. `spawn` in `main.rs`: 2 min warm-up, tick every 5 min, `MissedTickBehavior::Skip`.
- `TitleModel::lookup_code()` (ISBN → UPC → ISSN) shared by the re-download handler and the task (DRY; shrinks `titles.rs` by 5 lines).

**Queue rule (review round 1, M2, supersedes the first draft):** a live row means "this title's latest lookup was throttled". Every lookup that ends not throttled — scan wrapper, bulk loop (final outcome), re-download (once applied, or on a genuine miss), retry task — dequeues; a throttled miss enqueues (the task reschedules). `prepare` skips only a deleted title or one without a lookup code; the earlier `metadata_source` skip is gone, so a throttled re-download / backfill of an already-resolved title is retried as well (`manually_edited_fields` protects hand edits).

**Tests**: unit (`tasks::metadata_retry`: schedule lock, enqueue decision, verdict; `models::title::lookup_code_prefers_isbn_then_upc_then_issn`); DB integration `tests/metadata_retry.rs` (11 tests in round 0, 18 after review round 1, stub provider: throttled enqueues once, genuine miss not queued, revived row, resolved row dequeued + title updated + source recorded, manual edits respected, rescheduled, past last attempt dequeued, deleted title skipped, already-resolved skipped, not-yet-due untouched, found elsewhere drops the row).

**Deviations**: no `.sqlx` change — the queries are runtime `sqlx::query` (no macros), so `cargo sqlx prepare` has nothing to regenerate. No locale change.

**E2E skipped — justification against Foundation Rule #3:** the feature has no UI surface, and its user-visible effect only appears after a 15-minute (then hours-long) background back-off; exercising it in Playwright would need a test-only hook to shorten the schedule or fire a tick, i.e. production code that exists for the test. The proof is DB/router-level instead: `tests/metadata_retry.rs` drives the real scan wrapper, the real `/title/{id}/redownload` route (logged-in browser, CSRF, stub providers) and one real `tick` of the task.

**Files above 2000 lines**: `src/routes/titles.rs` 2727 (pre-existing, 2732 before; net −5), `src/routes/catalog.rs` 3200 (untouched).

### Review Findings

Code review round 1 of 885c804 (0 High, 2 Medium, 6 Low).

- [x] [Review][Patch] M1 — one failing row aborted the whole tick and, being the oldest, blocked the queue — each row now runs in `retry_row`; an error is logged with `title_id` and the row pushed back (`push_back_delay_secs`: current back-off slot, at least one tick); test `a_failing_row_does_not_stop_the_batch` (trigger-forced failure).
- [x] [Review][Patch] M2 — queue rule: a live row = "latest lookup throttled"; every non-throttled lookup end dequeues (`record_lookup` / `note_fetch_outcome`), `metadata_source` skip dropped; tests for scan success, bulk outcome, re-download paths, already-resolved title retried.
- [x] [Review][Patch] L1 — Trash restore does not re-enqueue; accepted, noted in Dev Notes.
- [x] [Review][Patch] L2 — task writes guarded by the `version` read in `due_batch` (`reschedule`, `dequeue_read`); `enqueue` bumps `version` on every write; test `an_enqueue_landing_during_a_retry_is_not_erased`.
- [x] [Review][Patch] L3 — tick skipped while a bulk cover-refetch / backfill run is in progress (`bulk_cover_fetch::snapshot().running`); test `the_tick_waits_while_a_bulk_run_is_in_progress`.
- [x] [Review][Patch] L4 — re-download dequeues only after the update succeeded (direct apply, no-change result, or `confirm_metadata` after `update_metadata`).
- [x] [Review][Patch] L5 — router-level tests of `/title/{id}/redownload` (throttled → enqueue, applied → dequeue, genuine miss → dequeue), bulk decision test, M1 isolation test; E2E justification in Completion Notes.
- [x] [Review][Patch] L6 — `context` field added to the dequeue-failure warn (now in `record_lookup`).
