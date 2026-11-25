# Story 19.3: Simultaneous actions on volumes and contributors (closes #541; #663 items 1–3)

Status: ready-for-dev

## Story

As a librarian,
I want two actions racing each other to leave the catalog consistent,
so that no volume lives on a deleted title and no contributor exists twice.

## Context

**#663 (deferred Low findings of story 17-2), items 1–3** — the add-a-volume path shared by `/catalog/scan` and `POST /title/{id}/volumes` (`src/routes/vcode_volume.rs::add_to_title` → `VolumeService::create_volume_in`, `src/services/volume.rs:91`):

1. **Same new V-code posted twice for one title.** Both posts pass the caller's `VolumeModel::find_by_label` pre-check (`catalog.rs`, `title_volume_add.rs:122`) before either INSERT; the loser hits `uq_volumes_label`, `resolve_duplicate_label` (`services/volume.rs:130`) finds a live volume and answers `feedback.volume_duplicate` ("already assigned to <this title>, scan a different label") instead of the #585 answer (`routes/catalog_existing_label.rs::existing_label_response`: same title → "already recorded" info; other title → warning + link).
2. **Adding a volume vs deleting its title.** `create_volume_in` reads the title without a lock (`TitleModel::find_by_id`, pool), then inserts on the pool (`VolumeModel::create`, autocommit). `TitleService::delete_without_volumes` (`services/title.rs:28`) locks the title `FOR UPDATE`, counts live volumes, soft-deletes. Between the add's read and its INSERT a delete can commit → a live volume on a trashed title (the auto-purge then hard-deletes the title — FK failure or orphan handling). The InnoDB FK check of the INSERT share-locks the parent row, so with the old code the INSERT *waits* for the delete and then **succeeds** (the soft-deleted row still exists): the race is reproducible deterministically. The label-reuse path (`VolumeModel::reactivate_for_reuse`, #442) has the same window.
3. **`/catalog`: title gone at the CR #300 / INSERT step.** The #441 check in `handle_scan` (`routes/catalog.rs` ≈ l. 591–632) clears `current_title_id` and refreshes `context-banner` / `guide-strip`; the later `VcodeAdd::TitleGone` branch (≈ l. 652) answers `feedback.volume_no_title` only — the banner keeps the deleted title until the next scan. Also: when it answers the CR #300 modal's Confirm, the modal is not closed (`closing_modal` not applied, unlike every other answer of the arm).

Items 4–6 of #663 (audio on the title page, CR #300 Confirm replacing the feedback list, header counts after a metadata edit) are **out**: #663 stays open with them.

**#541 — duplicate contributors.** `contributors.name` has no unique key. Three find-or-create paths read by name then insert:

- `ContributorService::find_or_create` / `find_or_create_in` (`services/contributor.rs:29–57`) — title page "Add contributor" (`add_to_title`), metadata proposal acceptance (`services/metadata_acceptance.rs:454`, inside the acceptance transaction).
- **A hand copy** in `src/tasks/metadata_fetch.rs:495` `add_author_contributor` (raw `SELECT … LIMIT 1` + `INSERT`) — the background metadata fetch. This is the likeliest real race: two scans of a series by the same new author fetch in parallel.

`find_by_name_exact_in` compares with the column collation (`utf8mb4_unicode_ci`: case- and accent-insensitive, trailing spaces ignored) and returns the oldest live match. A locking read on the unindexed name would lock the scanned table (the D-11-12-13 lock problem) and Rule #22 forbids a range lock; `GET_LOCK` cannot be held to the commit of the acceptance transaction and does not fix its REPEATABLE READ snapshot. Hence D4.

Other writers of `contributors.name`: the rename (`ContributorModel::update`, `services/contributor.rs::update_details`, route `catalog_contributors.rs:196 update_contributor`) and the Trash restore (`services/trash.rs::restore` / `restore_with_conflicts_cleared`, generic `UPDATE … SET deleted_at = NULL`). Both can produce a second live namesake today.

## Decisions

- **D1 — add vs delete: share lock on the title by primary key (Rule #22).** `create_volume_in` runs in one transaction: `models::volume_move::title_is_live_in(&mut tx, title_id)` (`SELECT id FROM titles WHERE id = ? AND deleted_at IS NULL LOCK IN SHARE MODE`, already used by the move — reuse it, do not copy it; moving it to `models::title` with a re-export is fine) → `NotFound` when gone (→ `VcodeAdd::TitleGone`, unchanged contract); then the INSERT, and on a duplicate label the #442 resolution (`find_by_label_any_state`, `count_loans`, `reactivate_for_reuse`) **on the same transaction** (add `_in(conn, …)` variants; keep the pool wrappers only if other callers remain). Commit, then the best-effort `admin_audit` row and the refreshed read, as today. Lock order: title (S) → volume (insert / label record). The delete takes the title X first, so the two serialise: add first → the delete waits, counts the new volume, refuses; delete first → the add waits, reads the title gone, answers "title gone". No cycle with the move (volume X → target title S) nor with a second add (both S).
- **D2 — the same label for the same title answers #585.** `create_volume_in` no longer turns a live duplicate into `BadRequest(volume_duplicate)`: it returns the live volume (new outcome, e.g. `enum VolumeAdd { Created(VolumeCreation), LabelLive(VolumeModel) }`, or a `LabelLive` error variant — dev's choice, no string matching on messages), and `vcode_volume::add_to_title` answers it with `existing_label_response(pool, &existing, title_id, loc)` wrapped in `closing_modal(…, confirmed)` — the same answer as its own pre-check. This also gives the other-title race the #585 warning + link instead of `volume_duplicate`. `feedback.volume_duplicate` is removed from the four locale files if nothing else uses it (today only `services/volume.rs:151`).
- **D3 — `/catalog` title gone at the INSERT resets the context.** One helper in `routes/catalog.rs` (or a small `catalog_*` module if it grows; `catalog.rs` is 1 683 lines, Rule #12) builds the #441 answer — clear `current_title_id`, warning `feedback.volume_no_title`, OOB `context-banner` emptied, OOB `guide-strip` = `guide.initial` — used by both the #441 check and the `VcodeAdd::TitleGone` branch (DRY); the latter also applies `vcode_volume::closing_modal(…, form.confirmed)`.
- **D4 — one contributor per live name: a unique key, existing duplicates left alone.** Migration `migrations/2026112…_contributors_live_name_unique.sql`:
  - `name_live VARCHAR(255) AS (IF(deleted_at IS NULL, name, NULL)) PERSISTENT` — same character set / collation as `name` (do not declare another; verify in a test that `Hergé` / `hergé` / `Hergé ` collide, as `find_by_name_exact_in` matches them);
  - `legacy_duplicate_slot BIGINT UNSIGNED NOT NULL DEFAULT 0`, backfilled `= id` for every live contributor that is **not the oldest** live row of its name (under the column collation) — so existing duplicates stay exactly as they are (merging is #591, out of this epic; no row, link or name is rewritten — Rule #20 does not apply because nothing visible changes);
  - `UNIQUE KEY uq_contributors_live_name (name_live, legacy_duplicate_slot)`. New rows have slot 0, so they collide with the oldest live namesake; soft-deleted rows (`name_live` NULL) never collide.
  - The migration must succeed on a database with duplicates (the production one has some or not; it cannot be inspected from here) and be a no-op-equivalent on one without.
- **D5 — one find-or-create, race-free by the key.** `ContributorModel` gets one `find_or_create_in(conn, name)`: read (`find_by_name_exact_in`) → if none, `INSERT INTO contributors (name) VALUES (?) ON DUPLICATE KEY UPDATE id = LAST_INSERT_ID(id)` → `last_insert_id()` is the new or the existing row → read that row **by primary key** with `LOCK IN SHARE MODE` (a current read: inside the acceptance transaction a plain read would use the older snapshot and miss a row committed by the other transaction; one table + primary key, Rule #22). A losing concurrent insert waits on the winner's uncommitted key, then resolves to it (or inserts if the winner rolled back). `ContributorService::find_or_create(_in)` keeps its validation and calls it; `metadata_fetch::add_author_contributor` calls `ContributorService::find_or_create` instead of its hand copy (DRY). The ODKU touches no column, so `updated_at` / `version` do not move — assert it in a test.
- **D6 — rename to a live namesake is refused** (proposed copy, to confirm with the project lead — practice A3): `ContributorModel::update` maps MySQL 1062 (`db.number() == 1062`, as `models/mcp_skill.rs:95`, never a message match) to a new refusal; `update_details` returns `AppError::BadRequest(contributor.error.name_taken)` — field hint, terse (Rule #9): EN "Another contributor already has this name." / FR « Un autre contributeur porte déjà ce nom. » (+ de, it). It lands where `name_required` lands today (the feedback entry of `update_contributor`), in the **request locale** (`update_details` gains `loc`; the handler's `contributor.updated` / `error.internal` too).
- **D7 — Trash restore of a contributor whose name is live again is refused** (proposed copy, A3): `TrashService::restore` / `restore_with_conflicts_cleared` map a 1062 on `contributors` to `AppError::Conflict` with `trash.restore_contributor_name_taken` (operation error, Rule #9): EN "“{name}” was not restored: another contributor already has this name. Rename one of them, then restore it again." The row stays in the Trash. Any other table's 1062 keeps its current behaviour.
- **D8 — out.** Reactivating a soft-deleted namesake instead of creating a row (#541 "Related"), merging existing duplicates (#591), #663 items 4–6.

## Acceptance Criteria

1. **AC1 (#663 item 2).** An add and a delete of the same title serialise: with the delete's lock held, the add waits; if the delete commits, the add answers "title gone" and no live volume references the trashed title; if the add commits first, the delete is refused (`title.delete_blocked_has_volumes`). Same for the label-reuse path. A DB integration test with two connections reproduces the race and fails on the old code.
2. **AC2 (Rule #22).** The add's lock is a share lock on one `titles` row by primary key; a lock-scope test shows an add on title A does not block the delete of title B (short `innodb_lock_wait_timeout`, as `an_acceptance_on_one_title_does_not_lock_proposals_of_another`), written with the query.
3. **AC3 (#663 item 1).** A new V-code whose INSERT loses to a concurrent post for the same title answers the #585 "already recorded" info (other title: the #585 warning + link), on `/catalog/scan` and `POST /title/{id}/volumes`; reproduced with two connections; fails on the old code.
4. **AC4 (#663 item 3).** On `/catalog`, a title deleted after the #441 check (e.g. while the CR #300 modal is open) and before the INSERT: the answer clears `current_title_id`, empties `context-banner`, resets `guide-strip`, and closes the modal when answering its Confirm.
5. **AC5 (#541).** Two concurrent find-or-creates of one new name — two title-page adds, an add and a metadata acceptance (inside its transaction), two background author fetches — leave one live contributor; reproduced with two connections; fails on the old code. A name differing only by case, accents or trailing spaces resolves to the same contributor, as before.
6. **AC6.** The migration applies on a database holding live duplicate names (they all stay live, unchanged) and on a clean one; afterwards a new row with an existing live name is impossible; a soft-deleted namesake does not block a new row.
7. **AC7 (D6, D7).** Renaming a contributor to a live namesake is refused with `contributor.error.name_taken` in the request language; restoring from the Trash a contributor whose name is live again is refused with `trash.restore_contributor_name_taken`, the row stays in the Trash. E2E for each refusal (Foundation Rule #3: a refusal is a user scenario), plus an E2E for AC4.
8. **AC8.** Four locale files, `locale_parity` green; `.sqlx` up to date (`cargo sqlx prepare --check --workspace -- --all-targets`); clippy clean; every new test file declared in `tests/integration.rs`.

## Tasks / Subtasks

- [ ] T1 — `create_volume_in` in one transaction with the title share lock; `_in` variants of the volume model calls it uses (AC1, AC2, D1)
- [ ] T2 — live duplicate → `LabelLive` outcome → `existing_label_response` in `add_to_title`; drop `volume_duplicate` if unused (AC3, D2)
- [ ] T3 — `/catalog` title-gone helper shared by the #441 check and the `TitleGone` branch, with `closing_modal` (AC4, D3)
- [ ] T4 — migration: `name_live`, `legacy_duplicate_slot` backfill, unique key (AC6, D4)
- [ ] T5 — `ContributorModel::find_or_create_in` (ODKU + PK share-locked read); service and `metadata_fetch::add_author_contributor` use it (AC5, D5)
- [ ] T6 — rename refusal (1062 → `contributor.error.name_taken`, request locale) (AC7, D6)
- [ ] T7 — Trash restore refusal for contributors (AC7, D7)
- [ ] T8 — locale keys ×4 (AC8)
- [ ] T9 — DB integration tests: `tests/volume_add_races.rs`, `tests/contributor_live_name.rs` (declared in `tests/integration.rs`) (AC1–AC3, AC5, AC6)
- [ ] T10 — E2E: catalog title gone at Confirm; rename refused; Trash restore refused (AC4, AC7)
- [ ] T11 — gates (below) and `CLAUDE.md` (a line under Key Patterns: contributor names unique among live rows + legacy slot; the add-volume share lock)

## Dev Notes

### Two-connection race tests (pattern)

`#[sqlx::test(migrations = "./migrations")]`, `pool: MySqlPool`. Open a transaction `tx_a` on its own connection and take the competing lock first (e.g. `lock_title_for_delete_in(&mut tx_a, id)` for AC1; an uncommitted `INSERT INTO volumes` / `INSERT INTO contributors` for AC3 / AC5); `tokio::spawn` the code under test on the pool; check it is blocked (it has not finished after ~200 ms — `tokio::time::timeout` returns `Err`, no sleep-based assertion on success); finish `tx_a` (soft-delete + commit, or commit); await the task; assert the end state (no live volume on a trashed title / one live contributor / the outcome variant). Run the test once against the old code (stash the fix) and note in the completion notes that it failed there. Lock-scope tests: `SET SESSION innodb_lock_wait_timeout = 2` on the second connection (see `tests/proposal_accept_metadata_cover.rs:955–1000`).

### Lock facts used

- InnoDB locking reads (`FOR UPDATE`, `LOCK IN SHARE MODE`) read the latest committed row, not the transaction snapshot; a plain `SELECT` after a lock wait in a transaction that has not yet made a consistent read sees the commits it waited for (the comment on `count_live_volumes_in`, `models/volume_move.rs:173`, relies on this).
- An INSERT that hits a duplicate key held by an uncommitted row waits for it; on commit it fails with 1062, on rollback it succeeds.
- The FK check of `INSERT INTO volumes` share-locks the parent `titles` row: that is why the old code *waits* for the delete and then commits a volume on a soft-deleted title — the test can assert it deterministically.

### Files

- `src/services/volume.rs` (create, resolve duplicate), `src/models/volume.rs` (`create_in`, `reactivate_for_reuse_in`, `find_by_label_any_state_in`, `count_loans_in` or equivalent), `src/models/volume_move.rs` (`title_is_live_in` reused)
- `src/routes/vcode_volume.rs`, `src/routes/catalog.rs`, `src/routes/title_volume_add.rs` (only if the outcome type change reaches it)
- `src/models/contributor.rs`, `src/services/contributor.rs`, `src/tasks/metadata_fetch.rs`, `src/routes/catalog_contributors.rs`, `src/services/trash.rs` (+ its route's error mapping if needed)
- `migrations/` new file; `locales/{en,fr,de,it}.yml`; `.sqlx/` if typed queries change
- Tests: existing `tests/volume_label_reuse.rs`, `tests/title_volume_add.rs`, `tests/scan_vcode_case.rs`, `tests/contributor_delete_modal.rs` must stay green; new files above. E2E: a new spec needs its own `specId` and a `VCODE_BLOCKS` entry (`tests/e2e/helpers/vcode.ts`); or extend `catalog-*` / contributor specs.

### Guardrails

- Rule #22: the only new locks are the title share lock (PK) and the contributor share-locked read (PK). No `FOR UPDATE` on a name, no join.
- Rule #20: nothing rewrites stored data (the backfill sets a technical slot only).
- Error 1062 detection by `db.number()` (`models/mcp_skill.rs:90–95`), not by message text (the volume model's `"Duplicate entry"` match may stay as is — out of scope).
- `deleted_at IS NULL` in every new read; MariaDB gotchas (CLAUDE.md) for dynamic queries.
- No formatter (it reformats the whole repository): write in the neighbouring style.
- Station: every heavy command under `flock -w 10800 /tmp/station-heavy.lock mem-guard …`; Rust test DB on 127.0.0.1:3317 (`mybibli_rust_test`), E2E stack on 18080; never touch kesh containers, ports or databases.

### Gates

`cargo clippy --all-targets -- -D warnings`; `cargo test` (lib) and `cargo test --test integration` against 3317; `locale_parity::`; `cargo sqlx prepare --check --workspace -- --all-targets`; E2E: `./scripts/e2e-reset.sh` then `CI=true npx playwright test --workers=2` (full), `npx tsc --noEmit`, flake grep, `tests/e2e/vcode-literals.sh`.

### Previous story (19-2) intelligence

- One integration-test binary (#717): a new `tests/*.rs` must be declared in `tests/integration.rs` (`every_test_file_is_declared`), use `crate::common`.
- E2E: `expectHtmxProcessed` before typing into a swapped form; specs assert only on their own rows (Rule #23); V-codes from `specVcode`; a flaky test fails CI.

### References

- Issues #663 (items 1–3), #541; epic 19 § Story 19.3 (`_bmad-output/planning-artifacts/epics.md`); D-11-18-11 (`epic-11-autonomous-decisions.md`).
- `CLAUDE.md`: Rule #22, Volume move (13-8), Scan of an existing V-code (13-1, #585), Add a volume from the title page (17-2), Rule #9 / `docs/error-message-style.md`.

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
