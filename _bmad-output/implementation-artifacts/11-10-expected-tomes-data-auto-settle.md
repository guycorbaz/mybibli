# Story 11.10: Expected tomes — data and auto-settle at scan

Status: ready-for-dev

GitHub issue: part of #493 (MCP server — do NOT close it here). Covers: FR129 (expected tomes), FR141, FR145, AR29. Epic 11 — tenth story; blocks 11-11 (series page + wishlist, manual CRUD UI) and 11-12 (series proposals that create expected tomes). Related open issues: #519 (`/series` list gap counting), #521 (writes that do not bump `version` — `title_series` attach included), #522 (cover staging).

## Story

As a librarian,
I want a series to know the tomes I do not own,
so that its gaps have names and settle by themselves once I scan them.

## Existing-code reality check (code map, 2026-10-28, `main` 94ebdbe)

| Place | Today | Consequence |
|---|---|---|
| `migrations/` | last file `20261028120000_create_proposals.sql`; AR24 quartet; test DB `mariadb:10.11`; `series` / `titles` / `title_series` created in the initial schema **without** an explicit charset (CI server default) | new `20261028180000_create_series_expected_tomes.sql`, `utf8mb4` explicit; FKs on integer ids only, so charset mismatch with `series` is harmless |
| `title_series` | `UNIQUE (title_id, series_id, position_number)` — nothing prevents two titles at the same position; omnibus = one row per covered position; `TitleSeriesModel::assign(pool, …)` restores a soft-deleted row or inserts | auto-settle needs a transaction-scoped assign → refactor to `assign_in(&mut MySqlConnection, …)` + pool wrapper (Rule #1, no copy) |
| `services::series` | `assign_title` checks position ≥ 1 and ≤ total for a closed series; `agent_series_detail` feeds MCP `get_series` (11-8) | same position rules for expected tomes; `AgentSeriesDetail` gains `expected_tomes` |
| `src/mcp/tools/get_series.rs` | doc comment: "Expected tomes arrive in story 11-10" | add `expected_tomes` to the document + description |
| `routes/catalog.rs` (3093 lines — already above Rule #12) | ISBN scan arm calls `routes::wishlist::autolink_feedback_html` (CR #242) and appends it to the info / skeleton feedback; `create_title` (manual form) has no autolink | ONE helper `routes::catalog_isbn_banners::isbn_banners(pool, title_id, isbn, loc)` = wishlist autolink + expected-tome settle banner(s); the ISBN arm swaps its call for it (net zero lines), `create_title` appends it when the title has an ISBN |
| `services::title::create_manual` | stores `form.isbn` as typed (ISBN-13 expected, D-11-1-4) | settle matches the stored ISBN exactly; a non-13-digit value matches nothing |
| `utils::isbn10_to_isbn13`, `TitleService::validate_isbn13_checksum` | story 11-1 helper | ISBN normalisation for expected tomes reuses both |
| `services::auto_purge::PURGE_DELETION_ORDER` | children before parents; `validate_schema` checks every entry exists | add `series_expected_tomes` before `titles` / `series` (soft-deleted tomes purged after 30 days) |
| `services::trash::permanent_delete` (Trash panel) | hard-deletes a title / series after clearing an explicit child list (`title_series`, …) | no change needed: the tome FKs are `CASCADE` (series) and `SET NULL` (title), so a manual permanent delete is never blocked — covered by an integration test |
| `tests/seed_drift.rs::ENTITY_TABLES_WITH_SOFT_DELETE` | AR24 regression net | add `series_expected_tomes` |
| `tests/scan_title_context.rs` | drives `/catalog/scan` through `tower::oneshot` with real login + CSRF | pattern for handler-level settle tests |
| `tests/e2e/specs/journeys/mcp-gaps-tools.spec.ts` | E2E over `get_series` | assert the new `expected_tomes` key |

## Acceptance Criteria

1. **AC1 — migration (AR29, AR24).** `series_expected_tomes`: `id BIGINT UNSIGNED PK`, `series_id` (NOT NULL, FK `series(id)` **ON DELETE CASCADE** — a hard-purged series takes its tomes), `position INT NOT NULL`, `title VARCHAR(500) NOT NULL`, `isbn VARCHAR(13) NULL` (always ISBN-13 when set), `origin ENUM('manual','proposal') NOT NULL DEFAULT 'manual'`, `settled_title_id` (NULL, FK `titles(id)` **ON DELETE SET NULL**), AR24 quartet, a stored generated `live_marker` (1 when `deleted_at IS NULL`, else NULL) with `UNIQUE (series_id, position, live_marker)` and `UNIQUE (series_id, isbn, live_marker)` (at most one live tome per position and per ISBN in a series; soft-deleted rows never collide), indexes on `isbn`, `deleted_at`, `settled_title_id`. InnoDB, utf8mb4.
2. **AC2 — ISBN.** An ISBN given for an expected tome is trimmed, hyphens and spaces removed; a valid ISBN-10 (incl. `X`) is stored as ISBN-13 via `utils::isbn10_to_isbn13`; an ISBN-13 must pass the checksum; anything else is refused (`InvalidIsbn`). Empty → no ISBN.
3. **AC3 — service CRUD** (`services::expected_tomes::ExpectedTomeService`): `create` (+ `create_in` on a connection, for 11-17), `update` (optimistic lock on `version`), `delete` (soft, optimistic lock), `list_by_series` (live tomes of a live series, ordered by position), `find`. Validation: series live (`SeriesNotFound`); position ≥ 1, and ≤ total for a closed series (`PositionOutOfRange`); position not held by a live title of the series (`PositionHeld`); no other live tome at the position (`PositionTaken`) or with the same ISBN in the series (`DuplicateIsbn`) — checked in the service and backed by the unique keys; title trimmed, 1..=500 characters (`TitleRequired` / `TitleTooLong`). Update re-checks all of this excluding the tome itself, and refuses a settled tome (`Settled`). Typed `ExpectedTomeError` with agent-facing English `Display` and `From<ExpectedTomeError> for AppError` (validation → `BadRequest`, `NotFound` → `NotFound`, version mismatch / `Settled` → `Conflict`).
4. **AC4 — settled.** A tome is *settled* when `settled_title_id` names a live title. A tome whose settled title was soft-deleted (or purged — FK SET NULL) reads as unsettled again and can be settled again.
5. **AC5 — auto-settle (FR145).** `ExpectedTomeService::settle_for_title(pool, title_id, isbn)` runs whenever a title carrying an ISBN is catalogued — the `/catalog/scan` ISBN arm (new title **and** re-scan of an existing title, like the wishlist auto-link) and the manual title form (`POST /catalog/title`). In one transaction, for each unsettled live tome of a live series with that ISBN (locked `FOR UPDATE`), in id order:
   - title already assigned to the series at the tome's position → mark settled, no new row (`Settled`);
   - title assigned to the series at another position → no change (`Conflict::TitleElsewhere { position }`);
   - closed series whose total is now below the position → no change (`Conflict::OutOfRange { total }`);
   - position held by another live title of the series (single or omnibus) → no change (`Conflict::PositionHeld { holder }`);
   - otherwise → attach the title at that position (`TitleSeriesModel::assign_in`), mark the tome settled, bump its `version` (`Settled`).
   Returns one outcome per matching tome (series id + name, position). A failure never fails the scan: the handler logs it and shows no banner.
6. **AC6 — feedback (FR145).** Each outcome appends a feedback entry to the scan / manual-creation response: settled → success ("Expected tome found — the title was added to «series» at position N."); conflict → warning naming the series, the position and the reason, stating that nothing was attached (docs/error-message-style.md: what happened → why → what you can do). Text through `t!()` with keys in `locales/{en,fr,de,it}.yml`; user data HTML-escaped; CSP-clean (no inline style/script).
7. **AC7 — MCP `get_series` (completes 11-8).** The document gains `expected_tomes`: `[{ id, position, title, isbn, origin, settled_title_id }]` ordered by position then id (`settled_title_id` null unless settled per AC4); the tool description mentions it. Still read-only, still a thin adapter (no SQL under `src/mcp/`).
8. **AC8 — purge & drift.** `series_expected_tomes` is in `PURGE_DELETION_ORDER` before `titles` and `series`; it is in `tests/seed_drift.rs`; hard-purging a series removes its tomes; hard-purging a settled title unsettles the tome.
9. **AC9 — sqlx.** Dynamic queries; `deleted_at IS NULL` on every read of tomes, series, titles, title_series; `cargo sqlx prepare --check` green.
10. **AC10 — tests.** Unit: ISBN normalisation (ISBN-10 → 13, `X`, hyphens, bad checksum, empty), title validation, error → `AppError` mapping, banner rendering (escaping, one entry per outcome), `get_series` document shape. Integration `tests/expected_tomes.rs` (`#[sqlx::test]`): CRUD (create, list order, update, stale version, delete, recreate at a freed position), refusals (unknown / deleted series, position 0, above total, held, taken, duplicate ISBN, settled update), an ISBN-10 stored as ISBN-13, auto-settle nominal (row in `title_series`, tome settled), already-at-position, title-elsewhere, position-held conflict (single and omnibus holder), out-of-range, deleted series ignored, soft-deleted settled title → unsettled, FK cascade / SET NULL on hard delete; handler-level `/catalog/scan` and `POST /catalog/title` responses carrying the banner; `get_series` over MCP returning the tomes. E2E: `mcp-gaps-tools.spec.ts` asserts `expected_tomes` on `get_series` (no web route creates a tome before 11-11 — the scan-settle E2E is 11-11's own AC).

## Tasks / Subtasks

- [ ] T1 — Migration (AC1).
- [ ] T2 — `TitleSeriesModel::assign_in` refactor (behaviour-preserving) (AC5).
- [ ] T3 — Model `src/models/expected_tome.rs` (rows, insert/update/soft delete, locking reads, holder lookups) (AC1, AC3–AC5, AC9).
- [ ] T4 — Service `src/services/expected_tomes.rs` (`ExpectedTomeError`, `normalize_isbn`, `validate_title`, CRUD, `settle_for_title`) (AC2–AC5).
- [ ] T5 — `src/routes/catalog_isbn_banners.rs` + wiring in `catalog.rs` ISBN arm and `create_title`; locale keys ×4 (AC5, AC6).
- [ ] T6 — MCP `get_series` + `AgentSeriesDetail.expected_tomes` (AC7).
- [ ] T7 — Purge order, seed drift (AC8).
- [ ] T8 — Tests (AC10); E2E assertion in `mcp-gaps-tools.spec.ts`.
- [ ] T9 — Gates: clippy `--all-targets -D warnings`, full `cargo test` (test DB 3307), `cargo sqlx prepare --check --workspace -- --all-targets`, flake grep, `tsc --noEmit`, full Playwright `CI=true --workers=2` on a reset stack (port 18080).

## Dev Notes

- **Layering:** SQL in `models/expected_tome.rs` only; rules in the service; `src/mcp/tools/get_series.rs` maps service rows to JSON (AR27, `no_sql_under_src_mcp` audit).
- **Transactions:** create / update / settle lock the series row (`SELECT … FROM series WHERE id = ? AND deleted_at IS NULL FOR UPDATE`) so two writers on the same series serialise; the unique keys are the last line of defence (map `Duplicate entry` → `PositionTaken` / `DuplicateIsbn`).
- **Position held** = a live `title_series` row of a live title at that position in the series (`ts.deleted_at IS NULL AND t.deleted_at IS NULL`). Positions above a closed series' total are refused at creation (same rule as `assign_title`).
- **No version bump on series / titles** when settle attaches (same as every `title_series` write today — #521).
- **MariaDB gotchas:** TIMESTAMP via `CAST(… AS DATETIME)` if read in dynamic queries; `CAST(… AS CHAR)` around string expressions built with `COALESCE`/`IF` (CI default charset can return VARBINARY).
- **Generated column:** `live_marker TINYINT AS (IF(deleted_at IS NULL, 1, NULL)) STORED` — never written by the app; never `SELECT *`.
- Format only touched leaf files (`rustfmt --edition 2024 <file>`). Heavy commands under `mem-guard`, cargo `-j 2`.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 11.10] — FR129, FR141, FR145, AR24, AR29; 11.11 / 11.12 consumers.
- [Source: src/routes/wishlist.rs#autolink_feedback_html] — CR #242 mechanism FR145 mirrors.
- [Source: src/services/series.rs#assign_title], [Source: src/models/series.rs#TitleSeriesModel::assign] — position rules and assignment.
- [Source: src/services/proposals.rs] — typed-error / `_in` transaction conventions (D-11-9-2, D-11-9-11).
- [Source: _bmad-output/implementation-artifacts/epic-11-autonomous-decisions.md] — D-11-8-3/8 (gap rule), D-11-9-*; this story's choices are D-11-10-*.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

### Completion Notes List

### File List

## Change Log

- 2026-10-28 — story created (ready-for-dev).
- 2026-10-28 — validated against the create-story checklist: added the Trash permanent-delete path (FK actions make it safe), confirmed `POST /catalog/title` is the manual creation route, `admin_reset` touches no catalogue table.
