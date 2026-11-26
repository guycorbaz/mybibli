# Story 19.7: Scheduled backups and rotation (#657, part 2)

Status: ready-for-dev

## Story

As the operator,
I want backups to happen on their own and old ones to go away,
so that I always have a recent copy without filling the disk.

## Context

Story 19-6 writes one backup on request (`services::backup::write_backup`, Admin → Backups). This story adds three things:
- a schedule, set in Admin → System, that skips a backup when nothing changed;
- the rotation of old backups;
- a backup status line on the Health tab (UX-DR45).

Covers FR179 (schedule), FR180 and UX-DR45 (backup status). The decisions recorded on #657 are the specification: "Schedule" and "Retention (fixed in v1, configurable later)".

What exists (branch based on story 19-6):
- **Writer.** `write_backup(pool, &BackupConfig) -> Result<BackupWritten, BackupError>` (`src/services/backup/mod.rs`). It refuses at once with `AlreadyRunning` when the guard (`BackupConfig::guard`, a `tokio::sync::Mutex` taken with `try_lock_owned`) is held, and with `FolderMissing` / `FolderNotWritable` from `folder_state`. The work runs in a spawned task: the dump goes to a scratch file `.tmp-<name>.database.sql` through a `HashingWriter` (its SHA-256 is `sql_digest`), then `covers::list_covers`, then the archive, then the sidecar.
- **Name shape.** `backup_file_name(instance_id, at)` = `mybibli-<16 hex>-<YYYYMMDD>T<HHMMSS>Z.tar.gz` (UTC); `is_backup_name` checks it (positive shape, Rule #20); `sidecar_path` = `<name>.sha256`. The instance id is in the name, so the id of a file is read from its name.
- **Instance id.** `services::instance_id::{ensure, read, is_instance_id}`; K/V setting `instance_id`, shown on the Health tab.
- **The dump is deterministic.** Rows are read `ORDER BY` the primary key, the header holds no timestamp, `sessions` has no rows and `pending_metadata_updates.session_token` is blanked. Two dumps of an unchanged database are byte-identical (a table without a primary key would only make two dumps differ, never look equal when they are not).
- **Settings.** K/V rows, `AppSettings` in `src/config.rs` (re-validated by `load_from_db`), whitelist `routes/admin_system.rs::fetch_setting_rows` (a fixed `IN` list of placeholders, policed by fix #406, test in `tests/admin_system_integration.rs`), saves through `services::admin_system::{save_setting, reload_settings_cache}`. `save_setting` is an `UPDATE … WHERE version = ?` only: the row must exist, so a new setting ships with a seed migration (precedent: `migrations/20261104120000_settings_time_zone.sql`). The closest form precedent is the time zone: `routes/admin_system_time_zone.rs` + `fragments/admin_system_time_zone_form.html`, pre-rendered into `AdminSystemPanel` as `time_zone_form_html` and embedded with `|safe` in `admin_system_panel.html`; its route is in `routes/mod.rs`.
- **Background tasks.** `tasks/auto_purge_scheduler.rs` (warm-up, `MissedTickBehavior::Skip`, interval re-read after each run, poisoned lock recovered), spawned from `main.rs` (~line 588).
- **Time zone.** `crate::utils::local_time` (`to_local`, `local_date`, `format_local_date_time`, `LocalClock`), `AppState::time_zone()`.
- **Health tab.** `routes/admin.rs::admin_health_panel` (`AdminHealthPanel`, template `fragments/admin_health_panel.html`). Story 19-5 added its watching panel as a separate route module and fragment (`routes/admin_health_watch.rs`, `fragments/admin_health_watch.html`), included by the Health panel.
- **File sizes (Rule #12).** `routes/admin.rs` 1886 lines, `routes/admin_system.rs` 1789 lines. New handlers and builders go in new files; only one-line hooks go into these two.
- **Daily churn.** Two daily tasks write a row even when nobody uses the library: the anonymous-session purge updates a `settings` row, and the auto-purge writes an `admin_audit` row. The two run at different times of day, so an idle library changes once or twice a day. Page views by an anonymous visitor or a probe write only `sessions` rows, which the dump does not copy (and `sessions` has no `AUTO_INCREMENT`). The dump includes each table's `SHOW CREATE TABLE` text, which carries the `AUTO_INCREMENT=` counter: an insert that fails or an `ON DUPLICATE KEY` upsert can move it without changing a row. That only makes a dump look changed (a spare file), never the reverse.

Practice A3: the setting and its copy (D1), the skip rule (D3), the rotation (D4, D5) and the Health copy (D6) are settled with the project lead before the first review pass.

## Decisions

Decisions marked **awaiting approval** belong to the project lead. The others are technical, proposed by this story, and may be changed in review.

- **D1 — the setting.** *(Awaiting approval.)*
  - K/V key `backup_interval_hours`, an integer; `AppSettings::backup_interval_hours: u32`; accessor `AppState::backup_interval_hours()`.
  - Allowed values (positive shape): `0` (off), `1`, `2`, `3`, `4`, `6`, `8`, `12`, `24`. On load, any other value falls back to the default with a `warn` (like the other settings). On save, any other value is refused next to the field.
  - Default: **`1` (every hour)**. *Options:* A — 1 h (recommended: the hourly tier of the rotation exists for it, and the skip rule makes an unchanged hour cost a dump and no file); B — 24 h; C — off until the operator turns it on (but #657 wants a backup without operator action, once the folder is mounted).
  - *Validator note.* With option A, every hour runs a full dump and writes it to a scratch file in the backups folder, even when nothing changed (see the D3 note on disk spin-down). The default and D3's cost go together.
  - Place: a new section "Backups" at the end of Admin → System, one `<select>` and a Save button, its own form and endpoint `POST /admin/system/backups` (story 8-5 pattern, hidden `*_version` field, optimistic lock). The handler lives in a new `routes/admin_system_backups.rs` (like `admin_system_time_zone.rs`); `admin_system.rs` only gets the whitelist key, two `AdminSystemPanel` fields (section heading, pre-rendered form) and the call that renders the form; the route goes in `routes/mod.rs`. *Option:* put the select on the Backups tab instead (closer to the button), which amends FR179's "Admin → System"; not recommended.
  - Copy (EN / FR; DE and IT follow them):
    - Section: "Backups" / « Sauvegardes »
    - Label: "Automatic backup" / « Sauvegarde automatique »
    - Options: "Off" / « Désactivée » — "Every hour" / « Toutes les heures » — "Every %{n} hours" / « Toutes les %{n} heures » — "Once a day" / « Une fois par jour »
    - Help: "mybibli checks at this interval and writes a backup into the backups folder only if something changed since the last one. Each backup is kept 24 hours, then the last one of each day is kept 14 days." / « mybibli vérifie à cet intervalle et n'écrit une sauvegarde dans le dossier de sauvegarde que si quelque chose a changé depuis la précédente. Chaque sauvegarde est gardée 24 heures, puis la dernière de chaque jour est gardée 14 jours. »
    - Saved: "Automatic backup saved: %{interval}." / « Sauvegarde automatique enregistrée : %{interval}. »
    - Refusal (a value not in the list, only by a tampered form): "Choose one of the listed intervals." / « Choisissez l'un des intervalles proposés. »
- **D2 — when a check runs.** *(Technical.)*
  - A task `tasks/backup_scheduler.rs` wakes every 60 s (`tokio::time::interval`, `MissedTickBehavior::Skip`: a clock jump never fires a burst; at most one check per wake). It reads the setting at each wake, so a change applies within a minute.
  - A check is due when the setting is not off and `now >= base + interval`, where `base` is the later of: the time of this process's last check, and the time stamped in the newest own backup name in the folder. With no base (fresh folder, first start), the check is due at once. A stamp later than now (clock skew, a clock that jumped back) is ignored for the base; otherwise one stray future-stamped file would stop the schedule until the clock reaches it. A name whose stamp does not parse as a real UTC time (`is_backup_name` checks digits only, e.g. month `13`) is ignored too.
  - The first wake comes after a **10-minute warm-up** (after the startup purge and the first requests).
  - A check is: write a backup unless unchanged (D3), then rotate (D4). Each check records its time and outcome (D7).
  - `AlreadyRunning` (a manual backup is being written, or a rotation is running): no check is recorded; the check stays due and is tried again at the next wake.
  - Folder missing or not writable: nothing is written, nothing is created, no rotation runs (a read-only folder would otherwise log a failed deletion per file per check), the task keeps running. One `warn` when the folder becomes unusable, one `info` when it is usable again (not one line per hour). The Health tab shows the warning (D6).
  - A failed write (`Io`, `Database`): the writer already logs it and leaves nothing; the check is recorded as failed, and the next one comes one interval later.
- **D3 — skipping when nothing changed.** *(Awaiting approval.)*
  - Positive rule: a scheduled check **skips** only when all of these hold:
    1. this process remembers the fingerprint of a backup it wrote (scheduled or manual);
    2. that backup's archive is still in the folder and is still the newest own backup;
    3. the new dump's SHA-256 equals the remembered one;
    4. the new cover list equals the remembered one: the sorted `(name, size, modification time in nanoseconds)` of every file `covers::list_covers` keeps.

    In every other case it writes. A false "changed" costs one file, which the rotation removes; a false "unchanged" would lose data. The rule errs on writing.
  - The comparison happens after the dump and the cover listing, before the archive is built. On "unchanged", the scratch dump is removed and nothing else is written.
  - The fingerprint is kept in memory only. Storing it in the database would change the next dump.
  - Consequences: the first check after a restart always writes; each check runs a full dump (seconds for this catalogue); an idle library still gets about one backup a day (the two daily task rows, see Context), which the rotation keeps at one per day.
  - *Options:* A — the rule above (recommended); B — the same with a SHA-256 of each cover instead of size + time (reads every cover at each check); C — per-table `COUNT(*)` / `MAX(updated_at)` (rejected: not sound, several writes bump no `updated_at`: `titles.detected_tome`, the one-time repairs, #521's cover writes).
  - *Validator note.* The rule is sound: the dump is the whole database (Trash, settings, schema text included), so an equal SHA-256 means equal data; the only blind spots are outside the dump, and covers are covered by condition 4. Two suggestions, for the lead:
    - Condition 2 checks the remembered file by name only. Also comparing its size to the remembered size costs one `stat` and catches a file replaced by hand under the same name.
    - Cost on a NAS: each unchanged hour still writes the full dump to a scratch file **in the backups folder**, then deletes it. On a NAS whose disks spin down, that wakes them every hour. An alternative (call it D): first hash the dump into a sink that writes nothing; only when it differs, run the normal write (a second snapshot, whose fingerprint is the one remembered). It costs a second dump only when something changed. Worth weighing together with D1's default.
- **D4 — rotation.** *(Awaiting approval.)*
  - A pure function `services::backup::rotation::plan(names, instance_id, now_utc, tz) -> Vec<String>` returns the archives to delete. It is unit-tested over synthetic names and a clock.
  - **Candidates:** only names where `is_backup_name` holds and the id part equals this instance's id. Nothing else is ever deleted: other names, foreign backups (D5), temporary files (the writer's own sweep handles them).
  - **Kept**, among the candidates:
    1. the newest one, whatever its age (with the skip rule, a library untouched for three weeks must keep its last backup);
    2. every one stamped less than 24 hours before now (and any stamped in the future: clock skew);
    3. for each of the 14 most recent local days (today and the 13 before, in the instance time zone), the newest one of that day.
  - **Deleted:** every other candidate. Its sidecar `<name>.sha256` is deleted after it; if the archive cannot be deleted, the sidecar stays. A sidecar of an own backup name whose archive is absent is deleted too (it checks nothing).
  - **Manual backups** have the same name shape and are rotated the same way. *Options:* A — rotated (recommended: a manual backup is a backup; it can be the last of its day); B — kept forever, which needs a different name shape and changes 19-6's D4.
  - **When:** after each scheduled check (written or unchanged), and after each successful manual backup. With the schedule off, only after manual backups. The rotation takes the guard with `try_lock`; if the guard is held, it is skipped (the next pass does it), so it never deletes a file that 19-8 is reading.
  - One `info` line per deleted file (its name), one summary line with the counts.
  - *Option on "14 days":* A — today and the 13 previous local days (recommended, as above); B — 14 days counted after the 24-hour window.
  - *Validator note.* (a) Rule 1 keeps "the newest one". With a future-stamped stray file, the newest is that stray, and the real last backup is then protected only by the 24 h and 14-day tiers: with the schedule off and no manual backup for 15 days, the real last backup would go. Suggest: rule 1 keeps the newest one stamped at or before now; rule 2 still keeps future ones. (b) Candidates should also be regular files (`DirEntry::file_type`, which does not follow links, as `list_covers` and the temporary-file sweep do), and a name whose stamp does not parse as a real time is never a candidate. These two are written into the guardrails and AC5 below, since they only narrow what can be deleted.
- **D5 — foreign backups.** *(Awaiting approval.)* #657 says a backup this instance did not write "escapes rotation until it has been restored". Story 19-8 (restore) does not exist yet. *Recommendation:* 19-7 never deletes a backup of another instance id, at all. Story 19-8 decides what "returned to normal handling" means after its restore (#657: deleted, or rotated). *Option:* none safer is possible before 19-8.
- **D6 — the Health status line (UX-DR45).** *(Awaiting approval for the copy.)*
  - A "Backups" block on the Health tab, after the watching panel. Built in a new `routes/admin_health_backup.rs`, rendered by `fragments/admin_health_backup.html`, included by the Health panel (19-5's pattern).
  - Times are in the instance time zone (`format_local_date_time`).
  - Lines (EN / FR; DE and IT follow them):
    - Last backup: "Last backup: %{when} (%{file})" / « Dernière sauvegarde : %{when} (%{file}) »; none: "Last backup: none yet" / « Dernière sauvegarde : aucune pour l'instant »
    - Next check: "Next check: around %{when}. A backup is written only if something changed." / « Prochaine vérification : vers %{when}. Une sauvegarde n'est écrite que si quelque chose a changé. »
    - Schedule off: "Automatic backup is off (Admin → System)." / « La sauvegarde automatique est désactivée (Admin → Système). »
    - Last check skipped: "Last check: %{when}; nothing had changed." / « Dernière vérification : %{when} ; rien n'avait changé. »
    - Folder missing (warning colour **and** the text "Warning:"): "Warning: the backups folder %{path} does not exist, so no backup is written. Mount a folder there (see the manual, “Backups”)." / « Attention : le dossier de sauvegarde %{path} n'existe pas ; aucune sauvegarde n'est écrite. Montez un dossier à cet endroit (voir le manuel, « Sauvegardes »). »
    - Folder not writable: "Warning: mybibli cannot write in the backups folder %{path}, so no backup is written. Give the container's user write access to it (see the manual)." / « Attention : mybibli ne peut pas écrire dans le dossier de sauvegarde %{path} ; aucune sauvegarde n'est écrite. Donnez au compte du conteneur le droit d'y écrire (voir le manuel). »
    - Last automatic backup failed: "Warning: the automatic backup at %{when} failed, so nothing was kept. The logs say why." / « Attention : la sauvegarde automatique de %{when} a échoué ; rien n'a été conservé. Les journaux en donnent la raison. »
  - The folder warning shows whenever the folder is unusable, schedule on or off (a manual backup needs it too).
  - "Next check" is the due time of D2 (or "in a minute or less" when already due: "Next check: in a minute or less." / « Prochaine vérification : d'ici une minute. »).
- **D7 — where the state lives.** *(Technical.)*
  - **Last backup** is read from the folder at each Health render: the newest own backup name (stamp parsed from the name). It survives a restart and also counts manual backups.
  - **The rest** (last check time and outcome, the remembered fingerprint and its file name, whether the folder was usable at the last wake) is in memory, in a shared `BackupStatus` (`Arc<std::sync::Mutex<…>>`, never held across an `.await`, poison recovered) added to `BackupConfig`, so every clone in `AppState` sees it. It resets on restart; the Health line then shows only the folder's last backup until the first check.
  - No table, no database write.
  - `write_backup` keeps its signature and records the fingerprint of what it wrote, together with the file name and its size in bytes. It records it only once the archive and its sidecar are renamed into place; a failed write leaves the previous fingerprint as it was (that file is still the newest, and still matches it). The scheduled path calls a new `write_backup_if_changed(pool, cfg) -> Result<CheckOutcome, BackupError>` with `CheckOutcome::{Written(BackupWritten), Unchanged}`; both share the writer's stages (dump → compare → archive), so 19-6's guarantees (guard, `0600`, temporary files, cleanup) stay in one place.
- **D8 — issue #729, L8 and L10.** *(Awaiting approval.)* L8: a copy loaded on another instance keeps the same `instance_id`. L10: two instances sharing an id and a folder sweep each other's temporary files. With rotation, L10 grows: such instances would also rotate each other's backups. *Recommendation:* 19-7 does not change the id rules. 19-8 keeps the local `instance_id` across a restore (it rewrites the row after loading a backup, so a restored foreign backup does not make this instance take the other's id). The manual (19-11) says: one backups folder per instance, and a dump loaded into a test database must not run against the production folder. A comment on #729 records this split. *Option:* 19-7 adds a startup check that refuses to rotate when the folder holds own-id backups this process cannot account for; not recommended (it cannot tell a restart from a twin).
- **D9 — the E2E stack and the scheduler.** *(Technical.)* The E2E stack keeps the default schedule. The 10-minute warm-up is longer than a CI run after a reset; on a long-lived local stack, a scheduled check holds the guard a few seconds per interval, so `admin-backups.spec.ts` could meet "already running" very rarely. No environment variable is added. *Option:* `MYBIBLI_SKIP_SCHEDULED_BACKUPS=1` in the test compose (strict accept-set, like `MYBIBLI_SKIP_SETUP`); not recommended, since the E2E Health line would then not show the real schedule.
  - *Validator note.* The last CI run on `main` (2026-11-25) took 18 minutes for the Playwright job, build and stack start included; the suite itself may well cross the 10-minute warm-up. CI sets `failOnFlakyTests`, so one "already running" answer in `admin-backups.spec.ts` (or in this story's spec) fails the job. Before keeping D9 as is, the dev should either measure the suite against the warm-up or make the specs' "Back up now" step retry once on the "already running" answer (a real user scenario, not a flake).

## Acceptance Criteria

1. **AC1 — the setting.** Admin → System shows the "Backups" section (D1) with the current value selected. Saving a listed value stores it, reloads the cache and answers D1's saved line. A value outside the list is refused next to the field, nothing is stored. A stale version answers the usual conflict. The key is in `fetch_setting_rows`; a value in the database outside the list loads as the default with a `warn`. Four locale files (`locale_parity::`).
2. **AC2 — the schedule.** With a usable folder and a due check, the task writes a backup, then rotates (D2). The check is due per D2's rule (unit test of the pure due-time function, including "no base" and a restart with a recent backup in the folder). A setting change applies at the next wake. With the setting off, no check runs. A check meeting `AlreadyRunning` is retried at the next wake, not one interval later. A future-stamped or unparsable name in the folder does not delay the check. Tests call the check function directly (e.g. `run_check(&state, now)`) and the pure due-time function with a given clock; no test waits for the warm-up or a wake.
3. **AC3 — missing folder.** With the folder missing or not writable, a check writes nothing anywhere (the test lists the parent folder), creates nothing, and the task keeps running; one `warn` on the change of state. The Health tab shows D6's warning.
4. **AC4 — skip when unchanged (D3).** Integration tests on a temporary folder:
   1. two checks with nothing changed → one archive;
   2. a title edit between them → two archives;
   3. a cover file rewritten (same name, same size, new content, a later modification time set with `File::set_modified`) → two archives;
   4. the newest archive deleted by hand → the next check writes;
   5. a fresh `BackupStatus` (restart) → the first check writes;
   6. a soft-deleted title hard-deleted between them (only Trash changes) → two archives;
   7. a settings row changed between them → two archives;
   8. a manual backup (`write_backup`) followed by a check with nothing changed → no new archive (the manual one's fingerprint counts).

   Unit tests cover the comparison rule (each of the four conditions alone makes it write).
5. **AC5 — rotation (D4, D5).** Unit tests of `rotation::plan` over synthetic names and a fixed clock, in `Europe/Zurich`:
   - hourly backups younger than 24 h kept, older ones reduced to the newest of each local day, days older than the 14 most recent local days deleted;
   - the newest backup kept even when 30 days old;
   - a name stamped in the future kept;
   - a foreign id, a temporary `.tmp-` name, a near-miss name (`mybibli-<id>-20261125T1200Z.tar.gz`, wrong case, other suffix), a sidecar name, and a name of the right shape whose stamp is not a real time (`20261325T120000Z`) never returned;
   - the 24-hour edge: a backup stamped exactly 24 h before now is no longer in the hourly tier;
   - **DST days**: on 2026-03-29 and 2026-10-25, the newest backup of the local day is chosen by local date, not UTC date (e.g. `2026-10-25T22:30Z` is 23:30 CET on the 25th and `2026-10-25T23:30Z` is 00:30 on the 26th; `00:30Z` and `01:30Z` are both 02:30 local on the 25th).

   An integration test on a temporary folder: the archives returned are deleted with their sidecars, an orphan own sidecar is deleted, a foreign sidecar and a symbolic link or directory named like an own backup are untouched, every other file is untouched, a rotation meeting a held guard deletes nothing, and a rotation on a read-only folder deletes nothing and does not fail the check.
6. **AC6 — Health line (D6, D7).** Route tests render each state: last backup / none, next check / due / off, last check skipped, folder missing, folder not writable, last automatic backup failed. Times are in the instance zone. Warnings carry the text "Warning:" (not colour only).
7. **AC7 — access.** Only an Admin reaches the System save and sees the Health block (Librarian → 403 feedback, anonymous → login); the save requires the CSRF token.
8. **AC8 — E2E** (`specs/journeys/admin-backup-schedule.spec.ts`, from `loginAs`; Rules #3, #23):
   - the Admin sets "Every 6 hours" in Admin → System, sees the saved line, and the Health tab shows a "Next check" line; the spec puts the setting back as it found it, in an `afterAll` / `finally` so a failed assertion does not leave it changed (the setting is instance-wide);
   - the refusal: the spec changes an option's value to `5` in the page (`evaluate`), submits, and sees the refusal next to the field;
   - after a "Back up now" on the Backups tab, the Health tab shows a last backup stamped no earlier than the one the spec wrote (other specs may write backups: never assert it is exactly the spec's file).

   The epic smoke test is story 19-10, not here.
9. **AC9 — gates.** clippy (`-D warnings`), unit and integration tests (`tests/backup_schedule.rs`, `tests/backup_rotation.rs`, declared in `tests/integration.rs`), `sqlx prepare --check` (run `cargo sqlx prepare` first if a typed `query!` macro was added), `locale_parity::`, `templates_audit`, tsc, the `waitForTimeout` grep gate, and the full E2E suite with `CI=true --workers=2` with no flaky test.

## Tasks / Subtasks

- [ ] T1 — Setting `backup_interval_hours` (D1): `AppSettings` field + default + `load_from_db` validation, `ALLOWED_BACKUP_INTERVALS` constant, accessor, whitelist key in `fetch_setting_rows` (one more `?` in the `IN` list, and its #406 test in `tests/admin_system_integration.rs`), the `#[cfg(test)]` default. A seed migration `migrations/<timestamp>_settings_backup_interval.sql` inserts the row `backup_interval_hours` = the default (`save_setting` only updates an existing row).
- [ ] T2 — `routes/admin_system_backups.rs` + `fragments/admin_system_backups_form.html` + the section in `admin_system_panel.html` (pre-rendered `backups_form_html` field, like `time_zone_form_html`) + the route in `routes/mod.rs` (AC1, AC7).
- [ ] T3 — Writer stages (D3, D7): `Fingerprint` (dump SHA-256 + cover list), `BackupStatus` in `BackupConfig`, `write_backup_if_changed`, `write_backup` recording its fingerprint; `covers::list_covers` also returns size and modification time. Keep `mod.rs` focused; put the fingerprint in `backup/fingerprint.rs` if it grows.
- [ ] T4 — `services/backup/rotation.rs`: `plan` (pure) + `apply` (deletes under `try_lock`, sidecars, logs) (D4, D5, AC5).
- [ ] T5 — `tasks/backup_scheduler.rs` (D2): warm-up, 60 s wake, due-time function (pure, unit-tested), check, rotation, state transitions and logs; spawned from `main.rs` next to the auto-purge scheduler. Call the rotation after a successful manual backup in `routes/admin_backups.rs`.
- [ ] T6 — Health block (D6): `routes/admin_health_backup.rs` + `fragments/admin_health_backup.html`, included by `admin_health_panel.html`, one field in `AdminHealthPanel` (`routes/admin.rs` stays under 2000 lines).
- [ ] T7 — Locales ×4 (D1, D6 copy as approved); `touch src/lib.rs` before the build.
- [ ] T8 — Tests (AC2–AC8): unit tests in each new module; `tests/backup_schedule.rs`, `tests/backup_rotation.rs` (+ `tests/common/backup.rs` helpers from 19-6); E2E spec `admin-backup-schedule.spec.ts`.
- [ ] T9 — `CLAUDE.md` Key Patterns: extend the Backups bullet (schedule, skip rule, rotation, in-memory status, foreign backups never deleted). `docs/auth-threat-model.md` § 5.13: add that mybibli now deletes files in the folder on its own, only own-id backup names and their sidecars, never a foreign backup, never the newest. Comment on #729 (D8). The manual is story 19-11 (schedule, rotation, "one folder per instance").

## Dev Notes

### Guardrails

- Never create the backups folder; never write or delete outside it; never delete a name that is not an own backup name or its sidecar (Rule #20: positive shape only).
- Never delete the newest own backup. Never delete a foreign backup (D5).
- Delete only regular files (`DirEntry::file_type`, which does not follow symbolic links); a directory or a link with a backup name is left alone. A name whose stamp does not parse as a real UTC time is never a candidate.
- The rotation takes the guard (`try_lock`); a held guard means "skip", never "wait".
- No database write for the schedule state: it would change the next dump and defeat the skip rule.
- Times: storage and names stay UTC; the day cut of the rotation and the Health times use the instance zone (`crate::utils::local_time`). Never `Local::now()`, never compute an offset by hand. Read the zone per check (`AppState::time_zone()`), since an admin can change it.
- Do not hold the `std::sync::Mutex` of `BackupStatus` or the settings `RwLock` across an `.await`.
- New System settings must be whitelisted in `fetch_setting_rows` (CLAUDE.md, story 8-5 pattern), or the form never shows the stored value.
- Copy: D1 and D6 word for word once approved (practice A3); field refusal terse; warnings follow "what happened → why → what to do" (Rule #9).
- CSP: no inline script or style; the select is a plain form (no JS needed).
- Focus (Rule #21): the time-zone precedent swaps its whole form (`hx-swap="outerHTML"`), which drops the focus held by the select or the button. Keep the button's id stable and let the answer land so that focus stays on it, or swap only a status region beside the select.
- The Health block reads the folder at each render: do that listing in `spawn_blocking` too (#729 L3), not on the async worker.
- Heavy commands under `flock -w 10800 /tmp/station-heavy.lock mem-guard …`; never run `cargo fmt`; do not touch kesh's containers, ports or databases.

### Couplings (Rule #24)

- **19-6** (merged before this story): reuses `write_backup`, the name shape, the guard and the Backups tab. The writer's stages are split, not duplicated.
- **19-5**: shares the Health panel template (`admin_health_panel.html`); this story adds one include after the watching panel.
- **19-8 (restore)** builds on this story:
  - it takes the same guard, so a rotation never runs during a restore (and the reverse);
  - it lists the folder with `is_backup_name` and the id from the name, like the rotation;
  - it decides the fate of a restored foreign backup (D5) and keeps the local `instance_id` across a restore (D8);
  - its safety backup goes through `write_backup` and is rotated like any backup unless 19-8 decides otherwise (that would need a different name shape).
- **19-9** reads the covers folder; it deletes only `<digits>.jpg` there, never in the backups folder.
- **19-10** (epic smoke) checks that the Health tab shows the backup.
- **19-11** documents the schedule, the rotation, "one folder per instance".
- **E2E**: `admin-backups.spec.ts` (19-6) and this story's spec both write backups into the same tmpfs folder; each asserts only on its own file or on "no earlier than" (Rule #23).
- **E2E timing**: the scheduler's first check can fall inside a long E2E run and hold the guard for a few seconds; see the D9 validator note.
- **The new setting is instance-wide**: `admin-backup-schedule.spec.ts` changes it and restores it; it only affects the scheduler, which no other spec reads, so the spec may stay in the parallel project (CLAUDE.md E2E patterns).
- `sprint-status.yaml`: only this story's line. Its key is `19-7-scheduled-backups-and-rotation`, while this file is `19-7-scheduled-backups.md`; the dev workflow must update that key.

### Previous story intelligence (19-6)

- `sqlx::raw_sql(…)` inside a spawned task fails the higher-ranked `Send` check; use `conn.execute("…")`.
- The seeded users read French: route tests assert the FR copy.
- The app has no request timeout layer; a write answers when done.
- Deferred Low findings of 19-6 are in #729 (L3 blocking I/O on the async worker, L4 `O_NOFOLLOW`, …). This story does not fix them, but it must not add new blocking I/O on the async worker in the scheduler loop: run the folder listing and the deletions in `spawn_blocking`.

### References

- #657 (decisions, "Schedule", "Retention"), #729 (L8, L10), epic 19 Story 19.7 and 19.8 in `_bmad-output/planning-artifacts/epics.md`, FR179, FR180, UX-DR45.
- `_bmad-output/implementation-artifacts/19-6-writing-a-backup.md` (D1–D8).
- `src/services/backup/mod.rs`, `src/services/backup/covers.rs`, `src/services/instance_id.rs`, `src/routes/admin_backups.rs`, `src/tasks/auto_purge_scheduler.rs`, `src/routes/admin_system.rs` (`fetch_setting_rows`), `src/routes/admin_system_time_zone.rs` (form precedent), `docs/auth-threat-model.md` § 5.13, `src/config.rs` (`AppSettings`), `src/utils/local_time.rs`, `src/routes/admin_health_watch.rs`.

## Validation (2026-11-26)

Independent validation against the epic (Story 19.7, FR179, FR180, UX-DR45), the 19-6 code on this branch and `CLAUDE.md`.

Checked, found correct:
- Every cited path and symbol exists: `write_backup`, `BackupConfig::guard` (`try_lock_owned`), `FolderMissing` / `FolderNotWritable` / `AlreadyRunning`, `folder_state`, `HashingWriter` and `sql_digest`, `covers::list_covers` (`CoverList`, `.staging` skipped), `backup_file_name`, `is_backup_name`, `sidecar_path`, `is_stale_temporary`, `instance_id::{ensure, read, is_instance_id}`, `fetch_setting_rows`, `save_setting`, `reload_settings_cache`, `auto_purge_scheduler` (spawned in `main.rs` line 588), `local_time::{to_local, local_date, format_local_date_time, LocalClock}`, `AppState::time_zone()`, `AdminHealthPanel`, the watch include in `admin_health_panel.html`, `tests/common/backup.rs`, `admin-backups.spec.ts`, the E2E tmpfs at `/app/backups`. File sizes: `admin.rs` 1886, `admin_system.rs` 1789 lines.
- Dump determinism: fixed header, rows `ORDER BY` primary key, `sessions` rowless, the session token blanked. The skip rule (D3) therefore never skips a change in a row (Trash and settings included) or in the schema; covers are covered by condition 4. A restart forgets the fingerprint, so the first check writes. A skipped check does not move "last backup" (read from the folder, D7).
- DST examples of AC5: 2026-03-29 and 2026-10-25 are the EU change Sundays; the UTC → local times given are right.
- Epic consistency: foreign backups never deleted (D5) matches "never deleted before it has been restored"; 19-10 and 19-11 exist for the smoke test and the manual.

Changed:
- Context: the seed migration need (`save_setting` is update-only), the time-zone form as precedent, the daily churn (once or twice a day), `AUTO_INCREMENT` in `SHOW CREATE TABLE` (false "changed" only).
- D1: the route goes in `routes/mod.rs`, not `admin_system.rs` (factual fix).
- D2: future-stamped and unparsable names ignored for the base; no rotation when the folder is unusable.
- D7: the fingerprint (with name and size) is recorded only after the rename of archive and sidecar.
- AC2: tests call the check function with a clock. AC4: cases 6–8 (Trash only, settings, manual backup), the cover case pins the mtime. AC5: unparsable stamp, sidecar names, 24 h edge, links and directories, read-only folder. AC8: restore the setting in `afterAll`. AC9: `locale_parity`, `templates_audit`, `sqlx prepare`.
- Tasks: seed migration and `IN` placeholder (T1), panel field (T2), threat model § 5.13 (T9).
- Guardrails: regular files only, focus (Rule #21), Health folder listing in `spawn_blocking`. Couplings: E2E timing, instance-wide setting, the sprint-status key differs from this file's name.
- Validator notes added under D1, D3, D4 and D9; no decision changed.

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List
