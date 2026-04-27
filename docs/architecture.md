# mybibli Architecture Reference

This document records cross-cutting design decisions that apply to the whole
codebase. Story-specific decisions live in
`_bmad-output/implementation-artifacts/`; this file is for patterns whose
shape doesn't change between stories.

For developer-facing conventions (build commands, foundation rules, lint
gates), see `CLAUDE.md`.

## Permanent Delete & Auto-Purge

Soft-deleted items are hard-purged after a fixed **30-day retention window**.
The window is enforced by two complementary mechanisms:

1. **Startup purge** — synchronous, runs in `main()` after migrations and
   before binding the HTTP listener. Bounded by `LIMIT 10000` per DELETE
   batch with a per-table drain loop (max 100 iterations) so a single huge
   family can't stretch startup latency without bound. Opt-out for fast dev
   loops via `MYBIBLI_SKIP_STARTUP_PURGE`; only the values `1`, `true`, and
   `TRUE` are accepted (R3-N6 — anything else, including empty string and
   `0` / `false`, runs the purge as normal).
2. **Daily scheduler** — `src/tasks/auto_purge_scheduler.rs` spawns a
   background task on a configurable cadence. The interval is read from
   `AppSettings::auto_purge_interval_seconds` (default 86 400 s = 24 h) so
   ops can tune it without redeploy. First run waits 1 minute after startup
   to avoid competing with the synchronous startup purge. Uses
   `tokio::time::MissedTickBehavior::Skip` so a clock jump (NTP correction,
   suspend/resume) does NOT trigger a burst of back-to-back purges.

### FK dependency ordering

The whitelist of purge-eligible tables lives in
`src/services/soft_delete.rs::ALLOWED_TABLES`. The auto-purge runner walks
its own `deletion_order` list which intentionally lists child tables first
(e.g., `title_contributors` before `titles`, `volume_locations` before
`volumes`) so that when both parent and child have stale rows the child
DELETE doesn't trip a foreign-key constraint. Each table runs in its own
transaction (one tx per LIMIT-bounded DELETE batch) so a failure in one
family rolls back only that family's batch — the outer loop continues to
the next table.

> **Drift risk:** `deletion_order` is hand-curated separately from
> `ALLOWED_TABLES`. A new whitelisted table is silently skipped from
> auto-purge until added to `deletion_order`. Tracked as P15 — see
> `_bmad-output/implementation-artifacts/8-7-permanent-delete-and-auto-purge.md`.

### Audit trail

Every purge run — whether triggered by startup or the daily scheduler —
writes one row to the `admin_audit` table with `action = "auto_purge"` and
a JSON `details` payload of the shape:

```json
{
  "tables_processed": 6,
  "rows_deleted": 47,
  "errors_count": 0,
  "per_table": { "titles": 5, "volumes": 12, "contributors": 30 }
}
```

The audit insert is performed inside `AutoPurgeService::run_purge` itself
(not by the caller) so both code paths produce identical audit history.

The `admin_audit` table is **append-only**. There is no API for editing or
deleting rows. `AdminAuditModel::create()` reads the DB-assigned timestamp
back via `LAST_INSERT_ID()` so the in-memory struct exactly matches the
persisted row (no clock-skew drift).

### Manual permanent delete

Admins can also force permanent deletion from the Trash panel
(`POST /admin/trash/{table}/{id}/permanent-delete`). This path:

- requires the admin to type the exact item name (UX-DR8 friction modal);
- enforces `_csrf_token` per the global CSRF middleware;
- blocks **self-delete** of users (`table == "users" && id == user_id`);
- blocks deletion of the **last active admin** (preserves at least one
  active admin in the system).

Those two guards apply ONLY to the manual-delete handler. The auto-purge
scheduler/startup task has no such guard — anything older than 30 days is
hard-deleted regardless of role. (If you need to preserve an admin row past
the retention window, restore it from Trash before day 30.)

Each manual delete writes one `admin_audit` row with
`action = "permanent_delete_from_trash"`, `entity_type` = the table,
`entity_id` = the row id, and `details = {"item_name": "..."}`.

### UTC consistency

The auto-purge `WHERE deleted_at < NOW() - INTERVAL 30 DAY` filter and the
trash-panel `days_remaining` calculation both compare DB-side `TIMESTAMP`
values against `chrono::Utc::now().naive_utc()` (Rust side). To make those
comparisons unambiguous regardless of the MariaDB server's
`default-time-zone`, `db::create_pool` registers a sqlx `after_connect`
hook that runs `SET time_zone = '+00:00'` on every new connection (R3-N15).
This means `NOW()` always returns UTC for our process, eliminating the
class of bugs where a row "older than 30 days" in the server's local TZ is
ambiguous in UTC.

### Stats counters in the audit payload

`PurgeStats` exposes three mutually-exclusive counters per run (R3-N2 +
R3-N11):

- `tables_attempted` — every whitelisted table the runner visited.
- `tables_succeeded` — drained without error (rows deleted may be `0`).
- `tables_errored` — at least one batch errored; some rows may still
  have committed before the failure (look at `per_table` and `errors`
  for the per-table detail).

`tables_processed` is preserved as an alias of `tables_succeeded` for
backwards-compatible parsing. `per_table` lists EVERY attempted table
(R3-N7), with a count of `0` when nothing was deleted; this lets forensic
readers distinguish "processed but empty" from "skipped due to error" by
cross-referencing the `errors` array.

When the per-table drain loop hits its `MAX_DRAIN_ITERATIONS = 100` cap
(roughly 1 M rows per family per run), the cap event is appended to
`stats.errors` and surfaced in `details.errors_count` so monitoring
tooling sees it without scraping logs (R3-N12).
