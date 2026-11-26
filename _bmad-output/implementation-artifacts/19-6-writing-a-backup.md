# Story 19.6: Writing a backup (#657, part 1)

Status: review

## Story

As the operator,
I want mybibli to write a complete, checkable backup on request,
so that the library can be recovered, or loaded elsewhere, without restoring all of MariaDB.

## Context

On 2026-11-09 the production database was erased by a mishandling. It was restored from the NAS nightly backup, and a day of entries was lost. mybibli has no backup of its own. The decisions recorded on #657 (2026-11-10, plus the epic 17 retro Q4 comment) are the specification. This story writes one backup on request.
- Story 19-7 adds the schedule, the rotation and the Health status line.
- Story 19-8 adds the restore and the list of backups.
- Story 19-9 adds the cover consistency tool.

Covers FR179 (format and writer) and NFR49 (round trip in CI).

What exists:
- **Database.** MariaDB 10.11 with 33 tables, all InnoDB (the server default; nine migrations also say it explicitly). The migrations are tracked in `_sqlx_migrations`. `contributors.name_live` is a generated column (story 19-3): a loader must skip it.
- **Covers.** `COVERS_DIR` (`main.rs:424`, default `./covers`, `/app/covers` in the image) holds `<digits>.jpg` covers, plus `.staging/` for proposed covers, which is excluded from backups (#657).
- **Image.** No `/app/backups` exists in the image (`Dockerfile`), and it must stay that way. mybibli never creates the folder; the operator mounts it.
- **Crates.** `sha2` and `rand` are already dependencies. No archive or compression crate is.
- **Admin.** The admin page has seven tabs (`AdminTab` in `routes/admin.rs:42`, a file of 1857 lines). A tab lives in its own `routes/admin_<name>.rs` (story 8-1 pattern).

Practice A3: the format (D2), the tab (D7) and the messages (D8) are settled with the project lead before the first review pass.

## Decisions

- **D1 — the folder.**
  - `MYBIBLI_BACKUP_DIR`, default `/app/backups` (in the image; `./backups` for `cargo run`), read once at startup.
  - mybibli **never creates it** (no `create_dir`, no `mkdir` in the Dockerfile).
  - If it is missing, not a directory, or not writable, a backup request is refused with D8's message, nothing is written anywhere, and the app keeps running.
  - The compose files get the mount `./backups:/app/backups`, commented with the reason. `tests/e2e/docker-compose.test.yml` mounts a folder that the E2E stack creates for the run.
- **D2 — the format: one `.tar.gz` file.** *(Approved by the project lead, 2026-11-25.)*
  - The tar stream holds, in order:
    1. `database.sql` — a standard SQL dump that the `mariadb` client loads into an empty database (`mariadb -u … newdb < database.sql`). It has a header (`SET NAMES utf8mb4; SET FOREIGN_KEY_CHECKS=0; SET UNIQUE_CHECKS=0;`), then for each table: `DROP TABLE IF EXISTS`, the `SHOW CREATE TABLE` text, and multi-row `INSERT`s of at most 500 rows. Generated columns are left out of the column list (`information_schema.COLUMNS.GENERATION_EXPRESSION IS NOT NULL` / `EXTRA` contains `GENERATED`). The dump ends by restoring the checks. It covers every table, `_sqlx_migrations` included, so that a loaded copy is at the right migration level. **Amended 2026-11-26 (review M2, project lead): `sessions` is dumped without its rows** — the definition only; its tokens are live credentials, and a restore logs everyone out anyway. The `INSERT`s also close at about 1 MiB (review M1).
    2. `covers/<name>` — every regular file directly in `COVERS_DIR` whose name is `<digits>.jpg` (Rule #20, positive shape: what the app writes; the shape 19-9 uses too). `.staging/` and any other entry are skipped and counted in the manifest.
    3. `manifest.json`, last — `format` (`"mybibli-backup"`), `format_version` (1), `instance_id`, `mybibli_version` (`CARGO_PKG_VERSION`), `last_migration` (highest `_sqlx_migrations.version`), `created_at` (UTC, RFC 3339), the row count of each table, the cover count and bytes, and `members`: the SHA-256 and size of each preceding member.
  - The manifest's own SHA-256 is written to a sidecar `<name>.sha256` (`sha256sum` format), so that the whole file can be checked with `sha256sum -c` and a truncated archive is detected even before it is opened. 19-8 verifies both.
  - Values are written as SQL literals built by the server, not in Rust:
    - text-like columns: `QUOTE(col)`;
    - binary, BLOB and JSON columns: `IF(col IS NULL, 'NULL', CONCAT('0x', HEX(col)))`;
    - numbers: `CAST` to text.
    
    The column kinds come from `information_schema.COLUMNS`. Every value survives a byte-exact round trip (AC5).
  - Crates: `tar` and `flate2` (both widely used; exact versions pinned in `Cargo.toml` / `Cargo.lock`; `cargo audit` green).
- **D3 — a consistent snapshot without a global lock.**
  - The dump reads on one dedicated pool connection, inside `SET SESSION TRANSACTION ISOLATION LEVEL REPEATABLE READ; START TRANSACTION WITH CONSISTENT SNAPSHOT, READ ONLY`. Every table is InnoDB, so readers and writers go on as usual, and no `LOCK TABLES` or `FLUSH TABLES WITH READ LOCK` is taken.
  - `SHOW CREATE TABLE` is read before the snapshot starts (DDL does not run while the app is up).
  - Covers are copied after the dump. A cover written in between may be in the archive without its row, or missing for a row: 19-9's consistency tool reports and repairs that, and the manual says so.
  - Rows are streamed (`fetch`, not `fetch_all`) into the gzip writer, so memory stays flat.
- **D4 — the name, the instance id and the permissions.**
  - The name is `mybibli-<instance_id>-<YYYYMMDD>T<HHMMSS>Z.tar.gz` (UTC). This positive shape is what 19-7's rotation will recognise.
  - `instance_id` is 16 lowercase hex characters, generated once per database by `data_repairs::run_once("19-6-instance-id", …)` into a new K/V setting `instance_id`. It is shown on the Health tab, under Versions, and is not editable.
  - The archive is written as `.tmp-<name>`, flushed and `sync_all`ed, then renamed. The sidecar is written the same way, after the archive's rename.
  - Both files are created with mode `0600` (`OpenOptions::mode`), never wider: they hold password hashes, API-key hashes and provider keys (#657).
  - On any failure, the temporary files are removed and nothing is renamed.
- **D5 — one backup at a time.** A process-wide guard (a `tokio::sync::Mutex` `try_lock`) refuses a second request while one runs (D8's message). The writer runs in a spawned task, owning its own connection, so a client disconnect does not abort it halfway. The HTTP answer waits for it. The catalogue is small (seconds); if the write takes longer than the request timeout, the answer says it is still running.
- **D6 — the writer API.** `services::backup::write_backup(pool, &BackupConfig) -> Result<BackupWritten, BackupError>`, with `BackupWritten { file_name, bytes, tables, rows, covers }` and `BackupError::{FolderMissing, FolderNotWritable, AlreadyRunning, Io, Database}`. Each error variant has `localized(loc)`. 19-7's scheduled task and 19-8's safety backup call the same function.
- **D7 — the Backups tab.** *(Approved by the project lead, 2026-11-25.)* A new admin tab "Backups" (`AdminTab::Backups`, `/admin?tab=backups`, `/admin/backups`, `routes/admin_backups.rs`, Admin only, story 8-1 pattern). In this story it shows:
  - the folder path and its state (present and writable / missing / not writable);
  - a "Back up now" button;
  - the answer in the tab's feedback region.
  
  The list of backups present (UX-DR46) comes with 19-8; the schedule setting with 19-7.
- **D8 — copy (Rule #9, four locales).** *(Approved by the project lead, 2026-11-25; EN and FR shown, DE and IT follow them.)*
  - Folder missing:
    - EN: "No backup was written: the backups folder %{path} does not exist. Mount a folder there (see the manual, “Backups”), then try again."
    - FR: « Aucune sauvegarde n'a été écrite : le dossier de sauvegarde %{path} n'existe pas. Montez un dossier à cet endroit (voir le manuel, « Sauvegardes »), puis réessayez. »
  - Folder not writable:
    - EN: "No backup was written: mybibli cannot write in %{path}. Give the container's user write access to that folder (see the manual), then try again."
    - FR: « Aucune sauvegarde n'a été écrite : mybibli ne peut pas écrire dans %{path}. Donnez au compte du conteneur le droit d'écrire dans ce dossier (voir le manuel), puis réessayez. »
  - Already running:
    - EN: "A backup is already being written. Wait for it to finish; it will show here."
    - FR: « Une sauvegarde est déjà en cours d'écriture. Attendez qu'elle se termine ; elle s'affichera ici. »
  - Failed (I/O or database):
    - EN: "The backup could not be written, so nothing was kept. Try again; if it keeps failing, the logs say why."
    - FR: « La sauvegarde n'a pas pu être écrite ; rien n'a été conservé. Réessayez ; si l'échec se répète, les journaux en donnent la raison. »
  - Success:
    - EN: "Backup written: %{file} (%{size}, %{titles} titles, %{covers} covers)."
    - FR: « Sauvegarde écrite : %{file} (%{size}, %{titles} titres, %{covers} couvertures). »
  - Tab and folder states: "Backups" / « Sauvegardes », "Back up now" / « Sauvegarder maintenant », "Folder: %{path} — ready" / « Dossier : %{path} — prêt », "— missing" / « — absent », "— not writable" / « — non accessible en écriture ».

## Acceptance Criteria

1. **AC1** — With the folder mounted, "Back up now" writes exactly one `mybibli-<id>-<UTC>.tar.gz` and its `.sha256` there, with mode `0600`, and answers D8's success message. Nothing else stays in the folder: no `.tmp-` file after success or failure.
2. **AC2** — The archive holds `database.sql`, `covers/…` (no `.staging`) and `manifest.json` (D2 fields). `sha256sum -c` passes on the sidecar, and each member matches its manifest hash. A test truncates the archive and shows the check fails.
3. **AC3** — With the folder missing or not writable, the request is refused with D8's message, nothing is written (the test lists the parent folder and the container's `/app`), and the app keeps serving. A second request during a backup is refused with "already running".
4. **AC4** — Writing does not block the catalogue: while a backup runs (slowed in the test with a large table or a hook), a title edit on another connection commits without waiting (integration test). The dump is the consistent state at its snapshot: a row inserted after the snapshot started is not in it.
5. **AC5 (NFR49)** — A round-trip integration test runs in CI:
   1. seed a database with every table non-empty where possible, including UTF-8 (accents, emoji), binary, JSON, NULLs, the generated column and covers;
   2. back up;
   3. load `database.sql` into a fresh database (multi-statement execution) and extract the covers;
   4. compare every table, row by row (`CHECKSUM TABLE` or a sorted dump of each), and every cover byte by byte.

   The test fails if any table is added to the schema without being dumped (it compares against `information_schema.TABLES`).
6. **AC6** — `instance_id` is created once, kept across restarts, and shown on the Health tab. The manifest carries it.
7. **AC7** — Only an Admin reaches the tab and the action (Librarian → 403 feedback, anonymous → login), and the action requires the CSRF token. E2E: an Admin backs up from a blank browser login and sees the success line naming the file; the refusal for a missing folder is covered by a route test (the E2E stack has the folder).
8. **AC8** — Manual (EN / FR), new "Backups" section:
   - adding the mount to `docker-compose.yml`, and that mybibli never creates the folder;
   - writing a backup;
   - what the file holds and that it is sensitive (`0600`, contents);
   - loading `database.sql` into another MariaDB, which is the Rule #20 corpus (#657 comment);
   - that a cover written during the backup is reconciled by the 19-9 tool.

   PDFs are rebuilt at release.
9. **AC9** — Gates: clippy (`-D warnings`), unit and integration tests, `sqlx prepare --check`, `cargo audit` with the two new crates, tsc and E2E gates; the full E2E suite with `CI=true --workers=2` has no flaky test.

## Tasks / Subtasks

- [x] T1 — `instance_id` setting via `run_once` + Health line (D4, AC6).
- [x] T2 — `services/backup/` (split by concern, Rule #12: `mod.rs` API + guard, `dump.rs` SQL writer, `archive.rs` tar/gzip + manifest + sidecar, `covers.rs`) (D2–D6).
- [x] T3 — `routes/admin_backups.rs` + `AdminTab::Backups` + template + nav of admin tabs (D7, AC7).
- [x] T4 — locales ×4 (D8).
- [x] T5 — Config `MYBIBLI_BACKUP_DIR`; compose files and the E2E compose mount (D1).
- [x] T6 — Tests: unit tests (literal building, name shape, manifest), integration tests `tests/backup_write.rs` (AC1–AC4), `tests/backup_round_trip.rs` (AC5), declared in `tests/integration.rs`; E2E `admin-backups.spec.ts` (new spec id).
- [x] T7 — Manual EN/FR, `CLAUDE.md` Key Patterns line, `docs/auth-threat-model.md` note (sensitive file), gates.

## Dev Notes

### Guardrails

- Never create the backup folder; never write outside it (temporary files live in it).
- Read-only snapshot transaction for the dump; no `LOCK TABLES`, no `FLUSH`.
- `deleted_at IS NULL` does **not** apply: a backup copies every row, Trash included (a deliberate exception, like `models/isbn_repair.rs`). Say so in the module doc.
- No user data in log lines beyond counts and the file name.
- File mode `0600` at creation (not a `chmod` afterwards).
- Heavy commands under `flock -w 10800 /tmp/station-heavy.lock mem-guard …`; never run `cargo fmt`; do not touch kesh's containers, ports or databases.

### Couplings (Rule #24)

- 19-7 builds on `write_backup`, the name shape and the Backups tab.
- 19-8 reads the manifest and sidecar.
- 19-9 reads the covers folder too; only 19-9 deletes, and only `<digits>.jpg`.

### References

- #657 (decisions + epic 17 retro comment), epic 19 Story 19.6 in `_bmad-output/planning-artifacts/epics.md`, FR179, NFR49, UX-DR46.

## Validation (2026-11-25)

Checked against the code and the stacks:
- **Covers.** They are written as `<digits>.jpg` directly in `COVERS_DIR`, and proposed covers as `<32 hex>.jpg` in `.staging/`: D2's positive shape keeps the first and skips the second.
- **E2E stack.** It has no host folder: `tests/e2e/docker-compose.test.yml` sets `COVERS_DIR=/tmp/test-covers`, and `scripts/e2e-reset.sh` runs `down -v` / `up --build`. D1 is made precise: the test compose mounts the backups folder as a `tmpfs` (`/app/backups`), which exists, is writable and needs nothing on the host or in CI. The production compose files get the bind mount `./backups:/app/backups`.
- **Round-trip test.** The Rust test database (root user) allows the test to `CREATE DATABASE` a second, empty schema for the load, and to drop it afterwards.

D2, D7 and D8 were approved by the project lead before development (practice A3).

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (`claude-opus-5-5`), BMAD dev-story workflow.

### Debug Log References

- `sqlx::raw_sql(…).execute(&mut *conn)` inside the spawned writer task failed the higher-ranked `Send` check ("implementation of `Executor` is not general enough"). Bisected to `begin_snapshot`; replaced by `conn.execute("…")` per statement (plain `&str` goes through the text protocol, which `START TRANSACTION` needs anyway).
- Route tests first asserted English copy: the seeded users read French (`users.preferred_language` default), so the route tests assert the FR copy.

### Completion Notes List

- **T1** — `services/instance_id.rs`: `ensure` (`run_once("19-6-instance-id")`, `INSERT IGNORE` of the K/V setting `instance_id`, 16 lowercase hex from 8 random bytes) and `read`; called at startup (non-fatal) and by every backup. Health tab, under Versions: `#health-instance-id` (a dash if the id is missing).
- **T2** — `services/backup/`: `mod.rs` (API, `BackupConfig` with the guard, `BackupError` + `localized`, `folder_state`, name shape, `create_private` 0600, cleanup of the temporary files, sidecar), `dump.rs` (schema read before a `REPEATABLE READ … WITH CONSISTENT SNAPSHOT, READ ONLY` transaction on a dedicated connection with `close_on_drop`, literals built by the server, generated columns left out, 500 rows per `INSERT`, rows streamed with `fetch`), `archive.rs` (tar + gzip, `HashingWriter`, `Manifest` format 1), `covers.rs` (`<digits>.jpg`, regular files only, the rest counted).
- **T3** — `routes/admin_backups.rs`, `AdminTab::Backups` (`/admin?tab=backups`, `GET`/`POST /admin/backups`), `templates/fragments/admin_backups_panel.html`. A refusal or a failure answers 200 with an error entry in `#admin-backups-feedback`, and the folder line (`#admin-backups-folder`) is refreshed out of band. `admin.rs` goes to 1883 lines (still under Rule #12).
- **T4** — keys `error.backup.*`, `admin.backups.*`, `admin.tabs.backups`, `admin.health.instance_id` in the four locales. EN and FR follow D8 word for word. DE and IT carry the same meaning, with „…“ / «…» quotes.
- **T5** — `MYBIBLI_BACKUP_DIR`, read once in `main.rs` and made absolute (`backup_dir_from_env`). Bind mount `./backups:/app/backups` in `docker-compose.yml` and `docker-compose.dev.yml`, with comments. `tmpfs: /app/backups` in `tests/e2e/docker-compose.test.yml`. `.env.example` entry. `/backups/` in `.gitignore`, because the files hold hashes. The Dockerfile is unchanged: no folder is created.
- **T6** — Unit tests are in each new module. Integration tests: `tests/backup_write.rs` (AC1–AC4, AC6, AC7 routes) and `tests/backup_round_trip.rs` (AC5, a fresh `mybibli_rt_<random>` schema dropped under `catch_unwind`), plus `tests/common/backup.rs`. E2E: `specs/journeys/admin-backups.spec.ts`, spec id `BK`.
- **T7** — Manual EN/FR: a new "Backups" / « Sauvegardes » section in chapter 7 (`sec:backups`, "Since 1.29.0") and a Backups item in the tab list of chapter 2. That list said "six tabs" although there were already seven; it now reads "eight tabs, among them". PDFs were not rebuilt. Also a `CLAUDE.md` Key Patterns line (plus the tab count in Source Layout) and `docs/auth-threat-model.md` § 5.13.
- Manual check on the E2E stack: the written file passes `sha256sum -c`, and its `database.sql` loads with the real `mariadb` client into an empty database (33 tables). That database was dropped afterwards.

**Deviations from the spec (smallest change consistent with the decisions):**
1. **Sidecar content.** D2 says the sidecar holds "the manifest's own SHA-256", but also that the whole file can be checked with `sha256sum -c` and that a truncated archive is detected before it is opened (AC2). Those requirements contradict each other: a `manifest.json` line cannot be checked by `sha256sum -c`, because no such file exists on disk. The sidecar therefore holds one `sha256sum` line for the whole archive, `<hex>  <name>.tar.gz`. The manifest stays covered, because it is inside the archive. The members are checked against the manifest's hashes.
2. **Dump header.** On top of D2's three `SET`s, the header writes `SET time_zone = '+00:00'` (the dump session also reads in UTC), so that `TIMESTAMP` values round-trip whatever the time zone of the session that loads them. It also writes `SQL_MODE = 'NO_AUTO_VALUE_ON_ZERO'`, which the footer restores, as mysqldump does.
3. **Scratch file.** A tar member needs its size before its bytes, so `database.sql` is streamed into `.tmp-<name>.database.sql` in the backups folder. That file is created 0600 and removed in every case. Memory stays flat.
4. **`BackupWritten`** also carries `titles`, the row count of `titles` with the Trash included, because the success message needs it.
5. **JSON detection.** MariaDB has no JSON data type, so a JSON column is recognised by its `json_valid(col)` check constraint. An empty binary value is written `''`, because a bare `0x` is not valid SQL.
6. **Default folder.** The default is `./backups`, which is `/app/backups` in the image (WORKDIR `/app`). It is made absolute at startup so that messages name the real path.
7. **Request timeout.** The app has no request timeout layer, so the answer always waits for the writer. D5's "still running" answer cannot happen and was not added; no new copy was needed.
8. **"Not a directory"** is reported with the "missing" message; access denied on the path is reported as "not writable".
9. **Unchanged.** D7 and D8 as approved. The panel has no intro text, since D8 gives none.

**Gates (2026-11-25):**
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --lib`: 1909 passed, 0 failed, 1 ignored.
- `cargo test --test integration`: full binary, 968 passed, 0 failed. This includes the 9 `backup_*` tests, `locale_parity::` and `every_test_file_is_declared`.
- `cargo sqlx prepare --check --workspace -- --all-targets`: OK. No typed query was added.
- `cargo audit`: no vulnerability. The 10 allowed warnings were already there; none concerns `tar` 0.4.46, `flate2` 1.1.11 or `filetime` 0.2.30.
- `npx tsc --noEmit`: OK. `waitForTimeout` grep gate: clean. `tests/e2e/vcode-literals.sh`: OK.
- E2E full run (`CI=true --workers=2`, freshly reset stack): 499 passed, 2 skipped (the already-skipped ones), 0 failed, 0 flaky (3.5 min); the new spec `admin-backups.spec.ts` passed in it.

### File List

- Cargo.toml, Cargo.lock
- .gitignore, .env.example, docker-compose.yml, docker-compose.dev.yml, tests/e2e/docker-compose.test.yml
- src/services/backup/mod.rs (new), src/services/backup/dump.rs (new), src/services/backup/archive.rs (new), src/services/backup/covers.rs (new)
- src/services/instance_id.rs (new), src/services/mod.rs
- src/routes/admin_backups.rs (new), src/routes/admin.rs, src/routes/mod.rs
- templates/fragments/admin_backups_panel.html (new), templates/fragments/admin_health_panel.html
- src/lib.rs, src/main.rs
- locales/en.yml, locales/fr.yml, locales/de.yml, locales/it.yml
- `AppState` literals (`backup: Default::default()`): src/routes/auth.rs, src/routes/title_lifecycle.rs, src/middleware/auth.rs, src/middleware/csrf.rs, src/middleware/locale.rs, tests/common/mcp.rs and 28 tests/*.rs files
- tests/backup_write.rs (new), tests/backup_round_trip.rs (new), tests/common/backup.rs (new), tests/common/mod.rs, tests/integration.rs
- tests/e2e/specs/journeys/admin-backups.spec.ts (new)
- docs/manual/en/07-backup-restore.tex, docs/manual/fr/07-backup-restore.tex, docs/manual/en/02-configuration.tex, docs/manual/fr/02-configuration.tex
- docs/auth-threat-model.md, CLAUDE.md
- _bmad-output/implementation-artifacts/sprint-status.yaml, this story file

## Senior Developer Review (AI)

### Review pass 1

**Date:** 2026-11-26. Two passes were run side by side:
- a manual pass over the dev commit (`b8f2fb3`), with no Medium+ finding;
- an independent pass that probed MariaDB 10.11.19 read-only (`max_allowed_packet`, the behaviour of `QUOTE()`, `EXTRA`, the CHECK constraints, engines, charsets, primary keys).

**Medium, fixed — M1: `INSERT`s were cut by row count only.**
- **The risk:** 500 rows of `titles` with long descriptions, `QUOTE` doubling the worst characters, could reach about 130 MB, and 500 cached metadata responses in hex about 6 MB. The `mariadb` client and the server both default to 16 MiB `max_allowed_packet`. A backup reported as written would then fail to load.
- **The fix:** `dump::InsertBatcher` closes a statement at 500 rows **or** once it reaches `INSERT_BYTE_BUDGET` (1 MiB), whichever comes first; a single larger row is written alone. A unit test covers small rows, big rows and a row above the budget.

**Medium, fixed by decision of the project lead (2026-11-26, option 1) — M2: the file held live session tokens.**
- **The risk:** `sessions.token` is the cookie value itself, stored in clear. A reader of a copy of the file could act as a signed-in user, an admin included, on the running instance while that session lived.
- **The fix:** `dump::ROWLESS_TABLES` = `["sessions"]`, so the definition is dumped but no row.
  - The round-trip test seeds a session, then asserts that the loaded table is empty and that the manifest count is 0.
  - The manual (EN/FR, "The file is sensitive") and threat model § 5.13 now say so.
  - D2 is amended above.

Low, fixed:
- **L1:** a server-side NULL literal (a value above `max_allowed_packet`, warning 1301) now fails the backup instead of being written as `NULL`.
- **L2:** temporary files left by an interrupted backup (`.tmp-` + this instance's name shape, Rule #20) are swept under the guard at the start of the next backup, with a unit test.

Low, deferred (issue to be filed):
- **L3:** blocking file I/O runs on the async worker (`sync_all`, `list_covers`, `rename`).
- **L4:** a cover listed as a regular file is read following symlinks (`O_NOFOLLOW`).
- **L5:** the round trip loads through sqlx (UTC session) rather than the `mariadb` client. It cannot see a missing `SET time_zone` or the packet limit, although the dev checked both by hand with the real client.
- **L6:** the AC4 test does not write during an open `fetch`.
- **L7:** a malformed `instance_id` row blocks backups (reachable only by a hand edit).
- **L8:** a copy loaded elsewhere keeps the same `instance_id` (to note for 19-7).
- **L9:** the manual names the 19-9 tool before it ships (same release).

Verification: `cargo clippy --all-targets -- -D warnings` is clean, `cargo test --lib backup` passes (22), and the integration tests `backup_` pass (9, round trip included).

### Review pass 2

**Date:** 2026-11-26. This was an independent re-review of the pass-1 fixes, as Rule #6 requires.

**Medium, fixed under the project lead's M2 decision — tokens in `pending_metadata_updates`.**
- **The risk:** `pending_metadata_updates.session_token` holds the same cookie value as `sessions.token`, so the file still carried live credentials after M2.
- **The fix:**
  - `dump::BLANKED_COLUMNS` = `[("pending_metadata_updates", "session_token")]`, so the column is dumped as `''` and the rows are kept.
  - The round trip asserts the same row count, `''` everywhere and the other columns equal, and that no session token appears in `database.sql`.
  - The threat model § 5.13, the manual (EN/FR) and `CLAUDE.md` now say "no session token".
  - Nothing visible changes.

Low, fixed:
- `InsertBatcher` checks the budget **before** adding a row, so a statement never passes the budget unless it holds one row only. Its unit test now expects 3 + 3 + 3 + 1 statements and asserts that each fits within the budget.
- A doc comment that pass 1 had moved went back onto `sidecar_path`.

Low, deferred (same issue as L3–L9):
- **L10:** two instances that share an `instance_id` (a copy loaded elsewhere, L8) and the same folder would sweep each other's temporary files.

Verification: clippy is clean, `cargo test --lib backup` passes (23), and the integration tests `backup_` pass (9).
