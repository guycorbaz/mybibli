# Story 19.5: Production watching at a glance (closes #708)

Status: ready-for-dev

## Story

As the operator,
I want to see from time to time how the instance is doing, without reading the logs,
so that a misbehaving client or a slow database is noticed in days, not weeks.

## Context

#696 ran eleven days in production before anyone noticed it, at about 1 000 `slow statement` warnings and several pool timeouts a day. It was found only by reading the logs by hand. The project lead does not want to read logs every day, only to glance at the instance from time to time (patch v1.28.1 retrospective, Q1).

Covers FR178 and UX-DR45 (the watching panel; the backup status line of UX-DR45 belongs to story 19-7).

What exists:

- **Slow statements.** sqlx logs them itself: `sqlx-core 0.8.6 src/logger.rs:124`, event at target `sqlx::query`, level `WARN` (the default `slow_statements_level`), message "slow statement: execution time exceeded alert threshold", default threshold 1 s. mybibli does not change the connect options (`src/db.rs`). sqlx exposes no hook, so the only place to count them is a `tracing` layer.
- **Pool timeouts.** `sqlx::Error::PoolTimedOut` after `acquire_timeout(30 s)` (`src/db.rs:59`). It reaches handlers through `impl From<sqlx::Error> for AppError` (`src/error/mod.rs:291`), and background tasks match it in a few places (`tasks/metadata_retry.rs:451`).
- **CSRF refusals.** `middleware/csrf.rs::reject` (:308) logs `client = client_addr(…)` on every refusal.
- **API-key refusals.** `middleware/api_key.rs::unauthorized` (:117, 401) and `insufficient_scope` (:141, 403) log the client too. The MCP guard's protocol-version refusal (`src/mcp/mod.rs`) is a client fault, not an authentication refusal; it stays out.
- **Logging setup.** `src/main.rs:80-130` uses a global `reload::Layer<EnvFilter>` (the admin "Log level" form swaps it), then the stdout JSON layer and the optional file layer. With a global filter, an operator level of `error` disables the `WARN` callsite, so a counting layer would see nothing.
- **Health tab.** `routes/admin.rs::admin_health_panel` (:317) with the `AdminHealthPanel` template (:139) and `templates/fragments/admin_health_panel.html` (sections: versions, counts, labels, providers, maintenance). `services/admin_health.rs` builds the data. `routes/admin.rs` is 1857 lines: new code goes in its own modules (Rule #12).

Practice A3: the thresholds (D5) and the panel's wording (D6) are settled with the project lead before the first review pass.

## Decisions

- **D1 — one in-memory store, no table, no write per request.** A new module `src/services/watch.rs` holds a process-wide `static WATCH: Watch` (`std::sync::OnceLock` or `LazyLock`, a `std::sync::Mutex` inside; never held across an `.await`). It counts four kinds (`WatchKind::{SlowStatement, PoolTimeout, CsrfRefusal, ApiKeyRefusal}`). Each kind has:
  - a total since start;
  - 24 hourly buckets, each stamped with its hour (UTC instant truncated to the hour), so the "last 24 hours" is the sum of the buckets whose hour is within the last 24 h, and an old bucket is reset when its slot is reused;
  - for the two refusal kinds, per-bucket client-address counts.

  A global store, not a field of `AppState`, because the `tracing` layer and the `From<sqlx::Error>` conversion cannot reach the state. The logic is a plain struct (`WatchStore`) with an injectable clock (`record_at(kind, client, now)`, `snapshot_at(now)`), so the window is unit-tested without sleeping. The static only wraps it. It is reset on restart, which the panel says (D6).
- **D2 — bounded memory.** Each bucket keeps at most `MAX_ADDRESSES_PER_BUCKET` = 64 distinct addresses; beyond that, refusals still count in the totals, and an `other` count shows how many fell outside. The top three are computed at read time over the 24 h buckets: most refusals first, then address in ascending order for ties (deterministic). The address is the `client_addr` string as logged, with no extra parsing.
- **D3 — where each kind is counted.**
  - Slow statements: a `tracing_subscriber::Layer` (`watch::SlowStatementLayer`) whose `on_event` counts events with target `sqlx::query`, level `WARN` and a message starting with "slow statement". It has its own per-layer filter (`Targets` for `sqlx::query` at `WARN`).
  - In `main.rs`, the operator's `EnvFilter` moves from a global layer to a **per-layer filter** on the two `fmt` layers. Keep one reload handle: the two output layers combined (`stdout.and_then(file)`), then `.with_filter(reload filter)`, or an equivalent that the admin "Log level" form still drives. That way the count does not depend on the log level, and the log output is unchanged (same filter, same layers). An integration-style unit test drives a fake sqlx event through a subscriber built the same way, with an `error` operator level, and sees it counted.
  - Pool timeouts: in `From<sqlx::Error> for AppError` when the error is `PoolTimedOut`. That is one place, and every handler and task goes through it. Check for a path that matches `PoolTimedOut` before the conversion; if one exists, count there too, and say so in the notes.
  - CSRF: in `csrf.rs::reject`, with its client address. API keys: in `api_key.rs::unauthorized` and `insufficient_scope`, with their client address.
  - Counting is a mutex lock and a few integer updates: no I/O, no allocation beyond a new address key, no database query (AC4).
- **D4 — the panel.** A new section "Production watching" on the Health tab, after the counts and before the providers. It is built in a new `routes/admin_health_watch.rs` (data + labels) and a new fragment `templates/fragments/admin_health_watch.html` included by the panel.
  - A four-row table: kind, last 24 hours, since start. Under the two refusal rows, the three most frequent client addresses over 24 h, each with its count (or "none").
  - The "since start" header names the start time, in the instance zone (`format_local_date_time`, story 15-1). Below the table: "Counted in memory since the last start; a restart resets them."
  - The panel is read on each Health tab render, with no polling.
- **D5 — thresholds (over the last 24 hours).** *(Approved by the project lead, 2026-11-25.)*
  - Slow statements above **50**. In normal use the instance has none; #696 produced about 1 000 a day.
  - Pool timeouts above **0**. A single one means requests waited 30 s and failed.
  - CSRF refusals above **20**. Expired sessions produce a few a day.
  - API-key refusals above **20**. A misconfigured agent retries far more often than that.

  These are constants in `watch.rs` (v1, not settings, like `RECENT_ACTIVITY_DAYS`), each locked by a unit test. A count above its threshold shows its cell in the warning colour (`text-amber-800 dark:text-amber-300`, `font-semibold`) **and** a text marker after the number ("— above the usual level", screen-reader visible): not colour alone (NFR12).
- **D6 — copy (four locales).** *(Approved by the project lead, 2026-11-25; EN and FR shown, DE and IT follow them.)*
  - Heading: "Production watching" / « Surveillance de la production ».
  - Rows:
    - "Slow database statements (over 1 s)" / « Requêtes lentes à la base (plus de 1 s) »
    - "Database connection waits that failed (30 s)" / « Attentes de connexion à la base échouées (30 s) »
    - "Requests refused for an invalid security token (CSRF)" / « Requêtes refusées pour un jeton de sécurité invalide (CSRF) »
    - "API requests refused (missing, wrong or insufficient key)" / « Requêtes API refusées (clé absente, fausse ou insuffisante) »
  - Columns: "Last 24 hours" / « Dernières 24 heures », "Since start (%{time})" / « Depuis le démarrage (%{time}) ».
  - Addresses: "Most frequent addresses:" / « Adresses les plus fréquentes : », "none" / « aucune », "and %{count} more not listed" / « et %{count} autres non listées » (the `other` count of D2).
  - Marker: "above the usual level" / « au-dessus du niveau habituel ».
  - Footnote: "Counted in memory since the last start; a restart resets them. The details are in the logs." / « Comptés en mémoire depuis le dernier démarrage ; un redémarrage les remet à zéro. Le détail est dans les journaux. »
- **D7 — out of scope.** Log shipping, alerts by mail, a metrics endpoint (#708 "out of scope"). The Prometheus blackbox probe on `/health` stays as it is. No history beyond 24 h.

## Acceptance Criteria

1. **AC1** — The Admin → Health tab shows the "Production watching" panel with the four kinds, their last-24-hours and since-start counts, the start time, and, for the two refusal kinds, the three most frequent client addresses over 24 h with their counts. A Librarian or an anonymous visitor never sees it: the Health tab's existing role gate applies.
2. **AC2** — The counts live in memory only: no table, no migration, no database write or read added to any request (D1, D3). The panel says they reset on restart.
3. **AC3** — Each count above its D5 threshold shows in the warning colour **with** its text marker; a count at or below it shows neither (unit test on the view, E2E on the marker).
4. **AC4** — Counting adds no database query to a request (by construction; noted in the review). Unit tests cover:
   - the 24-hour window: an event 23 h 59 min ago counts, one 24 h ago does not, and a reused hourly slot is reset;
   - the since-start total;
   - the top-addresses order (count descending, then address ascending) and the 64-address cap with its `other` count;
   - each threshold constant;
   - the slow-statement layer counting a `sqlx::query` WARN "slow statement" event even with an `error` operator filter, and ignoring other `sqlx::query` events.
5. **AC5** — The log output is unchanged by the filter move: same lines at the same level, and the admin "Log level" form still changes what is written. The existing log-level tests stay green, plus one test that a reload to `warn` drops an `info` line from the output layer.
6. **AC6** — E2E: an Admin reads the CSRF since-start count, provokes a CSRF refusal (a POST with a tampered token through `page.request`), reloads the Health tab and sees the count increased by at least 1. Other workers may add refusals, so the test asserts "at least" (Rule #23). An API-key refusal (a request with a wrong `Authorization: Bearer`) is seen the same way. The panel passes the axe check of the Health tab.
7. **AC7** — Gates: clippy (`-D warnings`), unit tests, the integration modules touched, `locale_parity`, tsc and the E2E gates; the full E2E suite with `CI=true --workers=2` has no flaky test.

## Tasks / Subtasks

- [ ] T1 — `src/services/watch.rs`: `WatchKind`, `WatchStore` (buckets, totals, addresses, cap), `snapshot_at`, thresholds, the global `WATCH` and `watch::record(kind, client)`; unit tests (AC4).
- [ ] T2 — `SlowStatementLayer` + the `main.rs` filter move (per-layer filter, one reload handle); tests (AC4, AC5).
- [ ] T3 — counting sites: `From<sqlx::Error>` (PoolTimedOut), `csrf.rs::reject`, `api_key.rs::unauthorized` / `insufficient_scope` (D3).
- [ ] T4 — `routes/admin_health_watch.rs` + `templates/fragments/admin_health_watch.html`, included in the Health panel; labels in four locales (D4, D6, AC1, AC3).
- [ ] T5 — E2E in the Health tab spec (or a new `admin-health-watch.spec.ts` with its own spec id if none fits) (AC6).
- [ ] T6 — gates (AC7); `CLAUDE.md`: a Key Patterns line (watch store, where counts are taken, per-layer log filter); user manual EN/FR (Health tab section: the panel, what each count means, thresholds, reset on restart). PDFs are rebuilt at release.

## Dev Notes

### Files

- New: `src/services/watch.rs`, `src/routes/admin_health_watch.rs`, `templates/fragments/admin_health_watch.html`.
- Changed: `src/main.rs` (logging setup :80-130, the reload handle stored in `AppState`, `lib.rs:35`), `src/error/mod.rs` (:291), `src/middleware/csrf.rs` (:308), `src/middleware/api_key.rs` (:117, :141), `src/routes/admin.rs` (Health panel: one include + one field, no logic), `src/services/mod.rs`, `src/routes/mod.rs`, `locales/{en,fr,de,it}.yml`, `docs/manual/{en,fr}/` (Health tab chapter).
- Look at how the log-level reload is applied (`src/main.rs:470-485`, the System tab handler) before moving the filter: its handle type changes.

### Guardrails

- No `.await` while the watch mutex is held; never poison-panic (recover the guard with `into_inner` on a poisoned lock).
- CSP: no inline style. Escape every address (it comes from request headers when a trusted proxy is configured).
- `t!()` with `locale = loc`; the four locale files.
- Couplings (Rule #24): 19-7 adds the backup status line to the same Health tab, starting from `main` after this story merges. Keep the panel's include self-contained.
- Heavy commands under `flock -w 10800 /tmp/station-heavy.lock mem-guard …`; never run `cargo fmt`.

### Previous stories (19-3, 19-4)

- Wait with `expectHtmxProcessed` before clicking a Confirm swapped into `#modal-slot` (#723).
- Specs with fixed data fail on `--repeat-each`; derive data from the spec's slot or reset the stack between repetitions.

### References

- Epic: `_bmad-output/planning-artifacts/epics.md`, Story 19.5 (:3709), FR178 (:207), UX-DR45 (:347). Issue #708. sqlx: `sqlx-core-0.8.6/src/logger.rs:93-145`.

## Validation (2026-11-25)

Checked against the code: sqlx 0.8.6 logs slow statements at target `sqlx::query`, `WARN`, message "slow statement: …" (`logger.rs:124-140`), and its enabled check also asks `tracing`, so a per-layer filter that enables that callsite is enough for it to fire. The log-level reload goes through the `LogLevelReloader` closure (`main.rs:471-485`); only its handle changes with the filter move, so the System tab handler stays as it is. `tracing-subscriber` 0.3 supports `reload::Layer` as a per-layer filter (`Handle::modify` rebuilds the callsite interest). D5 and D6 were approved by the project lead before development (practice A3).

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
