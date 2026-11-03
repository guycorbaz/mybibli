# Story 13.2: A throttled metadata fetch is retried

Status: ready-for-dev

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

- [ ] T1 — migration + model for the queue (enqueue, due batch, bump, dequeue)
- [ ] T2 — enqueue on `Throttled` (scan path and manual re-download path; the bulk loop keeps its own retries and does not enqueue unless its own retries are exhausted)
- [ ] T3 — retry task + schedule (pure function) + spawn in `main.rs`
- [ ] T4 — tests (AC7)
- [ ] T5 — gates; `cargo sqlx prepare`; full suites once on the final code before the single push

## Dev Notes

- The task needs the same inputs as the scan path (`ProviderRegistry`, timeouts, `reqwest::Client`, `covers_dir`): take them from `AppState` exactly as `routes/catalog.rs` does when it spawns `fetch_metadata_chain`.
- Deployment follow-up (not code): after v1.23.0, the eight production titles of #583 can simply be re-downloaded by hand.
