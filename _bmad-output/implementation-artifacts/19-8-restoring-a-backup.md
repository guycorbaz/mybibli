# Story 19.8: Restoring a backup (#657, part 3)

Status: in-progress

## Story

As an Admin,
I want to restore a backup present in the folder from the admin page,
so that a mistake or a lost day is undone without touching the NAS.

## Context

Covers FR181 and UX-DR46 (the list of backups and the restore modal). The decisions recorded on #657 are the specification ("Restore (v1)"), with these already settled by the project lead:
- only a backup **present in the backups folder** is restored (no upload, no download in v1);
- its checksum is verified before anything is touched; a damaged or truncated file is refused;
- a **safety backup** of the current state is written first;
- the app refuses other requests during the restore (maintenance) and **every user is logged out** afterwards;
- a backup from a **newer** version is refused; an **older** one is restored, then brought up to date by the migrations;
- a backup of **another instance** is never deleted automatically (before or after a restore) and is shown as "another instance" (19-7 D5, #657 comment of 2026-11-26);
- the local `instance_id` is **kept** across a restore (19-7 D8, comment on #729).

What exists (stories 19-6 and 19-7, on `main`):
- **Format** (`src/services/backup/`): `mybibli-<16 hex id>-<YYYYMMDD>T<HHMMSS>Z.tar.gz` + sidecar `<name>.sha256` (one `sha256sum` line for the whole archive). Members in order: `database.sql`, `covers/<digits>.jpg`…, `manifest.json` **last** (`archive::Manifest`: `format` = `"mybibli-backup"`, `format_version` = 1, `instance_id`, `mybibli_version`, `last_migration`, `created_at`, `tables` (row counts), `covers` {count, bytes, skipped}, `members` [{name, sha256, size}]). `archive::{SQL_MEMBER, MANIFEST_MEMBER, COVERS_PREFIX, HashingWriter, hex}`.
- **Dump** (`dump.rs`): header `SET NAMES utf8mb4; SET time_zone='+00:00'; SQL_MODE='NO_AUTO_VALUE_ON_ZERO'; FOREIGN_KEY_CHECKS=0; UNIQUE_CHECKS=0;`, then per table `DROP TABLE IF EXISTS`, the `SHOW CREATE TABLE` text, multi-row `INSERT`s (≤ 500 rows, ≤ ~1 MiB each, a bigger single row alone); footer restores the checks. Every base table, `_sqlx_migrations` included, Trash included; `sessions` **without rows** (`ROWLESS_TABLES`), `pending_metadata_updates.session_token` blanked (`BLANKED_COLUMNS`); generated columns left out of the `INSERT`s. Values from `QUOTE()` may hold raw newlines and `;`.
- **Writer**: `write_backup(pool, &BackupConfig)` takes the one-at-a-time guard itself (`BackupConfig::guard`, `try_lock_owned` → `BackupError::AlreadyRunning`), checks the folder (`folder_state`, `FolderMissing` / `FolderNotWritable`), runs in a spawned task, remembers the skip-rule fingerprint (`status::BackupStatus`).
- **Listing** (`listing.rs`): `list_folder` (regular files, sizes), `parse_backup_name` (id + real UTC stamp), `own_backups`, `newest_at_or_before`. **Rotation** (`rotation.rs`): `plan` (pure) / `rotate` (under `try_lock`, skipped when the guard is held), after every scheduled check and every successful manual backup.
- **Backups tab** (`routes/admin_backups.rs`, `fragments/admin_backups_panel.html`): folder line `#admin-backups-folder` (OOB), "Back up now" (`POST /admin/backups`), answer region `#admin-backups-feedback`.
- **Health tab** backups block (`routes/admin_health_backup.rs`, `fragments/admin_health_backup.html`).
- **Boot sequence** (`src/main.rs`): `sqlx::migrate!("./migrations").run` → `seed_gate::apply` → `MYBIBLI_RESET_ADMIN` hatch → `validate_schema` → startup purge → `migrate_legacy_env_vars` → `isbn_repair` → `convert_isbn10_upc_titles` → `description_repair` → `default_skills::insert_defaults` → `instance_id::ensure` → `AppSettings::load_from_db` → `SetupGateState::initialize` → log-level reconcile.
- **In-memory state that mirrors the database**: `AppState.settings` (reload: `services::admin_system::reload_settings_cache`), `AppState.setup_gate` (`middleware::setup_gate::refresh`), the log level (`AppState.log_level_reloader`), `BackupStatus` (19-7 fingerprint).
- **Request layers** (`routes::build_router`): `CSP → SetupGate → SessionResolve → Locale → (non-HTMX wrapper) → ModalConfirmRetargetGuard → CSRF → handler`.
- **Writers without a request**: `tasks::{auto_purge_scheduler, anonymous_session_purge, metadata_retry, backup_scheduler, provider_health, log_purge}`, the per-scan metadata fetches (`tasks::metadata_fetch::spawn_fetch`), the bulk cover / metadata runs (`routes/admin.rs`, status `AppState.bulk_cover_fetch`), the fire-and-forget `last_activity` update (`middleware/auth.rs`).
- **Database user**: the compose `mybibli` user holds rights on its own schema only — no `CREATE DATABASE`; a restore works inside the live schema.
- **Audit**: `admin_audit.user_id` is nullable (FK `ON DELETE SET NULL`), but `AdminAuditModel::create` takes a `u64`; background actions write as the SYSTEM user (`UserModel::find_system_user_id`).

Practice A3: every user-visible behaviour and copy below that the lead has not already decided is a numbered decision, settled before the first review pass.

## Decisions for the project lead

**Approved by the project lead, 2026-11-26: D1–D13 all as recommended (A; D3 copy as proposed), EN and FR copy as written.** Each D has a recommendation; one word ("A", "yes", "as proposed") is enough. EN and FR copy is given in full; DE and IT are translated at dev time from EN/FR (as in 19-6 / 19-7) and reviewed in the code review.

- **D1 — where restore lives, and the list (UX-DR46).**
  - **A (recommended):** on the Backups tab, under "Back up now", a list "Backups in the folder", newest first, one row per backup: date and time (instance zone, from the name), origin, size, a note column (D3, D4) and a "Restore…" button opening the modal. The list is refreshed out of band after "Back up now" (new file, rotation). Only names of the backup shape with a real stamp, regular files (19-7 listing rules); every other file is ignored. Empty: a sentence.
  - B: a separate "Restore" tab.
  - Copy:
    - Heading: "Backups in the folder" / « Sauvegardes présentes dans le dossier »
    - Columns: "Date" / « Date » — "Origin" / « Origine » — "Size" / « Taille » — "Note" / « Remarque »
    - Button: "Restore…" / « Restaurer… » (accessible name with the date: "Restore the backup of %{when}" / « Restaurer la sauvegarde du %{when} »)
    - Empty: "No backup in the folder yet." / « Aucune sauvegarde dans le dossier pour l'instant. »
    - Folder missing or not readable: the list is replaced by the existing folder line (19-6), no new copy.
- **D2 — the version column (UX-DR46 says the list shows the version).** The version is only in `manifest.json`, the **last** member of a gzip stream: reading it means decompressing the whole archive, for every row, at every tab open.
  - **A (recommended):** the list shows date, origin, size; the version (and the counts) appear in the modal, which verifies the file anyway (D5). Amends UX-DR46's "version" column.
  - B: the list reads every manifest, cached in memory by (name, size, modification time); the first tab open after a start can take tens of seconds on a large folder.
- **D3 — origin and other instances (decided: shown, never deleted automatically; copy only).**
  - Origin: "This instance" / « Cette instance » — "Another instance (%{id})" / « Autre instance (%{id}) ».
  - A restored foreign backup stays in the folder as it is: never deleted, never rotated (19-7 D5 unchanged). The manual (19-11) says to delete it by hand once no longer needed.
- **D4 — rotation and a hand-copied old backup of this instance.** 19-7 review: an own backup copied into the folder by hand, older than the retention, is deleted at the next rotation (after the next scheduled check, within the hour by default). Facts: the rotation never runs during a restore (it needs the same guard); once the restore has started, the file is no longer needed.
  - **A (recommended):** the list marks every own backup that the next rotation would delete (the pure `rotation::plan` with the current clock and settings): note "Removed at the next rotation" / « Supprimée à la prochaine rotation ». The manual (19-11) says: copy the backup in, restore it right away, or set "Automatic backup" to Off first and back on afterwards. No change to the rotation.
  - B: the manual only, no mark.
  - C: the rotation spares an own backup whose file was modified less than 24 hours ago (a fresh copy). Not recommended: `cp -p` / `rsync -a` keep the old time, so it protects only some copies, and it changes 19-7's rule.
- **D5 — checks before anything is touched, and the refusals.** When the modal opens, the server reads the file once, in a blocking task: the sidecar is present and its SHA-256 equals the archive's; the members are exactly `database.sql`, `covers/<digits>.jpg`… and `manifest.json` last (any other name, a link, a path with `/` or `..` beyond that shape → refused); each member's SHA-256 and size equal the manifest's; `format` / `format_version` are the known ones; the manifest's `instance_id` equals the id in the name; `last_migration` is a migration this binary knows (newer → R3; an unknown older level → R5). The confirm runs the same checks again, under the guard, before the safety backup. *(Changed at validation: the UX-DR8 macro always renders a Confirm button.)* A failure when the modal is asked for opens no modal: the refusal lands in the tab's feedback region `#admin-backups-feedback`, like 19-6's refusals. A failure at confirm (the file changed or vanished meanwhile, the guard held) lands in the modal's error region.
  - **A (recommended): a missing sidecar is a refusal (R2).** B: a missing sidecar only warns and the member hashes alone are trusted.
  - The list shows cheap states in the note column (no file read): "No checksum file" / « Pas de fichier de contrôle » when the sidecar is absent, "Not readable" / « Illisible » when `access(R_OK)` fails.
  - Refusals (all end with "Nothing was changed." — Rule #9):
    - R1 damaged: "This backup cannot be restored: its content does not match its checksum, so the file is damaged or incomplete. Nothing was changed. Use another backup, or copy this one again from where it came from." / « Cette sauvegarde ne peut pas être restaurée : son contenu ne correspond pas à sa somme de contrôle ; le fichier est endommagé ou incomplet. Rien n'a été modifié. Utilisez une autre sauvegarde, ou copiez celle-ci à nouveau depuis son origine. »
    - R2 no sidecar: "This backup cannot be restored: its checksum file %{file}.sha256 is missing, so mybibli cannot check it. Nothing was changed. Copy that file next to the backup, then try again." / « Cette sauvegarde ne peut pas être restaurée : son fichier de contrôle %{file}.sha256 est absent ; mybibli ne peut pas la vérifier. Rien n'a été modifié. Copiez ce fichier à côté de la sauvegarde, puis réessayez. »
    - R3 newer: "This backup was written by a newer mybibli (%{version}) and cannot be restored by this one (%{current}). Nothing was changed. Update mybibli to %{version} or later, then try again." / « Cette sauvegarde a été écrite par une version plus récente de mybibli (%{version}) et ne peut pas être restaurée par celle-ci (%{current}). Rien n'a été modifié. Mettez mybibli à jour en %{version} ou plus, puis réessayez. »
    - R4 not readable: "mybibli cannot read %{file}. Nothing was changed. Give the container's user read access to that file (see the manual, “Backups”), then try again." / « mybibli ne peut pas lire %{file}. Rien n'a été modifié. Donnez au compte du conteneur le droit de lire ce fichier (voir le manuel, « Sauvegardes »), puis réessayez. »
    - R5 not a known backup: "This file is not a backup this version of mybibli can restore. Nothing was changed." / « Ce fichier n'est pas une sauvegarde que cette version de mybibli peut restaurer. Rien n'a été modifié. »
    - R6 busy: "A backup or a restore is already running. Nothing was changed. Wait a minute, then try again." / « Une sauvegarde ou une restauration est déjà en cours. Rien n'a été modifié. Attendez une minute, puis réessayez. »
    - R7 bulk run in progress (D8): "A restore cannot start while covers or metadata are being downloaded for the whole library. Nothing was changed. Wait for that run to finish (Health tab), then try again." / « Une restauration ne peut pas démarrer pendant le téléchargement des couvertures ou des métadonnées de toute la bibliothèque. Rien n'a été modifié. Attendez la fin de ce traitement (onglet Santé), puis réessayez. »
    - R8 gone: "This backup is no longer in the folder. Nothing was changed." / « Cette sauvegarde n'est plus dans le dossier. Rien n'a été modifié. »
- **D6 — the confirmation modal (UX-DR8, `#modal-slot`, Pattern A).**
  - Title: "Restore this backup?" / « Restaurer cette sauvegarde ? »
  - Facts (after D5's checks): "Backup of %{when} — %{origin} — mybibli %{version} — %{size}" / « Sauvegarde du %{when} — %{origin} — mybibli %{version} — %{size} »; "It holds %{titles} titles and %{covers} covers; the library holds %{now_titles} titles and %{now_covers} covers now." / « Elle contient %{titles} titres et %{covers} couvertures ; la bibliothèque en contient %{now_titles} et %{now_covers} actuellement. » (counts Trash included, as the manifest and 19-6's success line).
  - Consequences: "The whole library — titles, volumes, loans, users, settings and covers — is replaced by the content of this backup. Everything entered since then is lost from the library, but a safety backup of the current state is written first. While the restore runs (a minute or two), mybibli answers nobody. Afterwards everyone is logged out, you included." / « Toute la bibliothèque — titres, volumes, prêts, utilisateurs, réglages et couvertures — est remplacée par le contenu de cette sauvegarde. Tout ce qui a été saisi depuis disparaît de la bibliothèque, mais une sauvegarde de sécurité de l'état actuel est écrite d'abord. Pendant la restauration (une minute ou deux), mybibli ne répond à personne. Ensuite, tout le monde est déconnecté, vous compris. »
  - Another instance, one more line: "This backup comes from another instance: its accounts and passwords replace those of this one. Log in afterwards with an account of the restored library." / « Cette sauvegarde vient d'une autre instance : ses comptes et mots de passe remplacent ceux de celle-ci. Connectez-vous ensuite avec un compte de la bibliothèque restaurée. »
  - Older version, one more line: "It was written by mybibli %{version}; it is brought up to date after the restore." / « Elle a été écrite par mybibli %{version} ; elle est mise à jour après la restauration. »
  - Buttons: "Cancel" / « Annuler » — "Restore this backup" / « Restaurer cette sauvegarde »; while it runs: "Restoring… This takes a minute or two; keep this page open." / « Restauration en cours… Cela prend une minute ou deux ; gardez cette page ouverte. »
  - **Confirmation style. A (recommended):** the explicit button alone — a safety backup makes the action reversible, and the consequences are spelled out. **B:** type-to-confirm like the Trash's permanent delete: the button is enabled once the Admin types the backup's date as shown (`%{when}`), with the label "Type %{when} to confirm" / « Saisissez %{when} pour confirmer ».
- **D7 — sessions and the restoring Admin.** Decided: everyone is logged out (the dump has no session rows, so the restored `sessions` table is empty).
  - **A (recommended):** the modal stays open and its body becomes the success line with a "Log in again" link to `/login` (focus moves to that link, the modal's own content; Rule #21 is about not stealing focus elsewhere). Success: "Backup %{file} restored (%{titles} titles, %{covers} covers). The safety backup of the previous state is %{safety}. Everyone has been logged out." / « Sauvegarde %{file} restaurée (%{titles} titres, %{covers} couvertures). La sauvegarde de sécurité de l'état précédent est %{safety}. Tout le monde a été déconnecté. » Link: "Log in again" / « Se reconnecter ».
  - B: an `HX-Redirect` to `/login`, with no message (the Admin does not see which safety backup was written).
- **D8 — maintenance while the restore runs.**
  - A process-wide maintenance state: from the confirm (after the checks) to the end of the restore, every request gets **503** with `Retry-After: 60`, except `/static/*` and `/health` (a probe keeps reading the database's reachability; `/covers/*` is refused: the files are being replaced). New requests are refused at once; requests already running are waited for, up to 30 s (beyond that the restore is abandoned with the R9 line below and maintenance ends: nothing was touched yet).
  - Direct navigation: a standalone page (no database read, no session), title "Maintenance" / « Maintenance », text "mybibli is restoring a backup. This takes a minute or two: reload this page then. Everyone will need to log in again." / « mybibli restaure une sauvegarde. Cela prend une minute ou deux : rechargez cette page ensuite. Tout le monde devra se reconnecter. » HTMX request: the same text as an error FeedbackEntry (`HX-Retarget: #feedback-list`, so 18-5's `responseError` handler shows it). `/api/*` (API v1, MCP): JSON `{"error": "maintenance", "message": "mybibli is restoring a backup; try again in a minute."}` (agent-facing English).
  - A restore is refused while a bulk cover or metadata run is in progress (R7); background writers skip their work while maintenance is on (technical, see T-decisions).
  - R9 (drain timed out): "The restore did not start: other requests were still running after 30 seconds. Nothing was changed. Try again in a moment." / « La restauration n'a pas démarré : d'autres requêtes étaient encore en cours après 30 secondes. Rien n'a été modifié. Réessayez dans un instant. »
  - **Options:** A — as above (recommended); B — `/covers/*` stays open too.
- **D9 — the safety backup.** It is an ordinary backup of this instance (same name shape, same writer, written during maintenance so nothing changes under it), named in the success line. It is rotated like any other (kept at least 24 hours, then as the newest of its day).
  - **A (recommended):** as above. If it cannot be written (folder not writable, disk full), the restore is refused: "Nothing was restored: mybibli could not first write a safety backup of the current library. %{reason}" / « Rien n'a été restauré : mybibli n'a pas pu écrire d'abord une sauvegarde de sécurité de la bibliothèque actuelle. %{reason} » (`%{reason}` = 19-6's existing localized backup error).
  - B: a distinct name shape (`mybibli-<id>-<stamp>-safety.tar.gz`) kept out of the rotation; not recommended (amends 19-6 D4 / 19-7 D4, a file that grows the folder forever).
- **D10 — failure, rollback and a crash in the middle.** MariaDB commits every `DROP` / `CREATE` at once: a restore cannot be one transaction.
  - A load or migration failure after the first change: mybibli restores the safety backup by the same procedure (no new safety backup), then ends maintenance. Message in the modal: "The restore failed, so mybibli put the library back as it was just before, from the safety backup %{safety}. Everyone has been logged out. The logs say why the restore failed." / « La restauration a échoué ; mybibli a remis la bibliothèque dans l'état où elle était juste avant, depuis la sauvegarde de sécurité %{safety}. Tout le monde a été déconnecté. Les journaux indiquent pourquoi la restauration a échoué. »
  - **Crash protection. A (recommended):** before the first change, mybibli writes a journal file `.restore-in-progress.json` (0600) in the backups folder naming the backup and the safety backup; it removes it at the end (success or rolled back). At boot, **before the migrations**, a journal present means a restore was cut short (process killed, container restarted, NAS reboot): mybibli restores the safety backup it names (checks of D5 included), logs a `warn`, removes the journal, then boots as usual. B: no journal; after a crash the database may be half loaded and the migrations at boot may fail (container restart loop), recovery by hand from the manual.
  - Rollback itself failing (or the journal's safety backup failing its checks at boot): maintenance **stays on** until a restart; `error` log; the maintenance page then reads: "mybibli could not restore a backup nor put the library back. Ask the person who installed mybibli to restore the backup %{safety} by hand (see the manual, “Restoring by hand”)." / « mybibli n'a pu ni restaurer une sauvegarde ni remettre la bibliothèque en état. Demandez à la personne qui a installé mybibli de restaurer à la main la sauvegarde %{safety} (voir le manuel, « Restaurer à la main »). » At boot with a journal whose rollback fails, the process exits non-zero with the same text in the log (no serving of a half-loaded database). The manual section is story 19-11.
- **D11 — covers.** The covers folder ends with exactly the backup's `<digits>.jpg` files: each is written as a temporary file in the covers folder then renamed over; afterwards every `<digits>.jpg` there that the backup does not hold is deleted (positive shape, Rule #20; the safety backup holds them). `.staging/` and any other name are left alone (the staging purge already reconciles staged files with the restored proposals). **A (recommended)** as said; B: covers added only, none deleted (titles of the restored database would be fine, but files no title owns would stay until 19-9's tool).
- **D12 — what is kept from the current instance.** Decided: the `instance_id` (the row is rewritten with the local id after the load). **A (recommended): nothing else** — settings (time zone, provider keys, backup interval and retention, log level, session timeout…), users and API keys all come from the backup; the in-memory caches are reloaded from it. B: also keep the local "Automatic backup" and "Daily backups to keep" settings (they describe the folder of this NAS, not the library).
- **D13 — afterwards: audit and Health tab.**
  - An `admin_audit` row `backup_restore` is written in the restored database (as the SYSTEM user, since the restoring Admin may not exist there), details: backup file, its origin id and version, safety backup file, the Admin's username, outcome (`restored` / `rolled_back` / `rolled_back_at_boot`).
  - **A (recommended):** the Health tab backups block gains one line from the newest such row: "Last restore: %{when}, from %{file} (safety backup: %{safety})." / « Dernière restauration : %{when}, depuis %{file} (sauvegarde de sécurité : %{safety}). »; a rolled-back one: "Last restore attempt: %{when}, from %{file}; it failed and the library was put back from %{safety}." / « Dernière tentative de restauration : %{when}, depuis %{file} ; elle a échoué et la bibliothèque a été remise en état depuis %{safety}. » The "Last backup" line then shows the safety backup (it is the newest), as today. B: no new Health line (audit row only).

### Technical decisions (not for the lead; may change in review)

- **T1 — modules (Rule #12).** `services/backup/restore/` : `mod.rs` (API `restore_backup(state, name, admin) -> Result<Restored, RestoreError>`, sequence, journal), `verify.rs` (D5, pure checks on a reader + file checks), `load.rs` (statement splitter + execution), `covers.rs` (D11). `services/maintenance.rs` (state) + `middleware/maintenance.rs` (layer, D8). `routes/admin_backups_restore.rs` (modal GET + POST) and `fragments/admin_backups_list.html`, `fragments/admin_backups_restore_modal.html`, `templates/pages/maintenance.html`.
- **T2 — the guard is held across the whole restore.** The restore takes `BackupConfig::guard` (`try_lock_owned`, R6) **before** the confirm-time checks and keeps it until the end; the safety backup and the rollback run through a writer entry that takes an already-held guard (e.g. `write_backup_holding(&OwnedMutexGuard, …)` sharing 19-7's `guarded` / `run` stages), never `write_backup` (which would meet its own guard and answer `AlreadyRunning`). Consequence: no scheduled check, manual backup or rotation runs during a restore.
- **T3 — one migrator.** `pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");` in `src/db.rs`, used by `main.rs` and by the restore; D5's "known level" = `MIGRATOR.iter().any(|m| m.version == last_migration)`, newer = above `MIGRATOR`'s highest version.
- **T4 — one post-load sequence (DRY).** Extract from `main.rs` the database steps after the migrations that boot runs (`seed_gate::apply`, `validate_schema` log, `migrate_legacy_env_vars`, `isbn_repair`, `convert_isbn10_upc_titles`, `description_repair`, `default_skills::insert_defaults`, `instance_id::ensure`) into `services::startup::prepare_database(pool)`; boot and restore call it. Not in it: the `MYBIBLI_RESET_ADMIN` hatch, the startup purge (the scheduler does it). After it, the restore reloads `AppState.settings` (`reload_settings_cache`), refreshes `setup_gate` (`setup_gate::refresh`), applies the restored log level (`log_level_reloader`), and makes `BackupStatus` forget its fingerprint.
- **T5 — the load.** On one dedicated connection (`close_on_drop`): `SET FOREIGN_KEY_CHECKS = 0`, drop **every** base table of the current schema (`information_schema.TABLES`, `TABLE_TYPE = 'BASE TABLE'`) — a table a newer migration created would otherwise survive and make the migrations fail —, then execute `database.sql` statement by statement, streamed from the archive member (memory flat). The splitter knows the dump's own syntax: statements end with `;` at the end of a line **outside** single-quoted strings (backslash escapes), backquoted identifiers and double-quoted strings; unit-tested on the writer's output (raw newlines and `;` inside values). Then the `instance_id` row is rewritten (`UPDATE … SET value = <local id>`; inserted if absent), then `MIGRATOR.run`. Use `conn.execute(&str)` (text protocol) — `sqlx::raw_sql` in a spawned task fails the `Send` check (19-6).
- **T6 — maintenance state and the drain.** `BackupConfig.maintenance: Arc<Maintenance>` (in `BackupConfig`, not a new `AppState` field: 70 `AppState { … }` literals in `src/` and `tests/` already take `backup: Default::default()`, so no literal changes; changed at validation) { `active: AtomicBool`, a `tokio::sync::RwLock<()>` gate, a generation counter, an optional "stuck" message }. The layer (outermost after CSP: `CSP → Maintenance → SetupGate → …`) answers D8 when active, else holds a read guard for the request's duration. The restore POST is the one route that takes no read guard (exact method + path pattern, unit-tested), so the restore can take the write guard (timeout 30 s → R9). Locale of the maintenance answer without a database: the `lang` cookie, else `Accept-Language`, else the cached default language.
- **T7 — writers without a request.** While maintenance is active: the scheduled tasks that write the database skip their tick (auto-purge, anonymous-session purge, metadata retry; the backup scheduler is already blocked by the guard; provider health and log purge write no row); a metadata fetch, a cover store or a bulk-run step captures the generation when it starts and drops its write when the generation changed (a write prepared before a restore never lands in the restored database). The fire-and-forget `last_activity` update may fail during the load: it is already ignored.
- **T8 — the route.** `GET /admin/backups/restore/{name}` (modal, D5 checks, D6), `POST /admin/backups/restore/{name}` (restore). `{name}` must satisfy `parse_backup_name` (positive shape: no path, no traversal); the file is opened in the backups folder only, regular file only (`symlink_metadata`). Admin only (`require_role_with_return`), CSRF on the POST. A file named in no listing is R8.

- **T9 — the restore survives a disconnect** *(added at validation)*. Like 19-6's writer, the restore runs in a spawned task that owns the guard, the maintenance write guard and its connection; the handler awaits its `JoinHandle`. A closed browser tab never stops a restore halfway.
- **T10 — a core without `AppState`** *(added at validation)*. The boot recovery of D10 runs before `AppState` exists. The sequence (verify → safety backup → drop → load → instance id → migrations → `prepare_database` → covers → audit) is a function of `(pool, &BackupConfig, local_instance_id)`; the HTTP path adds the maintenance state and the cache reloads of T4 around it. `main.rs` reads `MYBIBLI_BACKUP_DIR` and `COVERS_DIR` before `MIGRATOR.run` (today they are read after it).
- **T11 — the journal carries the local instance id** *(added at validation)*. After a crash the `settings` row may be gone (tables dropped), so `.restore-in-progress.json` holds `{backup, safety, instance_id, started_at}`; the boot recovery takes the id from it (and checks it equals the id in the safety backup's name).
- **T12 — answers of the modal** *(added at validation)*. Pattern A (`#modal-slot`, `static/js/modal.js`): a refusal at confirm answers a 4xx / 409, which lands in the dialog's `data-modal-error` region (`ModalConfirmRetargetGuard` strips the retarget); the success and the rolled-back answers are 200 and replace the content of `#modal-slot` with a dialog holding the line and the "Log in again" link (no Confirm button, so not the D-macro: a small fragment with `aria-modal`, scanner-guard still sees it).

## Acceptance Criteria

1. **AC1 — the list (D1–D5).** The Backups tab lists every backup-shaped regular file with a real stamp in the folder, newest first, with date (instance zone), origin (this instance / another instance + id), size and the notes (no checksum file, not readable, removed at the next rotation). Other files, links, directories and names with an unreal stamp are not listed. The list is refreshed out of band after "Back up now". Route tests render each state (empty, own, foreign, notes); the rotation mark comes from `rotation::plan` with the current settings.
2. **AC2 — checks (D5).** Integration tests on a temporary folder, each answering its refusal in the modal **and** changing nothing (row counts and a table checksum unchanged, covers folder unchanged, no new file in the backups folder): a truncated archive (R1), one byte flipped in a cover member with the sidecar recomputed (R1, member hash), a missing sidecar (R2), a manifest with `last_migration` above the newest known (R3), an unreadable file (R4, skipped when run as root — the test names that), an unknown `format_version`, an extra member `../x` or `covers/a.png`, a manifest id differing from the name (R5), a held guard (R6), a bulk run in progress (R7), a name not in the folder (R8). Unit tests of the pure checks (member shape, version rule) and of the splitter.
3. **AC3 — a full restore.** Integration test: seed (title, volume, loan, cover, a settings row, a Trash row), back up, then change the library (title edited, a title added, a cover added, a settings row changed), restore the backup: every table equals the backup's (same comparison as `tests/backup_round_trip.rs`), the covers folder holds exactly the backup's covers (the added one deleted, `.staging/` untouched), `sessions` is empty, `instance_id` is the local one, a safety backup newer than the restored one exists and holds the changed state, an `admin_audit` `backup_restore` row exists, the settings cache and the setup gate reflect the restored rows.
4. **AC4 — an older backup.** Integration test: back up a database from which the newest migration's row and effect were removed (`_sqlx_migrations` row of `20261126100000` + its `settings` row `backup_retention_days`); restore it on the current schema: the migration runs again, the row and the migration are present, the restore succeeds.
5. **AC5 — another instance (D3, D12).** Integration test: a backup written under another `instance_id` is listed as "another instance"; restoring it keeps the local `instance_id`; the file stays in the folder; a rotation afterwards does not delete it.
6. **AC6 — maintenance (D8).** Tests: while active, a page request gets 503 + `Retry-After` + the maintenance page; an HTMX request gets 503 with the FeedbackEntry and `HX-Retarget: #feedback-list`; `/api/v1/…` gets the JSON body; `/health` and `/static/…` pass; a request started before is waited for; the drain timeout gives R9 with nothing changed. The scheduled tasks skip while active; a write carrying an old generation is dropped (unit test on the generation check).
7. **AC7 — failure and rollback (D10).** Integration tests with a fault injected after the drop (e.g. a `database.sql` whose last statement fails, rebuilt with a valid sidecar and manifest): the library equals the safety backup's state afterwards, the modal answers the rollback line, the journal file is gone, maintenance is off. Boot recovery: a journal left in the folder (simulated crash) makes `startup` restore the named safety backup before the migrations and remove the journal; a journal whose safety backup fails D5 stops the boot with an `error`.
8. **AC8 — access.** Only an Admin reaches the list, the modal and the restore (Librarian → 403 feedback, anonymous → login); the POST requires the CSRF token; the name parameter outside the positive shape is a 404 / R8, never a file outside the folder.
9. **AC9 — E2E** (`specs/journeys/admin-backup-restore.spec.ts`, from `loginAs`; Rules #3, #23): create a title (spec ISBN), "Back up now" (`helpers/backups.ts::backUpNow`), delete the title, open the spec's backup's "Restore…" (found by the file name of the spec's own answer, never "the first row"), see the facts, confirm, see the success line, follow "Log in again", log in, find the title back. A refusal case: a damaged backup cannot be made from the browser — the refusal copy is covered by AC2's route tests. **The spec replaces the whole database and logs everyone out:** it runs in its own Playwright project chained after every other one (after `instance-session-timeout`), one worker; story 19-10's smoke test joins that project.
10. **AC10 — gates.** clippy (`-D warnings`), unit and integration tests (`tests/backup_restore.rs`, `tests/backup_restore_maintenance.rs`, declared in `tests/integration.rs`), `sqlx prepare --check`, `locale_parity::`, `templates_audit` (no inline script/style, the modal through the UX-DR8 macro, no `hx-confirm`, CSRF field), tsc, the `waitForTimeout` grep, `vcode-literals.sh`, and the full E2E suite with `CI=true --workers=2` with no flaky test.

## Tasks / Subtasks

- [x] T1 — `MIGRATOR` static (T3) and `services::startup::prepare_database` extracted from `main.rs` (T4); boot unchanged in behaviour.
- [x] T2 — `services/backup/restore/verify.rs` (D5) + unit tests.
- [x] T3 — writer entry holding the guard (T2) + `BackupStatus::forget`.
- [x] T4 — `restore/load.rs` (T5: drop all, splitter, execute, instance id, migrations) + `restore/covers.rs` (D11) + `restore/mod.rs` (sequence, journal D10, rollback, audit D13).
- [x] T5 — `services/maintenance.rs` + `middleware/maintenance.rs` (D8, T6) wired outermost after CSP in `routes::build_router`; `templates/pages/maintenance.html`; generation checks in the writers without a request (T7); boot journal check before `MIGRATOR.run` in `main.rs`.
- [x] T6 — `routes/admin_backups_restore.rs` + list fragment (D1–D4) included in `admin_backups_panel.html` + modal fragment (D6, D7) + routes in `routes/mod.rs`; Health line (D13) in `routes/admin_health_backup.rs`.
- [x] T7 — Locales ×4 (copy as approved); `touch src/lib.rs` before the build.
- [x] T8 — Tests AC1–AC8 (+ `tests/common/backup.rs` helpers: rebuild an archive with a changed manifest / member and a fresh sidecar); E2E spec + Playwright project (AC9).
- [x] T9 — `CLAUDE.md` Backups bullet (restore, maintenance layer, journal, guard held, `MIGRATOR`, `prepare_database`); `docs/auth-threat-model.md` § 5.13 (restore: Admin + CSRF, positive name shape, member shape checked before extraction, foreign accounts replace local ones, sessions emptied, the journal file). The manual is story 19-11 (step-by-step restore, "Restoring by hand", copying a backup in and D4's advice).

## Dev Notes

### Guardrails

- Nothing is touched before D5's checks pass at confirm time **under the guard**; the file is re-hashed while it is loaded, and a member differing from the manifest at that point is a failure (rollback), never a silent continue.
- Never extract a member name outside the positive shape; never write outside `COVERS_DIR` (covers) and the backups folder (journal, safety backup); temporary cover files are `.tmp-…` in `COVERS_DIR`, removed on failure.
- Never delete a foreign backup, the restored backup or the safety backup in this story's code.
- No `deleted_at IS NULL` in the restore (it replaces every row, Trash included — same exception as the dump).
- Do not hold the maintenance gate, the settings `RwLock` or `BackupStatus`'s mutex across an `.await` beyond what T6 states (the write guard is held across the restore on purpose).
- Log lines: file names, counts, outcome; never row contents.
- Copy word for word once approved (practice A3); Rule #9 for every refusal.
- CSP: the maintenance page and the modal have no inline script or style; the modal uses `templates/components/modal.html` (UX-DR8) so scanner-guard covers it.
- Heavy commands under `flock -w 10800 /tmp/station-heavy.lock mem-guard …`; never run `cargo fmt`; do not touch kesh's containers, ports or databases.

### Couplings (Rule #24)

- **19-6 / 19-7** (merged): format, writer stages, listing, rotation, guard. This story adds a writer entry; it does not change the format or the rotation rule.
- **19-9** (cover consistency): both touch `COVERS_DIR`; 19-9 runs after this story; both delete only `<digits>.jpg`.
- **19-10** (epic smoke) restores through the modal: it goes into this story's chained Playwright project.
- **19-11** (manual): step-by-step restore, "Restoring by hand", D4's advice, foreign backups removed by hand.
- **E2E**: the restore spec wipes every other spec's rows: it must run last (AC9). Specs in the earlier projects are untouched.
- `sprint-status.yaml`: only this story's line (`19-8-restoring-a-backup`).

### Previous story intelligence (19-6, 19-7)

- `sqlx::raw_sql` in a spawned task fails the higher-ranked `Send` check: `conn.execute("…")` per statement.
- The seeded users read French: route tests assert the FR copy. Askama escapes `'` as `&#39;`.
- Blocking file work (`read_dir`, hashing, extraction, renames) goes in `spawn_blocking`.
- `backUpNow` (E2E helper) retries once on "already running".
- The app has no request timeout layer.

### References

- #657 (decisions, "Restore (v1)", comments of 2026-11-11 and 2026-11-26), #729 (L8, L10 and the 19-7 comment), epic 19 Story 19.8 in `_bmad-output/planning-artifacts/epics.md`, FR181, UX-DR46.
- `_bmad-output/implementation-artifacts/19-6-writing-a-backup.md`, `19-7-scheduled-backups.md`.
- `src/services/backup/{mod,archive,dump,listing,rotation,status}.rs`, `src/routes/admin_backups.rs`, `src/routes/admin_health_backup.rs`, `src/main.rs`, `src/routes/mod.rs` (`build_router`), `src/middleware/setup_gate.rs`, `src/services/admin_system.rs`, `src/models/admin_audit.rs`, `src/models/user.rs` (`find_system_user_id`), `tests/backup_round_trip.rs`, `tests/common/backup.rs`, `tests/e2e/playwright.config.ts`, `docs/auth-threat-model.md` § 5.13.

## Validation (2026-11-26)

Checked against `main` (19-6 and 19-7 merged) — `src/services/backup/*`, `routes/admin_backups.rs`, `routes/admin_health_backup.rs`, `src/main.rs`, `routes/mod.rs::build_router`, `middleware/{setup_gate,auth,locale}.rs`, `services/admin_system.rs`, `models/admin_audit.rs`, the migrations, the compose files and `tests/e2e/playwright.config.ts`.

Checked, found correct:
- Every cited symbol exists: `write_backup` / `guarded` / `run`, `BackupConfig::{guard, status}`, `folder_state`, `is_backup_name`, `listing::{list_folder, parse_backup_name, own_backups, newest_at_or_before}`, `rotation::{plan, rotate}` (`plan(names, instance_id, now, tz, retention_days)` — what D4's mark needs), `archive::{Manifest, SQL_MEMBER, MANIFEST_MEMBER, COVERS_PREFIX, HashingWriter}`, `dump::{HEADER, FOOTER, ROWLESS_TABLES, BLANKED_COLUMNS}`, `reload_settings_cache(&AppState)`, `setup_gate::refresh(&Arc<RwLock<_>>, &DbPool)`, `seed_gate::apply`, `instance_id::{ensure, read}` (`settings.setting_key` / `setting_value`), `UserModel::find_system_user_id`, `AppState::{default_language, time_zone, backup_retention_days}`, `BulkCoverFetchStatus::running` (R7's test; the bulk metadata backfill shares the same status), the `lang` cookie read by `middleware/locale.rs`.
- The manifest is the last member: D2's cost argument holds (the version needs a full decompression).
- `sessions` is rowless in every backup, so "everyone logged out" needs no extra step; the fire-and-forget `last_activity` update ignores its error.
- No view, trigger, routine or event in the migrations: dropping every base table empties the schema. The compose `mybibli` user has rights on its schema only, so a load into a second schema (atomic `RENAME TABLE` swap) is not available — hence the safety-backup rollback and D10's journal.
- `_sqlx_migrations` is in the dump, so a restored database is at the backup's level and `MIGRATOR.run` brings it up. A backup holding a migration this binary does not know at a level below its newest would make `run` fail (`VersionMissing`) → rollback (D10); D5 refuses only by `last_migration`.
- Setup gate whitelist and `/health` (`SELECT 1`) behave as D8 assumes; `/static` goes through the router's layers (whitelisted by the setup gate), so the maintenance layer must whitelist it too.
- The E2E stack has the backups `tmpfs`, `MYBIBLI_SEED_DEV_USERS=1` (the seed gate keeps the seeded users of a restored E2E backup) and two chained instance-wide projects; AC9's project chains after `instance-session-timeout`.

Changed (no product decision needed):
- **D5:** the UX-DR8 macro always renders a Confirm button, so a failed check at modal time answers in the tab's feedback region instead of a modal without Restore; a failure at confirm lands in the modal's error region (T12). *Flagged for the lead with D5.*
- **T6:** the maintenance state lives in `BackupConfig` (no change to the 70 `AppState` literals).
- **T7:** provider health and log purge write no row (removed from the list).
- **Context:** `admin_audit.user_id` is nullable (`SET NULL`, migration `20260513000002`), but `AdminAuditModel::create` takes a `u64`: the SYSTEM user stays the author (D13).
- **Added:** T9 (spawned restore task), T10 (core without `AppState`; `main.rs` reads the two folders before the migrations), T11 (journal carries the local id), T12 (modal answers).
- **Tests:** the table comparison of `tests/backup_round_trip.rs` (`table_names`, `checksum`) moves to `tests/common/backup.rs` for AC3–AC5 (DRY).

Notes for the lead (no change made):
- "A v1.28 backup goes through the migrations": no version before 1.29.0 writes backups. Read as "a backup written by an older release" (AC4 builds one by removing the newest migration from a database before backing it up).
- AppState is not reachable from the per-scan metadata fetch (`spawn_fetch` takes a future over the pool): T7's generation check needs the counter passed in, or read from a process-wide static like 19-5's `WATCH`. The dev chooses; the AC6 unit test fixes the behaviour.
- A restored backup with no active Admin (for example one written before the setup wizard finished) opens the wizard, or needs `MYBIBLI_RESET_ADMIN`; not refused by D5 (the counts are not in the manifest). The manual (19-11) can say so.
- Browsers may keep a replaced cover image cached under the same URL until it expires; cosmetic.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (dev-story, 2026-11-26).

### Debug Log References

- Gates run locally: `cargo clippy --all-targets -- -D warnings` (clean), `cargo test --lib` (1976 passed, 1 ignored), `cargo test --test integration` full (1011 passed: `backup_restore::` 9, `backup_restore_maintenance::` 4, `locale_parity::`, `every_test_file_is_declared`), `cargo sqlx prepare --check --workspace -- --all-targets` (unchanged: only dynamic queries added), `npx tsc --noEmit`, the `waitForTimeout` grep, `vcode-literals.sh`.
- **Not run: the E2E suite** (stack busy). `admin-backup-restore.spec.ts` is written and typechecked; status stays in-progress until `CI=true npx playwright test --workers=2` is green.

### Completion Notes List

- T1–T12 implemented as written. `services::startup::prepare_database` takes the boot steps after the migrations; boot order changed slightly: the `MYBIBLI_RESET_ADMIN` hatch and the startup purge now run after all of them (both were between them before; no behaviour depends on it).
- T6: `Maintenance` holds its drain timeout (`DRAIN_TIMEOUT` 30 s by default, `with_drain` for tests). Requests are admitted with `try_read` after checking the flag, and the flag is read again once admitted.
- T7 (choice left to the dev): no process-wide static (parallel integration tests share the process). The per-scan fetches get the handle through `spawn_fetch(&maintenance, …)`; the fetch, the bulk run and the retry tick run inside `maintenance::scoped` (a tokio task-local carrying the handle and the generation), and each write section asks `maintenance::write_permit()`, which also holds the gate's read guard while it writes (a restore drains it). The auto-purge and anonymous-session purge skip their run while active and hold the gate while they run. The provider chain's own cache write (code-keyed) is not gated: one landing mid-load would at worst make the load fail → rollback.
- The success / rollback dialog needs no change to `modal.js`: htmx re-dispatches `afterRequest` on `#modal-slot` once the form is detached, so `modal.js` does not close; its slot observer opens the new dialog, whose "Log in again" link carries `data-modal-default-focus`.
- Restore checks: verify runs before `instance_id::ensure`, so a refused restore writes nothing at all.
- R4 test skips itself when the file stays readable after `chmod 000` (root).
- E2E (AC9): project `instance-restore`, after `instance-session-timeout`; the spec restores the backup it wrote moments before, so the stack stays usable (the library it found plus its title back, seeded users kept by `MYBIBLI_SEED_DEV_USERS=1`).

### File List

- `src/db.rs` (MIGRATOR), `src/main.rs`, `src/services/mod.rs`, `src/services/startup.rs` (new), `src/services/maintenance.rs` (new)
- `src/services/backup/mod.rs`, `src/services/backup/status.rs`, `src/services/backup/restore/{mod,verify,load,covers}.rs` (new)
- `src/middleware/mod.rs`, `src/middleware/maintenance.rs` (new), `src/middleware/locale.rs`
- `src/routes/mod.rs`, `src/routes/admin_backups.rs`, `src/routes/admin_backups_restore.rs` (new), `src/routes/admin_health_backup.rs`, `src/routes/admin.rs`, `src/routes/catalog.rs`
- `src/models/admin_audit.rs`, `src/tasks/{metadata_fetch,metadata_retry,auto_purge_scheduler,anonymous_session_purge}.rs`
- `templates/fragments/admin_backups_panel.html`, `templates/fragments/admin_backups_list.html` (new), `templates/fragments/admin_backups_restore_modal.html` (new), `templates/fragments/admin_backups_restore_done.html` (new), `templates/pages/maintenance.html` (new)
- `locales/{en,fr,de,it}.yml`
- `tests/integration.rs`, `tests/common/backup.rs`, `tests/backup_round_trip.rs`, `tests/backup_restore.rs` (new), `tests/backup_restore_maintenance.rs` (new)
- `tests/e2e/playwright.config.ts`, `tests/e2e/specs/journeys/admin-backup-restore.spec.ts` (new)
- `CLAUDE.md`, `docs/auth-threat-model.md`, `_bmad-output/implementation-artifacts/sprint-status.yaml`

### Change Log

- 2026-11-26 — create-story: story 19-8 written, decisions D1–D13 for the project lead.
- 2026-11-26 — validate-story: checked against the code; T9–T12 added, D5 and T6–T7 corrected (see Validation).
- 2026-11-26 — dev-story: implemented; unit, integration and static gates green; E2E written, not yet run (status in-progress).
