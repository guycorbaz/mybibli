# Story 19.11: Documentation (epic 19)

Status: done

No GitHub issue of its own (collects the "for the manual" notes of #657, #658, #733, #740, #744). Covers FR178–FR182 (documentation) and retrospective v1.28.1 action A4. Epic 19; ships in v1.29.0 (with Epic 20). Sprint-status key: `19-11-epic-19-documentation`.

**Shipped with story 20-7 in one change** (branch `story/19-11-20-7-documentation`, one PR): both stories edit `docs/manual/{en,fr}/*.tex`, and the PDFs are rebuilt once for both (epics.md, Epic 20 couplings, "Manual and PDFs"). Story file of the other half: `20-7-epic-20-documentation.md`.

## Story

As an operator who is not an IT specialist,
I want the manual to explain backup and restore step by step,
so that I can set it up and use it on the day I need it.

## Context

What the manual already holds (written by the feature stories, PDFs not rebuilt since):
- **19-5** — chapter 2, "Production watching" (`sec:health-watch`): the Health-tab panel, its four counts, thresholds, reset on restart.
- **19-6** — chapter 7, section "Backups" (`sec:backups`, "Since 1.29.0"): mounting the folder, "Back up now", what the file holds, "The file is sensitive", loading `database.sql` into another MariaDB, a cover written during the backup; chapter 2, the Backups item of the tab list.

What is missing (19-7, 19-8, 19-9 and the notes they left for this story):
- **19-7 (schedule and rotation)** — Admin → System → Backups: "Automatic backup" (Off / Every hour / Every 2, 3, 4, 6, 8, 12 hours / Once a day; default every hour), "Daily backups to keep" (1–365, default 14), "Save automatic backup"; a check 10 minutes after start then each interval, a backup only when something changed since the last one this process wrote; rotation keeps everything of the last 24 hours, the newest, and the last backup of each of the N most recent days *on which a backup was written* (instance time zone); sidecars follow; **another instance's backup is never deleted**; lowering N deletes the older daily backups at the next rotation (#733 item 2 — the saved line now names both values, commit `7b2d1afa`). Health tab: last backup, next check, off, last check unchanged, the folder warnings, a failed automatic backup. **One backups folder per instance**; never point a test instance at the production folder (19-7 D8, comment on #729).
- **19-8 (restore)** — Admin → Backups, "Backups in the folder" (Date, Origin, Size, Note; "Restore…"); the modal and its facts; the checks and refusals (checksum, missing `.sha256`, newer version, not readable, not a backup, busy, bulk run, gone, drain timeout, safety backup failed); the safety backup; maintenance (503 page, a minute or two); everyone logged out; the instance id kept; a foreign backup listed as "Another instance (id)", its accounts replacing the local ones; a restored foreign backup stays in the folder for good — **delete it by hand once no longer needed** (D3); "Removed at the next rotation" — **a hand-copied old backup: restore it right away, or set "Automatic backup" to Off first and back afterwards** (D4); rollback after a failure; the crash journal `.restore-in-progress.json` and the boot recovery; the stuck case naming **"Restoring by hand"** / « Restaurer à la main » (the maintenance page's copy points at this exact section name); Health tab "Last restore" lines; a backup with no active Admin opens the setup wizard or needs `MYBIBLI_RESET_ADMIN` (19-8 review note).
- **#740** — a reverse proxy with a 60 s read timeout shows "Server error (504)" while the restore goes on; the double failure (audit row + journal removal) leaves the journal, and the next boot rolls the successful restore back: check the folder after a restore and remove a leftover `.restore-in-progress.json` by hand once the restored library is confirmed. Item 1 (a panic after a successful restore shows the "by hand" copy; a restart is enough) gives the first step of "Restoring by hand": look for the journal first.
- **19-9 (cover check, not merged yet — branch `story/19-9-covers-consistency`)** — Admin → Backups → "Cover check": "Check covers", the two lists, the notes, "Left alone…", "Repair…" and its modal; D12 "folder looks unmounted" (no repair offered: fix the mount, check again); the "looks like another database" guard asked by the project lead in review item (b) — **not in the branch yet**: documented as the brief describes (fix the database the app points at; move files by hand only when sure), to be checked against the merged code; **#744** — the first check on an existing library lists the cover files of purged titles (normal, repair removes them).
- **Retro v1.28.1 A4** — Rule #19 of `CLAUDE.md` must name the manual's environment-variable table and the "current release" sentence of `website/roadmap.html`.

## Acceptance Criteria

1. **AC1 — mount (new and existing installs).** Chapter 1 has a step for the backups folder in Method 1 (and the external-database method points to it) with the `docker-compose.yml` line `- ./backups:/app/backups`; chapter 7 says how to add it to an older compose file (exists, kept); the environment-variable table lists `MYBIBLI_BACKUP_DIR`.
2. **AC2 — permissions and sensitivity.** The manual says who owns the files on the host (the container's user, root in the shipped image), their mode (`0600`), how to keep the folder private and what the file holds (exists in chapter 7, extended to copies made elsewhere).
3. **AC3 — schedule and rotation.** A subsection in chapter 7 with the exact labels of Admin → System → Backups, the skip rule, the rotation rule, the never-deleted foreign backups and "one folder per instance", and the Health-tab backup lines.
4. **AC4 — step-by-step restore.** A numbered procedure in chapter 7 from the Backups tab to logging in again, with the refusals explained, the safety backup, the maintenance page, the reverse-proxy 504, checking the folder for a leftover journal, a hand-copied backup (D4 advice), a foreign backup (accounts, delete by hand).
5. **AC5 — "Restoring by hand".** A section of that exact name (FR « Restaurer à la main ») in chapter 7: when it applies, the journal check, the restart that retries on its own, then the manual procedure (stop, check the checksum, load `database.sql`, put the covers back, remove the journal, start, run the cover check).
6. **AC6 — another NAS and a test database.** Moving a backup to another NAS (copy archive and `.sha256`, same or newer version, wizard first on a fresh install, restore, delete by hand); loading a backup into a test database (Rule #20 corpus) with the "never the production folder" warning; the sentence "Restoring a backup into the running library comes with a later version." removed.
7. **AC7 — cover check.** A section in chapter 7: what it compares, the lists and notes, the repair and its modal, the unmounted guard, the other-database guard (to be checked after 19-9 merges), the purged titles of #744; the "cover written during the backup" paragraph points to it.
8. **AC8 — troubleshooting.** Chapter 10 gains a "Backups and restore" section with the failure cases (folder missing / not writable, restore refusals, 504 behind a proxy, maintenance page stuck, a restore undone at the next boot, cover check refusing to repair).
9. **AC9 — Health-tab watching panel.** Already documented by 19-5 (`sec:health-watch`); chapter 7 refers to it where the Health tab is mentioned. No duplicate text.
10. **AC10 — compose and env.** `docker-compose.yml` and `.env.example` carry the mount (check; they do since 19-6).
11. **AC11 — CLAUDE.md Rule #19 (A4).** The checklist names the manual's environment-variable table (`docs/manual/{en,fr}/01-installation.tex`, `sec:env-vars`) and the "The current release is …" sentence of `website/roadmap.html`.
12. **AC12 — French.** The French manual says the same in careful French, with the French UI labels word for word (`locales/fr.yml`).
13. **AC13 — PDFs.** Rebuilt once with 20-7's changes (`./docs/manual/build.sh`); no undefined reference, no new overfull box.
14. **AC14 — not in this story.** Version numbers, "What's new" chapters, the "Current version" line and the upgrade note on the new mount are release work (Rule #19, v1.29.0).

## Tasks / Subtasks

- [x] T1 — Chapter 1 EN/FR: backups-folder step in Method 1, a pointer from Method 2, `MYBIBLI_BACKUP_DIR` row (AC1, AC2).
- [x] T2 — Chapter 7 EN/FR: schedule and rotation; restore step by step; a backup from elsewhere (another NAS, a hand copy, foreign backups); restoring by hand; cover check; loading into a test database updated; the "cover written during the backup" pointer (AC3–AC7, AC9).
- [x] T3 — Chapter 10 EN/FR: "Backups and restore" section (AC8).
- [x] T4 — Check `docker-compose.yml`, `.env.example` (AC10).
- [x] T5 — `CLAUDE.md` Rule #19 (AC11).
- [x] T6 — Build the PDFs once with 20-7 (AC13), read the log.

## Dev Notes

- Copy: quote the UI labels exactly (`locales/en.yml`, `locales/fr.yml`); the cover-check labels come from the 19-9 branch (`git show story/19-9-covers-consistency:locales/en.yml`).
- Labels differ between the EN and FR sources (e.g. `sec:troubleshoot-covers-missing` vs `sec:troubleshoot-couvertures-manquantes`): check each `\ref` in its own language tree.
- Build: `flock -w 10800 /tmp/station-heavy.lock mem-guard ./docs/manual/build.sh` (both languages). No cargo, no formatter.
- **19-9 is not merged**: its section is written from the branch; after the merge, compare the section with the merged copy and the other-database guard (may need a touch-up). The story stays in-progress until then.

### References

- `_bmad-output/planning-artifacts/epics.md` (Epic 19, Story 19.11; Epic 20 couplings).
- `_bmad-output/implementation-artifacts/19-5-production-watching.md`, `19-6-writing-a-backup.md`, `19-7-scheduled-backups.md`, `19-8-restoring-a-backup.md`; `19-9-cover-database-consistency.md` on `story/19-9-covers-consistency`.
- Issues #657, #658, #729, #733, #740, #744.
- `locales/en.yml`, `locales/fr.yml` (`admin.backups.*`, `admin.covers.*` on the 19-9 branch, `admin.health.backup.*`, `admin.system.backup_*`, `error.backup.*`, `error.restore.*`, `maintenance.*`).
- `docker-compose.yml`, `.env.example`, `Dockerfile` (no `USER`: the container runs as root).

## Validation (2026-11-26)

Checked against `main` (`3ad5f343`) and `story/19-9-covers-consistency` (`d2e4d40f`): `locales/{en,fr}.yml`, `docker-compose.yml`, `docker-compose.dev.yml`, `.env.example`, `tests/e2e/docker-compose.test.yml`, `Dockerfile`, `src/routes/mod.rs`, `src/main.rs`, `src/middleware/maintenance.rs`, `src/tasks/backup_scheduler.rs`, `src/config.rs`, `src/services/backup/{mod,rotation}.rs`, `src/services/backup/restore/mod.rs`, the manual (`docs/manual/{en,fr}/01, 02, 07, 10, 12`), `website/roadmap.html`, `CLAUDE.md` Rule #19.

Checked, found correct:
- **V1 — mount (AC10).** `docker-compose.yml` carries `- ./backups:/app/backups` (with its comment), `.env.example` documents `MYBIBLI_BACKUP_DIR` (commented, default `/app/backups`), `docker-compose.dev.yml` mounts it, the E2E stack uses a tmpfs. Nothing to change there.
- **V2 — labels quoted in the manual**, EN / FR, from the locales:
  - Admin → Backups: "Back up now" / « Sauvegarder maintenant »; folder line "Folder: … — ready / missing / not writable" / « Dossier : … — prêt / absent / non accessible en écriture »; "Backups in the folder" / « Sauvegardes présentes dans le dossier »; columns "Date", "Origin", "Size", "Note" / « Date », « Origine », « Taille », « Remarque »; "Restore…" / « Restaurer… »; "This instance" / « Cette instance », "Another instance (%{id})" / « Autre instance (%{id}) »; notes "No checksum file" / « Pas de fichier de contrôle », "Not readable" / « Illisible », "Removed at the next rotation" / « Supprimée à la prochaine rotation ».
  - Modal: "Restore this backup?" / « Restaurer cette sauvegarde ? »; "Cancel" / « Annuler »; "Restore this backup" / « Restaurer cette sauvegarde »; "Log in again" / « Se reconnecter ».
  - Admin → System → Backups / Système → Sauvegardes: "Automatic backup" / « Sauvegarde automatique » (Off, Every hour, Every %{n} hours, Once a day / Désactivée, Toutes les heures, Toutes les %{n} heures, Une fois par jour), "Daily backups to keep" / « Sauvegardes quotidiennes à garder », "Save automatic backup" / « Enregistrer la sauvegarde automatique ». `ALLOWED_BACKUP_INTERVALS` = 0, 1, 2, 3, 4, 6, 8, 12, 24; default 1 h; retention 1..=365, default 14.
  - Health tab / Santé: "Backups" block (`admin.health.backup.*`: "Last backup", "Next check: around …", "Automatic backup is off (Admin → System).", warnings starting "Warning:" / « Attention : », "Last restore: …" / « Dernière restauration : … »).
  - Maintenance page: "Maintenance"; stuck copy names the manual section “Restoring by hand” / « Restaurer à la main » — the section titles must be exactly these.
  - Cover check (19-9 branch, `admin.covers.*`): "Cover check" / « Contrôle des couvertures », "Check covers" / « Contrôler les couvertures », "Repair…" / « Réparer… », "Repair the covers?" / « Réparer les couvertures ? », "Repair" / « Réparer », the two list headings, the four notes, the unmounted sentence.
- **V3 — scheduler.** `WARM_UP_SECS` = 600 (first check 10 minutes after start), a wake every 60 s; a check writes only when something changed since the last backup *this process* wrote (after a restart the first due check writes one).
- **V4 — maintenance.** Every request gets 503 except `/health` and `/static/*` (`middleware/maintenance.rs::is_passthrough`); covers are refused too.
- **V5 — boot recovery.** `main.rs` runs `restore::recover_at_boot` before the migrations; on failure it logs `error` and exits 1. With the compose `restart: unless-stopped`, that is a restart loop until the restore is done by hand — the manual says so.
- **V6 — routes.** `/admin/backups`, `/admin/backups/restore/{name}`, `/admin/system/backups`; the restore name must have the backup shape.

Changed (no product decision needed):
- **V7 — who owns the files.** The image has no `USER` line: the container runs as root, so on the host the backups are owned by root with mode `0600` (`create_private`). The manual says to copy them with `sudo` (or as the NAS administrator), and that Docker creates a missing `./backups` itself (as root) when the mount line is present — "missing" on the Backups tab therefore means the mount line is absent (or `MYBIBLI_BACKUP_DIR` points elsewhere). AC2 made explicit.
- **V8 — environment table.** `MYBIBLI_BACKUP_DIR` is missing from `sec:env-vars` (chapter 1, EN and FR) — exactly the A4 lesson; the row is added in this story (AC1).
- **V9 — test database.** The 19-6 subsection ends with "Restoring a backup into the running library comes with a later version." — replaced (AC6).
- **V10 — "Restoring by hand" begins with the journal.** #740 item 1: after a successful restore a panic may show the stuck copy although the journal is gone; following the copy would undo the restore. The section therefore starts with "is `.restore-in-progress.json` still in the folder?" (absent → restart only), then "restart once: mybibli tries the rollback itself", and only then the procedure.

Notes for the project lead (no change made):
- **N1 — the "looks like another database" guard** is the 19-9 review item (b), waiting for the lead; it is not in the branch. The manual documents it as briefed (fix the database the app points at; move files by hand only when sure) in a paragraph marked in the source (`% 19-9 review (b)`), to be removed or reworded if 19-9 ships without it or with other copy.
- **N2 — restoring by hand loads into the live schema.** The procedure drops and recreates the database as MariaDB `root` (bundled stack), as the existing "Restore procedure" section does; the `mybibli` user cannot create a database.

## Review (2026-11-26)

Self-check after story 19-9 merged (`8eedb650`, main merged into this branch as `dd68623a`), against the merged `locales/en.yml` / `locales/fr.yml` and templates:
- Every label and message quoted in chapters 1, 2, 7 and 10 (EN and FR) was looked up in the merged locales by script: all present word for word — Backups tab, list, notes, modal, refusals, maintenance and stuck copy, System → Backups labels, Health lines, the 504 line (`errors.server_error_retry`), and the merged `admin.covers.*` (heading, check, lists, notes, `unmounted`, `another_database`, repair modal, `repair.failed`, `repair.stopped`).
- `templates/fragments/admin_covers_check.html`: no "Repair…" when the folder looks unmounted or the database looks like another one, as written; the cover-check section is included in the Backups panel; rows are "name · size · date" and capped at 50 (`LIST_LIMIT`), as written.
- `services/cover_consistency.rs`: unmounted = more than half of the local covers missing; another database = an orphan numbered at or above the next title id (`never_given`), as written after the lead's change.
- #744 still open: "the purge leaves their cover files behind" stays true.
- #746: the saved line names both values ("Automatic backup saved: … Daily backups to keep: …"), as the manual says.
No text change was needed by this check.

### Review — independent pass (2026-11-26)

Three Medium and seven Low findings, all fixed (EN and FR):
- **M1** — `-p"$MYSQL_ROOT_PASSWORD"` expanded in the host shell (empty, then a password prompt that `exec -T` cannot answer). Every `mariadb` / `mariadb-dump` call through `docker compose exec` now runs in `sh -c '…'` inside the `db` container, which has `MYSQL_ROOT_PASSWORD` and `MYSQL_DATABASE` (the names in `docker-compose.yml`): restoring by hand, the old backup and restore procedures (ch. 7), the last-admin SQL fix (ch. 10).
- **M2** — root-owned `0600` files in a `700` folder: `sudo` added to the checksum check, the test-database recipe (`sudo tar`, `sudo cat … | mariadb`) and restoring by hand (one `sudo sh -c 'cd backups && sha256sum -c … && tar -xzf …'`).
- **M3** — a cover downloaded while the backup was written has a number at or above the dumped next title id: after restoring that backup the repair stays blocked by the wrong-database guard; said in "A cover written during the backup" (pointing to the guard) and in the cover-check note (also a file left by a restore by hand).
- **Lows** — the double failure named (audit row and journal removal; see the log) in ch. 7 and 10; restore-by-hand step 4 "unless it reports one of its guards"; the scan banner quoted as in the locales ("This ISBN is on your wish list. Remove it?" / « Cet ISBN est sur votre wish list. La retirer ? »); "the location's pencil"; API description "as typed (leading and trailing spaces removed)"; "Recommended cadence" rewritten (automatic backup covers database and covers; `.env` and an off-site copy of `backups/` with `sudo` remain); FR wording (« appliquent chacune leur rotation aux sauvegardes de l'autre », « vers cette version », « Cherchez le fichier `.restore-in-progress.json` »).

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (dev-story, 2026-11-26).

### Debug Log References

- Build logs: `docs/manual/{en,fr}/main.log` (not committed). No undefined reference. Overfull boxes on changed lines: two left, EN ch. 10 (1.6 pt) and FR ch. 10 (5.4 pt), both on the `./backups:/app/backups` path; overfull boxes attributed to changed lines with a small script over `git diff -U0 main`.

### Completion Notes List

- **Stays in-progress:** story 19-9 is not merged. Its section (chapter 7, "Cover check" / « Contrôle des couvertures », `sec:cover-check`) and the chapter 10 symptom were written from `story/19-9-covers-consistency` (`d2e4d40f`). After the merge: compare the labels with the merged `admin.covers.*` copy, and settle the paragraph "When the library looks like another database" / « Quand la bibliothèque semble être une autre base » (marked `% 19-9 review (b)` in both sources) — it documents a guard the branch does not have yet (review item (b), waiting for the lead); remove or reword it to match what ships.
- Chapter 1: step "1d. The backups folder" / « 1d. Le dossier de sauvegarde » (`sec:install-backups`): the compose line, `mkdir -p backups && chmod 700 backups`, root ownership, `0600`, another host path, one folder per instance, existing installs; Method 2 keeps the line; `MYBIBLI_BACKUP_DIR` row in `sec:env-vars`.
- Chapter 7 (`sec:backups` extended): one folder per instance; "Automatic backups and rotation" (`sec:backups-schedule`); "Who owns the files"; "Loading a backup into a test database" (`sec:backups-test-db`, the "later version" sentence removed); new sections "Restoring a backup" (`sec:backups-restore`: step by step, refusals, rollback, reverse proxy 504 with the real copy "Server error (504) — please try again." / « Erreur serveur (504) — veuillez réessayer. », after the restore and the leftover journal, a backup copied in by hand, another instance, moving to another NAS `sec:backups-move`), "Restoring by hand" / « Restaurer à la main » (`sec:restore-by-hand`), "Cover check" / « Contrôle des couvertures » (`sec:cover-check`).
- Chapter 2: Backups tab item (restore, cover check), System → "Backups" subsection pointing to the schedule.
- Chapter 10: section "Backups and restore" / « Sauvegardes et restauration » (`sec:troubleshoot-backups`), eight symptoms.
- Chapter 6: row "Back up, restore a backup, check covers" (Admin).
- `CLAUDE.md` Rule #19: two checklist items (manual env-var table, roadmap "current release" sentence).
- `docker-compose.yml` / `.env.example`: already carry the mount (V1), unchanged.
- "Restoring by hand" puts the covers back with `docker compose run --rm --no-deps --entrypoint sh mybibli -c …` (the app image with its own volumes) rather than the `alpine` + `-v mybibli_covers:` pattern of the older sections: a compose named volume is really `<project>_mybibli_covers`, so `-v mybibli_covers:` would mount a new empty volume unless the project is named so. The older "Backup procedure" / "Restore procedure" sections still use that pattern — **not changed here** (outside this story), noted for the lead.
- Health-tab watching panel: already in chapter 2 (`sec:health-watch`, 19-5); chapter 7 refers to it.

### File List

- `docs/manual/en/01-installation.tex`, `docs/manual/fr/01-installation.tex`
- `docs/manual/en/02-configuration.tex`, `docs/manual/fr/02-configuration.tex`
- `docs/manual/en/06-roles-permissions.tex`, `docs/manual/fr/06-roles-permissions.tex`
- `docs/manual/en/07-backup-restore.tex`, `docs/manual/fr/07-backup-restore.tex`
- `docs/manual/en/10-troubleshooting.tex`, `docs/manual/fr/10-troubleshooting.tex`
- `docs/manual/mybibli-manual-en.pdf`, `docs/manual/mybibli-manual-fr.pdf` (rebuilt, with 20-7)
- `CLAUDE.md`
- `_bmad-output/implementation-artifacts/sprint-status.yaml`, this story file

### Change Log

- 2026-11-26 — dev-story (with 20-7): manual EN/FR, CLAUDE.md Rule #19, PDFs rebuilt. Stays in-progress until 19-9 merges.
- 2026-11-26 — follow-up (coordinator): the older "Backup procedure" / "Restore procedure" commands use the real Compose volume name (`mybibli_mybibli_covers`, how to find it with `docker volume ls | grep mybibli_covers`); the "another database" guard documented with its approved copy (since changed again by the lead: an orphan file numbered at or above the next title id blocks the repair; purged titles' files stay repairable; no way around it), the two new repair lines (files not deleted: folder not writable; repair stopped by a restore), `% 19-9 review (b)` markers removed. PDFs rebuilt. A last comparison with the merged 19-9 copy is still due.

- 2026-11-26 — validate-story: see Validation.
- 2026-11-26 — create-story (with 20-7).
- 2026-11-26 — review: checked against the merged copy (19-9 merged); done.
- 2026-11-26 — independent review: M1–M3 and L1–L7 fixed, PDFs rebuilt.
