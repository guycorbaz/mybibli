# Story 19.3: Simultaneous actions on volumes and contributors (closes #541; #663 items 1–3)

Status: done

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
  - **Two reuses of one trashed label.** `reactivate_for_reuse` updates `WHERE id = ?` only (`models/volume.rs:207`): two adds that both read the row soft-deleted both reactivate it — the second moves it to its own title, wipes its fields again and writes a second audit row. `reactivate_for_reuse_in` adds `AND deleted_at IS NOT NULL`; 0 rows → the label went live meanwhile → the `LabelLive` answer of D2. That re-read must not use the transaction's snapshot (taken by the earlier plain `find_by_label_any_state_in`, it still shows the row trashed): read it with a locking read by primary key or after the rollback. Cover it in AC1's reuse test.
  - **Snapshot.** `title_is_live_in` is a locking read and opens no snapshot; keep every read before the INSERT locking, so the plain duplicate-resolution read after a 1062 sees the row the INSERT waited for (otherwise `find_by_label_any_state_in` returns `None` → `volume_label_race` instead of #585).
- **D2 — the same label for the same title answers #585.** `create_volume_in` no longer turns a live duplicate into `BadRequest(volume_duplicate)`: it returns the live volume (new outcome, e.g. `enum VolumeAdd { Created(VolumeCreation), LabelLive(VolumeModel) }`, or a `LabelLive` error variant — dev's choice, no string matching on messages), and `vcode_volume::add_to_title` answers it with `existing_label_response(pool, &existing, title_id, loc)` wrapped in `closing_modal(…, confirmed)` — the same answer as its own pre-check. This also gives the other-title race the #585 warning + link instead of `volume_duplicate`. `feedback.volume_duplicate` is removed from the four locale files if nothing else uses it (today only `services/volume.rs:151`).
- **D3 — `/catalog` title gone at the INSERT resets the context.** One helper in `routes/catalog.rs` (or a small `catalog_*` module if it grows; `catalog.rs` is 1 683 lines, Rule #12) builds the #441 answer — clear `current_title_id`, warning `feedback.volume_no_title`, OOB `context-banner` emptied, OOB `guide-strip` = `guide.initial` — used by both the #441 check and the `VcodeAdd::TitleGone` branch (DRY); the latter also applies `vcode_volume::closing_modal(…, form.confirmed)`.
- **D4 — one contributor per live name: a unique key, existing duplicates left alone.** Migration `migrations/2026112…_contributors_live_name_unique.sql`:
  - `name_live VARCHAR(255) AS (IF(deleted_at IS NULL, name, NULL)) PERSISTENT` — same character set / collation as `name` (do not declare another: `contributors.name` has no pinned collation — `initial_schema.sql:80`, never altered — it carries the table default, which a column added without `COLLATE` inherits too; the compose files set `utf8mb4_unicode_ci`). Verify in a test that `Hergé` / `hergé` / `Hergé ` collide, as `find_by_name_exact_in` matches them, and that `information_schema.COLUMNS` gives both columns the same `COLLATION_NAME`;
  - `legacy_duplicate_slot BIGINT UNSIGNED NOT NULL DEFAULT 0`, backfilled `= id` for every live contributor that is **not the lowest id** among the live rows of its name (under the column collation — the row `find_by_name_exact_in` returns, `ORDER BY id`) — so existing duplicates stay exactly as they are (merging is #591, out of this epic; no row, link or name is rewritten — Rule #20 does not apply because nothing visible changes). Order: add both columns, backfill, then the key. A multi-table self-join `UPDATE contributors c JOIN contributors o ON o.name = c.name AND o.deleted_at IS NULL AND o.id < c.id SET c.legacy_duplicate_slot = c.id WHERE c.deleted_at IS NULL` is valid on MariaDB 10.11 (error 1093 concerns a subquery on the target, and 10.3+ lifts even that);
  - `UNIQUE KEY uq_contributors_live_name (name_live, legacy_duplicate_slot)`. New rows have slot 0, so they collide with the oldest live namesake; soft-deleted rows (`name_live` NULL) never collide.
  - The migration must succeed on a database with duplicates (the production one has some or not; it cannot be inspected from here) and be a no-op-equivalent on one without.
  - A generated column cannot be written: every `INSERT INTO contributors` lists its columns (all do today). Hidden coupling (Rule #24): the backup writer / loader of stories 19-6+ must skip `name_live` and carry `legacy_duplicate_slot`.
- **D5 — one find-or-create, race-free by the key.** `ContributorModel` gets one `find_or_create_in(conn, name)`: read (`find_by_name_exact_in`) → if none, `INSERT INTO contributors (name) VALUES (?) ON DUPLICATE KEY UPDATE id = LAST_INSERT_ID(id)` → `last_insert_id()` is the new or the existing row → read that row **by primary key** with `LOCK IN SHARE MODE` (a current read: inside the acceptance transaction a plain read would use the older snapshot and miss a row committed by the other transaction; one table + primary key, Rule #22). A losing concurrent insert waits on the winner's uncommitted key, then resolves to it (or inserts if the winner rolled back). `ContributorService::find_or_create(_in)` keeps its validation and calls it; `metadata_fetch::add_author_contributor` calls `ContributorService::find_or_create` instead of its hand copy (DRY). The ODKU touches no column, so `updated_at` / `version` do not move — assert it in a test.
- **D6 — rename to a live namesake is refused** (copy confirmed by the project lead on 2026-11-25 — practice A3): `ContributorModel::update` maps MySQL 1062 (`db.number() == 1062`, as `models/mcp_skill.rs:95`, never a message match) to a new refusal; `update_details` returns `AppError::BadRequest(contributor.error.name_taken)` — field hint, terse (Rule #9): EN "Another contributor already has this name." / FR « Un autre contributeur porte déjà ce nom. » (+ de, it). It lands where `name_required` lands today (the feedback entry of `update_contributor`), in the **request locale** (`update_details` gains `loc`; the handler's `contributor.updated` / `error.internal` too). A rename that changes the name (under the column collation) also sets `legacy_duplicate_slot = 0` — assigned **before** `name` in the `SET` (MariaDB evaluates a single-table `SET` left to right), e.g. `SET legacy_duplicate_slot = IF(name = ?, legacy_duplicate_slot, 0), name = ?, …` — otherwise a legacy duplicate (slot = id) could be renamed onto any live name unrefused; a case-only fix of a legacy row keeps its slot. **No rename form exists today**: `POST /catalog/contributors/update` (`routes/mod.rs:180`) is posted by no template or script, so the E2E posts it through the logged-in page (`page.request.post` with the `X-CSRF-Token` from the meta tag) and asserts the refusal copy in the answer; adding a form is out of scope.
- **D7 — Trash restore of a contributor whose name is live again is refused** (copy confirmed by the project lead on 2026-11-25 — A3): `TrashService::restore` / `restore_with_conflicts_cleared` map a 1062 on `contributors` to `AppError::Conflict` with `trash.restore_contributor_name_taken` (operation error, Rule #9): EN "“{name}” was not restored: another contributor already has this name. Rename one of them, then restore it again." The row stays in the Trash. Any other table's 1062 keeps its current behaviour. The services are locale-less and do not know the name: they return a typed refusal (or take `loc` + name), and `routes/admin.rs::admin_trash_restore` words it with `entry.item_name` in `loc`; propagated with `?`, the `Conflict` lands in `#feedback-list` from the panel's Restore button and in the modal's error region from the conflict modal's Confirm (`restore_with_conflicts_cleared`, `ModalConfirmRetargetGuard`) — cover both in tests. The contributor restore also sets `legacy_duplicate_slot = 0`, so a legacy duplicate restored next to a live namesake is refused like any other.
- **D8 — out.** Reactivating a soft-deleted namesake instead of creating a row (#541 "Related"), merging existing duplicates (#591), #663 items 4–6.

## Acceptance Criteria

1. **AC1 (#663 item 2).** An add and a delete of the same title serialise: with the delete's lock held, the add waits; if the delete commits, the add answers "title gone" and no live volume references the trashed title; if the add commits first, the delete is refused (`title.delete_blocked_has_volumes`). Same for the label-reuse path. A DB integration test with two connections reproduces the race and fails on the old code.
2. **AC2 (Rule #22).** The add's lock is a share lock on one `titles` row by primary key; a lock-scope test shows an add on title A does not block the delete of title B (short `innodb_lock_wait_timeout`, as `an_acceptance_on_one_title_does_not_lock_proposals_of_another`), written with the query.
3. **AC3 (#663 item 1).** A new V-code whose INSERT loses to a concurrent post for the same title answers the #585 "already recorded" info (other title: the #585 warning + link), on `/catalog/scan` and `POST /title/{id}/volumes`; reproduced with two connections; fails on the old code.
4. **AC4 (#663 item 3).** On `/catalog`, a title deleted after the #441 check (e.g. while the CR #300 modal is open) and before the INSERT: the answer clears `current_title_id`, empties `context-banner`, resets `guide-strip`, and closes the modal when answering its Confirm.
5. **AC5 (#541).** Two concurrent find-or-creates of one new name — two title-page adds, an add and a metadata acceptance (inside its transaction), two background author fetches — leave one live contributor; reproduced with two connections; fails on the old code. A name differing only by case, accents or trailing spaces resolves to the same contributor, as before.
6. **AC6.** The migration applies on a database holding live duplicate names (they all stay live, unchanged) and on a clean one; afterwards a new row with an existing live name is impossible; a soft-deleted namesake does not block a new row.
7. **AC7 (D6, D7).** Renaming a contributor to a live namesake is refused with `contributor.error.name_taken` in the request language; restoring from the Trash a contributor whose name is live again is refused with `trash.restore_contributor_name_taken`, the row stays in the Trash. E2E for each refusal (Foundation Rule #3: a refusal is a user scenario; the rename one posts the route, no form exists — D6), plus an E2E for AC4. A legacy duplicate renamed onto a live name or restored next to one is refused too (D6, D7).
8. **AC8.** Four locale files, `locale_parity` green; `.sqlx` up to date (`cargo sqlx prepare --check --workspace -- --all-targets`); clippy clean; every new test file declared in `tests/integration.rs`.

## Tasks / Subtasks

- [x] T1 — `create_volume_in` in one transaction with the title share lock; `_in` variants of the volume model calls it uses (AC1, AC2, D1)
- [x] T2 — live duplicate → `LabelLive` outcome → `existing_label_response` in `add_to_title`; drop `volume_duplicate` if unused (AC3, D2)
- [x] T3 — `/catalog` title-gone helper shared by the #441 check and the `TitleGone` branch, with `closing_modal` (AC4, D3)
- [x] T4 — migration: `name_live`, `legacy_duplicate_slot` backfill, unique key (AC6, D4)
- [x] T5 — `ContributorModel::find_or_create_in` (ODKU + PK share-locked read); service and `metadata_fetch::add_author_contributor` use it (AC5, D5)
- [x] T6 — rename refusal (1062 → `contributor.error.name_taken`, request locale) (AC7, D6)
- [x] T7 — Trash restore refusal for contributors (AC7, D7)
- [x] T8 — locale keys ×4 (AC8)
- [x] T9 — DB integration tests: `tests/volume_add_races.rs`, `tests/contributor_live_name.rs` (declared in `tests/integration.rs`) (AC1–AC3, AC5, AC6)
- [x] T10 — E2E: catalog title gone at Confirm; rename refused (route posted, no form); Trash restore refused (AC4, AC7)
- [x] T11 — gates (below) and `CLAUDE.md` (a line under Key Patterns: contributor names unique among live rows + legacy slot; the add-volume share lock)

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
- Tests: existing `tests/volume_label_reuse.rs`, `tests/title_volume_add.rs`, `tests/scan_vcode_case.rs`, `tests/contributor_delete_modal.rs` must stay green — except `volume_label_reuse.rs::live_label_collision_names_the_owning_title`, which asserts the `BadRequest` naming the owner that D2 removes: rewrite it to the `LabelLive` outcome (the owner is named by `existing_label_response`), and adapt the `.volume()` call sites if the return type changes. `VolumeService::get_volume_title_name` loses its only caller (`services/volume.rs:149`): remove it if nothing else uses it. New files above. E2E: a new spec needs its own `specId` and a `VCODE_BLOCKS` entry (`tests/e2e/helpers/vcode.ts`); or extend `catalog-*` / contributor specs.

### Guardrails

- Rule #22: the only new locks are the title share lock (PK) and the contributor share-locked read (PK). No `FOR UPDATE` on a name, no join. InnoDB's own locks on `uq_contributors_live_name` come with any insert into a unique key (a hit takes a next-key lock — X for the ODKU — held to commit, i.e. to the end of a metadata acceptance); they are accepted, not new reads, and the acceptance's deadlock retry (`acceptance::is_deadlock`, 40001) covers a cycle.
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

Claude Opus 5.5 (claude-opus-5-5)

### Debug Log References

- Contributor E2E: the add answer lists every credit of the title, including the author the mock metadata fetch adds — the junction id is read from the remove button of the spec's own name.

### Completion Notes List

- **D1 / AC1–AC2.** `VolumeService::create_volume_in` runs in one transaction: `models::volume_move::title_is_live_in` (reused, not copied) → `VolumeModel::create_in`; on 1062 the #442 resolution runs on the same transaction (`find_by_label_any_state_in`, `count_loans_in`, `reactivate_for_reuse_in` with `AND deleted_at IS NOT NULL`; 0 rows → rollback, then `find_by_label` on the pool → `LabelLive`). Pool wrappers kept for `create` and `find_by_label_any_state` (tests use them); `count_loans` / `reactivate_for_reuse` had no other caller and became `_in` only.
- **D2 / AC3.** New `VolumeAdd { Created(VolumeCreation), LabelLive(VolumeModel) }` (+ `into_creation`). `vcode_volume::add_to_title` answers `LabelLive` with `existing_label_response` wrapped in `closing_modal` (shared helper `label_live_answer`, also used by the pre-check). `feedback.volume_duplicate` removed from the four locales; `VolumeService::get_volume_title_name` removed (no caller). `volume_label_reuse.rs::live_label_collision_names_the_owning_title` rewritten as `live_label_collision_returns_the_live_volume`.
- **D3 / AC4.** `catalog::title_gone_reset` builds the #441 answer for both the check and the `TitleGone` branch. **Deviation (small):** it applies `closing_modal(…, confirmed)` in both branches, not only `TitleGone` — the realistic case of AC4 (title deleted while the CR #300 modal is open) is caught by the Confirm request's own #441 check, which left the modal open before.
- **D4 / AC6.** Migration `20261125090000_contributors_live_name_unique.sql` as specified; the backfill also writes `updated_at = updated_at` so the ON UPDATE timestamp does not move on legacy rows (nothing visible changes — asserted by test). `name` / `name_live` collations checked equal via `information_schema`.
- **D5 / AC5.** `ContributorModel::find_or_create_in` (read → ODKU `id = LAST_INSERT_ID(id)` → PK `LOCK IN SHARE MODE` read). `ContributorService::find_or_create(_in)` keep validation and call it; `metadata_fetch::add_author_contributor` calls `ContributorService::find_or_create` (hand copy removed). `ContributorModel::create` / `create_in` removed (no caller left; a raw insert would bypass the find-or-create). The background-fetch path is covered through `ContributorService::find_or_create` (the function is private to the task module).
- **D6.** `ContributorModel::update` returns `ContributorUpdate { Updated, NotFound, NameTaken }` (1062 via the shared `db::is_duplicate_entry`, moved from `models/mcp_skill.rs`); `SET legacy_duplicate_slot = IF(name = ?, legacy_duplicate_slot, 0), name = ?, …`. `update_details` takes `loc`; the route words `contributor.updated` / `error.internal` in the request locale.
- **D7.** Restores of `contributors` also set `legacy_duplicate_slot = 0`. The services stay locale-less and keep their signatures: `trash::restore_refusal(table, &err) -> Option<RestoreRefusal>` reads the typed 1062 (error number, contributors only); `admin_trash_restore` words it with `entry.item_name` in `loc` as `AppError::Conflict`. **Deviation (key path):** the key is `admin.trash.restore_contributor_name_taken`, next to the other Trash restore keys (`admin.trash.restore_*`); there is no root `trash:` namespace. Copy as approved.
- **Race tests fail on the old code** (run once on an emulation of the old logic — pre-19-3 `create_volume_in` body: unlocked `TitleModel::find_by_id`, pool `VolumeModel::create`, pool resolution returning `BadRequest(volume_duplicate)` for a live label and an unguarded reactivation; old `find_or_create_in` read-then-plain-INSERT; migration replaced by a column-only stub): FAILED `an_add_waiting_on_a_title_delete_answers_title_gone` (the INSERT waited on the FK share lock, then committed a live volume on the trashed title), `a_label_reuse_waiting_on_a_title_delete_answers_title_gone`, `two_reuses_of_one_trashed_label_reuse_it_once` (moved to the second title), `a_label_losing_the_insert_to_the_same_title…` / `…_another_title…` / `the_title_page_answers…` / `the_catalog_scan_answers…` (got `volume_duplicate`), `the_catalog_resets_its_context_when_the_title_goes_at_the_insert`, `two_find_or_creates_of_one_new_name_leave_one_contributor` (no wait, two rows), `an_acceptance_resolves_a_name_committed_after_its_snapshot` (two rows), `an_add_waits_for_an_acceptance_that_created_the_name`, plus every migration / rename / restore test. Passing there as expected: `a_delete_waiting_on_an_add_is_refused` (delete side, unchanged) and the lock-scope test. `the_catalog_title_gone_check_closes_the_modal_on_confirm` fails on the old `catalog.rs` by construction (no `hx-trigger-after-swap`; the emulation did not revert catalog.rs).
- **Gates:** `cargo clippy --all-targets -- -D warnings` clean; `cargo test --lib` 1856 passed (then 3 unit tests added — `restore_set_clause`, `restore_refusal`, `VolumeAdd::into_creation` — green in a targeted run); `cargo test --test integration` 950 passed (incl. `locale_parity`, 34 new/rewritten tests); `cargo sqlx prepare --check --workspace -- --all-targets` OK (no typed query changed; 3317 DB migrated); `npx tsc --noEmit`, flake grep, `vcode-literals.sh` clean; E2E: after `e2e-reset.sh`, `CI=true --workers=2` over the new and affected specs (`catalog-phantom-volume-confirm`, `contributor-live-name`, `catalog-contributor`, `catalog-volume`, `scan-duplicates-existing-label`, `admin-trash-restore`): 29 passed, 0 flaky. Full suite not run (left to the lead).
- **E2E:** new `contributor-live-name.spec.ts` (specId `CU`, no volumes: rename refused through the route; Trash restore refused — its removed credit opens the conflict modal, the refusal lands in `[data-modal-error]`); `catalog-phantom-volume-confirm.spec.ts` gains the AC4 journey (PV slots 4–5, ISBN PV/3). The panel-button path of the restore refusal (`#feedback-list`) is covered by the route-level Rust test.
- **Out of scope, observed:** `catalog-contributor.spec.ts` "anonymous user cannot POST … (DB unchanged)" compares library-wide entity counts (Rule #23): it can flake whenever a parallel spec creates a title (seen once, as flaky, in a run over a non-reset stack; green on the clean run).

### File List

- `migrations/20261125090000_contributors_live_name_unique.sql` (new)
- `src/db.rs`
- `src/models/contributor.rs`
- `src/models/mcp_skill.rs`
- `src/models/volume.rs`
- `src/routes/admin.rs`
- `src/routes/catalog.rs`
- `src/routes/catalog_contributors.rs`
- `src/routes/vcode_volume.rs`
- `src/services/contributor.rs`
- `src/services/trash.rs`
- `src/services/volume.rs`
- `src/tasks/metadata_fetch.rs`
- `locales/en.yml`, `locales/fr.yml`, `locales/de.yml`, `locales/it.yml`
- `tests/integration.rs`
- `tests/volume_add_races.rs` (new)
- `tests/contributor_live_name.rs` (new)
- `tests/volume_label_reuse.rs`
- `tests/e2e/specs/journeys/contributor-live-name.spec.ts` (new)
- `tests/e2e/specs/journeys/catalog-phantom-volume-confirm.spec.ts`
- `CLAUDE.md`
- `_bmad-output/implementation-artifacts/19-3-simultaneous-actions.md`

## Senior Developer Review (AI)

**Date:** 2026-11-25 · **Outcome:** changes made — one Medium finding fixed; a re-review is due (Rule #6). Status stays `review`.

Layers: a manual adversarial pass over the whole diff, plus an Edge Case Hunter subagent with project access. Acceptance criteria were checked against D1–D8.

### Findings

- **[Medium] Fixed — the D1 reuse guard took a range lock and deadlocked two reuses of one label** (`src/models/volume.rs::reactivate_for_reuse_in`).
  - The statement was `UPDATE volumes … WHERE id = ? AND deleted_at IS NOT NULL`. MariaDB 10.11 plans it on `idx_volumes_deleted_at` (a range scan) when that index is estimated as cheap as the primary key, for example when the Trash holds one volume. `EXPLAIN` gives `range` on `idx_volumes_deleted_at`.
  - That plan next-key locks the trashed volumes up to the end of the index, against Rule #22. It deadlocks with the concurrent reuse D1 is meant to handle (1213 on the competitor), and it blocks any volume going to the Trash while the add runs.
  - **Fix:** the guard is now a locking read by primary key (`SELECT CAST(deleted_at AS DATETIME) … WHERE id = ? FOR UPDATE`), followed by `UPDATE … WHERE id = ?`. Both statements use the primary key only. The behaviour is unchanged: `false` means not reused (live, or purged).
- **[Medium] Fixed — the guard's 0-rows branch had no test.** D1 required this coverage. `two_reuses_of_one_trashed_label_reuse_it_once` resolves through the `!is_deleted` path, because its INSERT waits and the plain read then sees the row live. With the guard removed, every test stayed green.
  - New `a_reuse_losing_the_row_at_its_update_returns_the_live_volume`: the competitor holds the trashed row unchanged, the add reaches its reactivation and waits there, then the competitor reuses the label and commits. The add must return `LabelLive` and write no audit row.
  - It fails without the guard: the volume is moved to the second title. It also fails with the previous guarded UPDATE, on a 1213 deadlock.
  - New lock-scope test `a_reuse_of_one_label_does_not_lock_other_trashed_volumes`: it fails on the previous code with a 1205 lock-wait timeout.
- **[Low] Fixed — `a_find_or_create_inserts_when_the_competing_insert_rolls_back` asserted no wait.** It could pass by scheduling alone. It now checks that the second insert waits before the rollback.

### Not fixed (Low, to file as issues)

- A rename or Trash restore onto a name held only by legacy duplicates is not refused. This happens once the oldest (slot 0) namesake has been trashed or renamed, because the remaining live namesakes have slot = id. This is a limit of the accepted D4 design; merging the duplicates is #591.
- `find_or_create_in`'s fast path is a plain, unlocked read. Inside an acceptance's older snapshot, or racing `delete_contributor`'s count-then-delete, it can link a credit to a contributor that has just been trashed. This race existed before the story.
- A Trash restore's `UPDATE … WHERE id = ? AND deleted_at IS NOT NULL AND version = ?` (every table) can take the same `idx_*_deleted_at` range plan. This predates the story.
- A label reuse racing an auto-purge of the same trashed volume can deadlock. The INSERT's duplicate S lock is now held to the end of the transaction. The case is very rare, and the answer is "internal error", with nothing corrupted.
- The background author fetch now applies `MAX_CONTRIBUTOR_NAME_BYTES`: an author name over 255 bytes but within 255 characters is no longer added (warning logged).
- The race answers assume `innodb_snapshot_isolation=OFF`, the 10.11 default. On MariaDB ≥ 11.6, error 1020 would surface instead of 0 rows or a resolution. This needs revisiting with the engine bump.
- The `uq_contributors_live_name` ODKU hit takes an X next-key lock held to the end of a metadata acceptance. This was accepted in the Guardrails; there is no lock-scope test for it.

### Verification

- `cargo clippy --all-targets -- -D warnings` clean.
- `cargo test --test integration -- volume_add_races:: contributor_live_name:: volume_label_reuse:: title_volume_add:: scan_vcode_case:: volume_move::`: 69 passed.
- No typed query changed (`.sqlx` unaffected).
- The fix changes no E2E-visible copy or HTMX answer.

### Review pass 2: no Medium+ finding

**Date:** 2026-11-25. A full manual pass over `git diff main...HEAD` (code, migration, locales, Rust and E2E tests), with the first pass's fix (`reactivate_for_reuse_in`: PK `FOR UPDATE` read + PK `UPDATE`) read closely.

Checked and found sound:

- Lock order and scope: add = title S (PK) → label record; delete = title X (PK); move = volume X → title S; two adds S + S. No S→X upgrade on the title in any of them, and no trigger on `volumes`. The reuse guard locks one volume row by PK and changes no `label` entry, so two reuses of one label serialise without a cycle.
- Snapshots: every read before the INSERT is a locking read, so the plain duplicate lookup opens its snapshot after any lock wait. The reuse guard re-reads the row with a current read, and the lost-reuse answer reads it again after the rollback.
- Transaction boundaries: every early return rolls back (explicitly, or by dropping the transaction on `?`), and the audit row and the refreshed read come after the commit.
- Migration: `name_live` and `name` both take the table default collation, and the backfill join and the key compare names the same way. Trashed rows (NULL) never collide.
- HTMX answers: the title-gone reset closes the CR #300 modal on Confirm. The restore `Conflict` lands in `#feedback-list` from the panel and in the modal's error region from the Confirm (route test). Locale keys exist in all four files, and `volume_duplicate` is no longer used anywhere.
- Tests: the race tests wait on a real lock before the competitor finishes, so they fail without the fix.

Fixed in passing: the doc comment of `a_reuse_losing_the_row_at_its_update_returns_the_live_volume` still described the old `AND deleted_at IS NOT NULL` guard.

New Low findings (to file as issues):

- **Three-way insert deadlock on a new contributor name.** When the first inserter rolls back while two find-or-creates of the same name wait on its key, both waiters can deadlock (1213, the classic InnoDB duplicate-key case). This needs a rollback, so it is very rare. The background fetch logs a warning, and an acceptance retries on 40001.
- **An acceptance can now fail on a credit added meanwhile.** A metadata acceptance's `credits` set is read from its snapshot. If a title-page add links the same new name, with the same role, to the same title during the acceptance, the acceptance now resolves to that same contributor. Its credit INSERT can then hit the junction's duplicate instead of creating a second contributor. Result: the acceptance answers an error and stays pending, and retrying it works.
- **The migration backfill has no index on `name`.** The self-join is a nested loop over the live contributors. It takes seconds for a few thousand rows, at one boot only.

Verification: `cargo test --test integration -- volume_add_races:: contributor_live_name:: volume_label_reuse:: title_volume_add::` → 47 passed. No source change in this pass, so clippy was not re-run; the first pass's run stands. E2E not run (out of scope here); nothing E2E-visible changed.
