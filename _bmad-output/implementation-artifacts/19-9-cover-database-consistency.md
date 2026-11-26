# Story 19.9: Cover / database consistency (closes #658)

Status: in-progress

## Story

As an Admin,
I want to find and repair covers that do not match the database,
so that no title points to a missing image and no stray file stays on disk.

## Context

Covers FR182 and UX-DR46 (cover repair through the UX-DR8 modal). The rules recorded on #658 are the specification:
- **orphan file**: a cover file with no title in the database (neither live nor in the Trash) → deleted;
- **missing file**: a title whose `cover_image_url` points to a local file that does not exist → address cleared and a cover re-download queued;
- positive shape only (Rule #20): it acts only on files named `<digits>.jpg` at the root of the covers folder; never on `.staging/` nor anything else;
- covers of titles in the Trash are kept (they leave with the purge);
- it first shows the list of what it would change, and acts only after explicit confirmation (UX-DR8 modal);
- each run writes an `admin_audit` row with the counts.

**Developed after story 19-8 merges** (branch `story/19-8-restore`, in review). 19-8 changes three things this story relies on:
- **Restore places covers itself** (19-8 D11): after a restore through mybibli, the covers folder holds exactly the backup's `<digits>.jpg`. This tool is for the current production state (the 2026-11-09 incident) and for any restore done outside mybibli (NAS backup of MariaDB alone).
- **Maintenance gate** (19-8 D8, `middleware/maintenance.rs`): while a restore runs every request gets 503, so the check and the repair cannot start during a restore. A repair already admitted holds its request's maintenance read scope, so a restore waits for it (drain). Anything the repair spawns must be spawned in the captured scope (`maintenance::current_scope` / `rescoped`) and write under `maintenance::write_permit()` (19-8 review L7).
- **Backup guard** (`BackupConfig::guard`, held by a backup, a rotation and a whole restore, 19-8 T2): the repair takes it with `try_lock_owned` for its write phase, so it never deletes a file while a backup is copying the covers folder or a restore is replacing it.

What exists (on `main`, 19-6 / 19-7 merged):
- **Covers folder**: `AppState.covers_dir` (`COVERS_DIR`, default `./covers`, created at boot by `main.rs`), served at `/covers` (`routes/mod.rs`, dot-segments refused by `deny_hidden_cover_paths`). Staging under `<covers_dir>/.staging` (`cover_staging::staging_dir_for`).
- **Cover writes**, all `<covers_dir>/<title_id>.jpg`: `CoverService::process_and_save_bytes` (`services/cover.rs`), `tasks::metadata_fetch::store_fetched_cover` (temp `.tmp-<32 hex>` then rename), `cover_staging::promote` (accepted proposal), the hand upload (`routes/title_cover_upload.rs`), the re-download confirm (`routes/title_metadata_confirm.rs`).
- **Backup cover listing**: `services::backup::covers::{is_cover_name, list_covers}` (regular files only, links skipped).
- **Bulk cover re-fetch** (Health tab, "Re-fetch missing covers"): eligible = live title, `cover_image_url IS NULL OR ''`, at least one of ISBN / ISSN / UPC (`TitleModel::{count_missing_covers, list_missing_covers}`).
- **Metadata retry queue** (story 13-2/13-3): `MetadataRetryModel::enqueue_fresh` queues a title due now; `tasks/metadata_retry.rs` serves ≤ 20 rows per 5-minute tick, paced, skipped while a bulk run is in progress, through the scan's chain (`manually_edited_fields` respected).
- **Audit**: `AdminAuditModel::create(pool, user_id: u64, action, entity_type, entity_id, details)`.
- **Backups tab**: `routes/admin_backups.rs`, `fragments/admin_backups_panel.html` (folder line, "Back up now", `#admin-backups-feedback`; 19-8 adds `#admin-backups-list`).

Practice A3: every user-visible behaviour and copy below not settled on #658 is a numbered decision, settled with the project lead before the first review pass.

## Decisions for the project lead

**Approved by the project lead, 2026-11-26: D1–D13 all as recommended (A), EN and FR copy as written.**

Each D has a recommendation; one word ("A", "yes", "as proposed") is enough. EN and FR copy is given in full; DE and IT are translated at dev time from EN/FR (as in 19-6 – 19-8) and reviewed in the code review.

- **D1 — where the tool lives.**
  - **A (recommended):** Admin → Backups, a section "Cover check" under the list of backups (UX-DR46 already places cover repair there).
  - B: Admin → Health, next to "Re-fetch missing covers".
  - Copy: heading "Cover check" / « Contrôle des couvertures »; intro "Compares the covers folder with the library: cover files that no title owns, and titles whose cover file is missing. The check itself changes nothing." / « Compare le dossier des couvertures avec la bibliothèque : les fichiers de couverture qu'aucun titre ne possède, et les titres dont le fichier de couverture manque. Le contrôle lui-même ne modifie rien. »; button "Check covers" / « Contrôler les couvertures ».
- **D2 — what the check shows.**
  - **A (recommended):** two lists, then one line for what is left alone. Each list shows its first 50 rows, then "… and %{count} more." / « … et %{count} de plus. »
    - "Cover files that no title owns (%{count})" / « Fichiers de couverture qu'aucun titre ne possède (%{count}) »; row: file name, size, date modified (instance zone).
    - "Titles whose cover file is missing (%{count})" / « Titres dont le fichier de couverture manque (%{count}) »; row: the title, linked to its page, with the notes of D4–D7.
    - "Left alone: %{others} other entries in the covers folder, %{remote} titles whose cover address has another form." / « Laissés tels quels : %{others} autres éléments dans le dossier des couvertures, %{remote} titres dont l'adresse de couverture a une autre forme. » (shown only when one count is above 0). *(Wording changed at validation.)*
    - Order: files newest first; titles by name.
    - Nothing to repair: "Covers and library match: %{files} cover files checked, nothing to repair." / « Les couvertures et la bibliothèque concordent : %{files} fichiers de couverture contrôlés, rien à réparer. »
    - Folder not readable: "mybibli cannot read the covers folder %{path}, so nothing was checked. Give the container's user access to that folder, then try again." / « mybibli ne peut pas lire le dossier des couvertures %{path} ; rien n'a été contrôlé. Donnez au compte du conteneur l'accès à ce dossier, puis réessayez. »
  - B: counts only, no lists.
- **D3 — a cover file of an existing title that its address does not use** (title live or in the Trash, `cover_image_url` empty or another address).
  - **A (recommended):** left alone and not reported: the next cover written for that title replaces it, and a file always exists a moment before its address is recorded (fetch, upload, accepted proposal), so deleting it could race a cover being installed.
  - B: treated as an orphan.
- **D4 — missing file, title in the Trash.**
  - **A (recommended):** the address is cleared, no download is queued (a restored title then shows "no cover" and joins "Re-fetch missing covers"). Note "in the Trash" / « dans la corbeille ».
  - B: left alone (it leaves with the purge).
- **D5 — missing file, cover set by hand** (uploaded, or from an accepted proposal: `manually_edited_fields` holds `cover_image_url`).
  - **A (recommended):** the address is cleared, the "set by hand" mark is kept, no download is queued (a download would never replace a hand-set cover anyway). Note "set by hand: upload it again" / « choisie à la main : téléversez-la à nouveau ».
  - B: the mark is removed and a download queued, so a provider image may replace the lost hand-chosen one.
- **D6 — how the re-download is queued.**
  - **A (recommended):** each cleared live title with an ISBN, ISSN or UPC is queued in the metadata retry queue, due now (as the 13-3 ISBN conversion does); the retry task downloads about 20 titles every 5 minutes, paced. Like the bulk re-fetch, the lookup also refreshes the title's metadata not set by hand. Note for a title without a code: "no ISBN, ISSN or UPC: nothing to download" / « ni ISBN, ni ISSN, ni UPC : rien à télécharger ».
  - B: nothing queued; the cleared titles join "Titles with missing covers" on the Health tab and the Admin presses "Re-fetch missing covers".
- **D7 — missing file, a cover proposal pending on the title.**
  - **A (recommended):** left alone (clearing the address would make the proposal stale at acceptance, since every cover change outdates a cover proposal); note "a cover proposal is pending: accept it to install a cover" / « une proposition de couverture est en attente : acceptez-la pour installer une couverture ».
  - B: cleared anyway; the proposal becomes stale.
- **D8 — the repair and its confirmation (UX-DR8, `#modal-slot`, Pattern A).**
  - After a check with something to repair, a button "Repair…" / « Réparer… » opens the modal. Title "Repair the covers?" / « Réparer les couvertures ? »; body "%{orphans} cover files that no title owns will be deleted. %{cleared} titles lose the address of their missing cover; %{queued} of them are queued for a new download. A deleted file can only come back from a backup." / « %{orphans} fichiers de couverture qu'aucun titre ne possède seront supprimés. %{cleared} titres perdent l'adresse de leur couverture manquante ; %{queued} d'entre eux sont mis en file pour un nouveau téléchargement. Un fichier supprimé ne peut revenir que d'une sauvegarde. »; buttons "Cancel" / « Annuler » — "Repair" / « Réparer ».
  - The repair acts **only on what the check listed**; each item is checked again just before it is changed, and an item that changed meanwhile is left alone.
  - Success (in the tab, the check result refreshed): "Covers repaired: %{deleted} files deleted, %{cleared} cover addresses cleared, %{queued} downloads queued." / « Couvertures réparées : %{deleted} fichiers supprimés, %{cleared} adresses de couverture effacées, %{queued} téléchargements mis en file. » plus, when some were skipped, "%{skipped} items had changed since the check and were left alone." / « %{skipped} éléments avaient changé depuis le contrôle et ont été laissés tels quels. »
  - **Confirmation style. A (recommended):** the explicit button alone. B: type-to-confirm (as the Trash's permanent delete).
- **D9 — a backup before the repair.**
  - **A (recommended):** none; the modal says a deleted file can only come back from a backup (the scheduled backups already hold every cover).
  - B: write a backup first, as the restore does, and refuse the repair when it cannot be written.
- **D10 — audit.**
  - **A (recommended):** one `admin_audit` row `cover_repair` per repair (counts, the Admin's username); a check alone writes none (it changes nothing).
  - B: also one row `cover_check` per check.
- **D11 — a backup or restore running.** The repair is refused while the backup guard is held, with the existing copy of 19-8 (`error.restore.busy`): "A backup or a restore is already running. Nothing was changed. Wait a minute, then try again." **A (recommended):** reuse it. B: a new sentence naming the repair. The check is never refused (read only).
- **D12 — a covers folder that looks unmounted** *(added at validation)*. `main.rs` creates `COVERS_DIR` when it is missing, so a forgotten or failed volume mount gives an **empty** folder: every title with a cover would be listed as missing, and a repair would clear every cover address of the library.
  - **A (recommended):** when more than half of the titles' local covers are missing, the check shows its lists but offers no "Repair…" and says: "%{missing} of the %{total} covers the library uses are missing from %{path}. That looks like a covers folder that is not mounted, so mybibli will not repair anything. Check the covers volume of the installation (see the manual), then check again." / « %{missing} des %{total} couvertures utilisées par la bibliothèque manquent dans %{path}. Cela ressemble à un dossier des couvertures non monté ; mybibli ne réparera donc rien. Vérifiez le volume des couvertures de l'installation (voir le manuel), puis contrôlez à nouveau. » The repair POST applies the same rule again.
  - B: only when the folder holds no cover file at all (same sentence).
- **D13 — the purge leaves cover files behind** *(found at validation)*. Neither the auto-purge nor the Trash's permanent delete removes `<id>.jpg` of a purged title (no `remove_file` on covers in `services/auto_purge.rs` or `routes/admin.rs`): every purged title that had a cover leaves an orphan, today, without any incident. This tool cleans them up, but they keep coming.
  - **A (recommended):** open a change request (the purge removes the cover file of each title it purges, own shape only, after its commit); not in this story.
  - B: do it in this story.

### Technical decisions (not for the lead; may change in review)

- **T1 — modules (Rule #12).** `services/cover_consistency.rs` (pure plan + file and DB steps), `routes/admin_covers.rs` (check GET/POST, modal GET, repair POST), `fragments/admin_covers_check.html`, `fragments/admin_covers_repair_modal.html` (UX-DR8 macro), one include line in `admin_backups_panel.html`.
- **T2 — the plan is pure.** `plan(files: &[CoverFile], titles: &[TitleCover], pending_cover_titles: &HashSet<u64>) -> CoverPlan` over a directory listing and one DB read; unit-tested on synthetic sets. A file named by **any** title's local address is never an orphan (defensive; the app never writes another title's id, but a hand-edited database might). The D12 ratio is computed here.
- **T3 — the DB read includes the Trash** (deliberate exception to `deleted_at IS NULL`, like the backup dump): `SELECT id, cover_image_url, deleted_at IS NOT NULL, manually_edited_fields, isbn/issn/upc presence FROM titles`.
- **T4 — the repair re-checks each item** under the backup guard: an orphan is deleted only if still no title row has its id and its size and modification time equal the listed ones; an address is cleared with `UPDATE titles SET cover_image_url = NULL WHERE id = ? AND cover_image_url = <listed value>` (no `version` bump, like the 13-4 / 17-3 repairs, so no series proposal is outdated) and only if the file is still missing.
- **T5 — the modal form carries the listed items** (orphan ids with size and mtime, title ids with the listed address) as hidden fields; the server re-validates every value (positive shapes) and never trusts them beyond T4's checks.
- **T6 — routes**: `/admin/covers/check` (POST, read only but CSRF-protected like every admin action), `/admin/covers/repair` (GET modal, POST repair). Admin only (`require_role_with_return`).
- **T7 — the repair survives a disconnect** *(added at validation)*. Like 19-6's writer, it runs in a spawned task that owns the guard (spawned in the request's maintenance scope after 19-8, `maintenance::rescoped`); the handler awaits its `JoinHandle`. A closed tab never leaves a half repair without its audit row.

## Acceptance Criteria

1. **AC1 — shapes (Rule #20).** Unit tests: an own cover file name is exactly what the app writes, `format!("{id}.jpg")` for an id ≥ 1 that fits a `u64` (no leading zero, so `0.jpg`, `007.jpg` and a 25-digit name are not own names — stricter than 19-6's `is_cover_name`, which accepts `004217.jpg`), at the folder root, a regular file; `.staging/`, temp files, links, directories and any other name are never listed as orphans nor deleted, only counted (D2). An address is a local cover when it is `/covers/<the title's id>.jpg`, optionally followed by `?v=<digits>`; any other address is counted, never changed.
2. **AC2 — the plan.** Unit tests of `plan` over synthetic sets: orphan (no title live or in the Trash), missing (live, Trash, hand-set, no code, pending cover proposal), D3 file kept, nothing to repair.
3. **AC3 — the check.** Route tests render each state of D2 (match, both lists, "… and N more", left-alone line, unreadable folder); the check changes no file and no row.
4. **AC4 — the repair.** Integration test (`tests/cover_consistency.rs`, declared in `tests/integration.rs`): build an orphan file, a live title with a missing file, a Trash title with a missing file, a hand-set one, one with a pending cover proposal, plus `.staging/x.jpg`, `notes.txt`, a link `9.jpg`; run the repair: the orphan is gone, the others follow D4–D7, the live title is queued in `metadata_fetch_retries`, `titles.version` unchanged, the other entries untouched, one `admin_audit` `cover_repair` row with the counts (D10), details carrying `user_username` / `user_role` like every audit call site (#70). D12: with the covers folder emptied, the check offers no repair and the POST changes nothing. An item changed between check and repair (title created with the orphan's id, address rewritten) is skipped and counted.
5. **AC5 — busy.** The repair is refused while the backup guard is held (D11), nothing changed.
6. **AC6 — access.** Admin only (Librarian → 403 feedback, anonymous → login); CSRF on the POSTs.
7. **AC7 — E2E** (`specs/journeys/admin-cover-check.spec.ts`, from `loginAs`; Rules #3, #23): the spec makes an orphan of its own with existing flows only — a title of its own ISBN, a cover uploaded on its page (`setInputFiles`), the title deleted, then permanently deleted from the Trash (D13: the file stays, named after the id read from `/title/{id}`) — runs the check, finds its own file in the list (never asserts on counts), repairs through the modal (one retry on the D11 refusal, like `backUpNow`: parallel specs write backups), checks again and no longer finds it. A missing file cannot be made from the browser (the covers folder is inside the container): AC4 covers it.
8. **AC8 — gates.** clippy (`-D warnings`), unit and integration tests, `sqlx prepare --check`, `locale_parity::`, `templates_audit`, tsc, the `waitForTimeout` grep, `vcode-literals.sh`, the full E2E suite with `CI=true --workers=2`.

## Tasks / Subtasks

- [x] T1 — `services/cover_consistency.rs`: shapes, pure plan, DB read, repair (T2–T4) + unit tests (AC1, AC2).
- [x] T2 — `routes/admin_covers.rs` + fragments + routes in `routes/mod.rs` + include in the Backups panel (D1, D2, D8).
- [x] T3 — Locales ×4 (copy as approved); `touch src/lib.rs` before the build.
- [ ] T4 — Tests AC3–AC6 (done); E2E spec (AC7) written, tsc / grep / vcode-literals clean — **not run yet** (E2E stack in use).
- [x] T5 — `CLAUDE.md`: a "Cover consistency" bullet (shapes, plan, guard, maintenance scope); the manual is story 19-11.

## Dev Notes

### Guardrails

- Never delete a file whose name is not exactly an own cover name; never follow a link; never touch `.staging/`.
- Never clear an address that is not a local cover of its own title; never bump `titles.version`.
- Blocking file work (`read_dir`, `metadata`, `remove_file`) in `spawn_blocking`.
- Copy word for word once approved (practice A3); Rule #9 for the refusals.
- CSP: no inline script or style; the modal through `templates/components/modal.html`; no `hx-confirm`; CSRF field in every form.
- Heavy commands under `flock -w 10800 /tmp/station-heavy.lock mem-guard …`; never run `cargo fmt`.

### Couplings (Rule #24)

- **19-8**: develop on `main` after 19-8's merge (maintenance scope, guard held by a restore, `#admin-backups-list` in the same panel template).
- **19-6 / 19-7**: read the covers folder; only this story deletes covers outside a restore, and only own cover files.
- **19-10** (epic smoke) does not use this tool; **19-11** documents it.
- `sprint-status.yaml`: only this story's line.

### References

- #658 (rules), #657 (context), epic 19 Story 19.9 in `_bmad-output/planning-artifacts/epics.md`, FR182, UX-DR46, UX-DR8.
- `_bmad-output/implementation-artifacts/19-6-writing-a-backup.md`, `19-7-scheduled-backups.md`, and 19-8 on its branch.
- `src/services/cover.rs`, `src/services/cover_staging.rs`, `src/services/cover_acceptance.rs`, `src/tasks/metadata_fetch.rs`, `src/tasks/metadata_retry.rs`, `src/models/metadata_retry.rs`, `src/models/title.rs`, `src/models/admin_audit.rs`, `src/services/backup/covers.rs`, `src/routes/admin_backups.rs`, `templates/fragments/admin_backups_panel.html`.

## Validation (2026-11-26)

Checked against `main` (19-6, 19-7 merged; 19-8 read on `story/19-8-restore`): `services/cover.rs`, `cover_staging.rs`, `cover_acceptance.rs`, `tasks/metadata_fetch.rs`, `routes/title_cover_upload.rs`, `routes/title_metadata_confirm.rs`, `models/title.rs`, `models/metadata_retry.rs`, `tasks/metadata_retry.rs`, `models/proposal.rs`, `models/admin_audit.rs`, `services/backup/{mod,covers}.rs`, `services/auto_purge.rs`, `routes/admin.rs`, `routes/admin_backups.rs`, the Backups panel, the wish-list schema, `main.rs`, `tests/e2e/docker-compose.test.yml`.

Checked, found correct:
- Every cover write names `<covers_dir>/<title_id>.jpg` and writes the file **before** the address (upload, `store_fetched_cover` inside its transaction, accepted proposal `promote` then `set_cover_url`): a title never points to a file that is about to appear, so "missing" is never a race; the reverse (file first) is why D3 leaves unused files of existing titles alone.
- Address shapes: `/covers/<id>.jpg` (older rows, `update_cover_image_url`) and `/covers/<id>.jpg?v=<unix seconds>` (`cover_acceptance::cache_busted`, also built by hand with `format!` in `title_metadata_confirm.rs` — same shape). Nothing writes a remote URL into `titles.cover_image_url`; the wish list keeps its own (remote) `wishlist_items.cover_image_url` and writes nothing in the covers folder.
- `.staging/` holds `<32 hex>.jpg`; failed writes may leave `.tmp-<32 hex>` in the covers folder root: neither is an own cover name, both are only counted (D2).
- Clearing the address makes the title eligible for the Health tab's "Re-fetch missing covers" (`count_missing_covers`), which is D6 B's path; `MetadataRetryModel::enqueue_fresh` fits D6 A (its doc comment says "used only by the startup ISBN-10 conversion" — update it).
- A cleared address outdates a pending cover proposal (`cover_acceptance::cover_changed`), hence D7. Pending cover proposals by title: a plain read (no locking read, so the lock-scope rule does not apply).
- `admin_audit` takes a `u64` user id; the Admin's session id is the author.
- A backup lists the covers and reads them afterwards (`backup/mod.rs`): a file deleted in between fails the backup — the guard (D11) is needed.

Changed (no product decision needed):
- **AC1:** the own file name is the canonical `format!("{id}.jpg")`, id ≥ 1 — 19-6's `is_cover_name` accepts leading zeros and would let the tool delete `004217.jpg`, which the app never reads.
- **D2:** "cover address is not a local file" → "has another form" (an address naming another title's file is counted there too); list order added.
- **T2:** a file named by any title's address is never an orphan.
- **T4:** the cleared address leaves `version` **and** `updated_at` as they are (17-3 repair precedent).
- **AC4:** audit details carry `user_username` / `user_role` (#70 convention).
- **AC7:** the E2E orphan is made with existing flows (upload, delete, permanent delete) and retries once on D11.
- **Added:** T7 (spawned repair task), D12 (unmounted folder), D13 (purge leaves files).

Notes for the lead (no change made):
- D13 means orphans are the normal state today, not only after the 2026-11-09 incident: the first check on production will list every purged title that had a cover.
- A title without ISBN, ISSN or UPC whose file is missing ends with no cover; only a hand upload brings one back (D6 note).
- Browsers may show a cached image for a cleared address until the page is reloaded; cosmetic.

## Review pass 1 (2026-11-26)

Two reviews (safety, acceptance). Items and outcomes:

**Waiting for the project lead (new copy needed, not implemented):**
- (a) A failed delete, or a repair stopped by a restore starting, is reported as "had changed since the check" (counted in `skipped`).
- (b) A ratio guard against a mass deletion of orphans (the app pointed at the wrong database).

**Safety review**
- L1 (rename race between the last file check and `remove_file`) — accepted, not fixed: the window is microseconds, and the app only renames onto `<id>.jpg` of an existing title, which the re-check just ruled out.
- L2 — fixed: an error inside the repair loop stops it, the `cover_repair` audit row is still written with the partial counts and an `error` field, then the error is returned; a failed `restore_address` is logged (`error`). Test `an_error_midway_still_writes_the_audit_row_with_the_counts`.
- L3 — fixed: `restore_address` dequeues the retry row when the clear had queued one. Test `a_put_back_address_takes_its_lookup_off_the_queue`.
- L4 — fixed: `owned_ids` re-checks orphans by chunks of `OWNED_CHUNK` (500) — one `id IN (…)` read and one read of the `%/covers/%` addresses matched in Rust — just before that chunk's deletions. Test `owned_ids_reads_rows_and_addresses_in_one_chunk`.
- L5 (the 1 MiB form body cap is reached above ~20 000 listed items) — accepted, not fixed: far above any library this tool serves; a check after a first repair lists the rest.
- L6 — fixed: `list_folder` skips a file whose `metadata` answers `NotFound` (deleted since `read_dir`) instead of failing the check (`cover_file`). Unit test `a_file_gone_since_the_listing_is_skipped_not_a_failure`.
- L7 — fixed: just before each clear, `has_pending_cover_proposal` re-reads the title's pending cover proposal (plain read, `FORCE INDEX (idx_proposal_items_target)`); one that appeared is left alone. Test `a_pending_cover_proposal_is_read_from_the_target_index`.
- L8 — fixed: `named_cover_ids` treats any `/covers/<id>.jpg` followed by the end, `?` or `#` as naming the file (positive shape), in the plan and in the SQL re-check; only `local_cover_id` decides what may be cleared. Unit tests `an_address_names_every_own_file_it_holds`, `a_file_named_by_an_absolute_address_is_never_an_orphan`.

**Acceptance review**
- L1 (no browser-side refusal case in the E2E spec) — accepted: AC7 says a missing file cannot be made from the browser, and the D11 / D12 refusals are covered by the integration tests.
- L2 — fixed: route test of the unreadable folder (check, modal, repair: `an_unreadable_folder_is_reported_and_nothing_changes`), `restore_address` tests, a forged orphan item with a wrong mtime (`a_forged_orphan_with_another_mtime_is_left_alone`).
- L3 — fixed: `only_an_admin_checks_and_repairs` posts real items taken from an admin check and asserts the exact anonymous answer (303, `Location` = `HX-Redirect` = `/login?next=…`).
- L4 — fixed: `UserModel::actor_username` (the #70 audit name, `user-<id>` when gone) replaces the three `SELECT username FROM users WHERE id = ?` copies (`admin_covers.rs`, two in `routes/admin.rs`).
- L5 — fixed: `RESULT_ID` is used for the `HX-Retarget` header.
- L6 — fixed: `aria-live` is on a short `sr-only` status line (`#admin-covers-status`) only, refreshed out of band (`hx-swap-oob="innerHTML"`, so it keeps its attributes) with the match line, the list headings and the D12 sentence, the D2 refusal or the repair's line. Unit test `the_status_line_is_short_and_escaped`.

Gates: clippy `--all-targets -D warnings` clean; `cargo test --lib` 2028 passed; integration `cover_consistency::` (11), `admin_`, `locale_parity::`, the declaration check: 105 passed. E2E not run yet.

## Dev Agent Record

### Agent Model Used

Claude Opus 5.5 (dev-story, 2026-11-26).

### Debug Log References

- Logs under `/home/gcorbaz/devel/.wt-targets/19-9-logs/` (clippy, lib, integration).

### Completion Notes List

- Implemented as approved (D1–D13 A). Shapes: `own_cover_id` = exactly `format!("{id}.jpg")`, id ≥ 1; `local_cover_id` = `/covers/<id>.jpg[?v=<digits>]`. An entry with an own name that is not a regular file (link, directory) counts as present: its title is never "missing", the entry is never an orphan.
- Missing-cover precedence: pending cover proposal (left alone, D7) > Trash (D4) > set by hand (D5) > no code (D6 note) > cleared and queued.
- **Deviation from T6:** the modal is opened by `POST /admin/covers/repair/confirm` (not a GET): the check's "Repair…" form posts every listed item (hidden fields), which can be far more than a URL holds. The modal recomputes a fresh plan and carries only the listed items it still holds (`confirm`); when none is left or the folder now looks unmounted, the refreshed check replaces the result (`HX-Retarget #admin-covers-result`).
- T4 refinement: after an address is cleared the file is checked again; if it appeared meanwhile (a cover being installed: file first, address next), the address is put back (`WHERE cover_image_url IS NULL`). No row lock is taken (Rule #22 not engaged).
- The repair stops (remaining items counted as skipped) when `maintenance::write_permit()` is refused.
- Row lists have no column headers (no approved copy for them): `name · size · date` lines, titles linked with their note.
- D13: change request #744 opened (purge leaves cover files).
- Gates: `cargo clippy --all-targets -D warnings` clean; `cargo test --lib` 2024 passed; integration `cover_consistency::` 5/5, plus `locale_parity::`, `backup_*`, `admin_trash*`, `every_test_file_is_declared` green; no query macro added (`.sqlx/` unchanged); E2E `tsc --noEmit`, `waitForTimeout` grep, `vcode-literals.sh` clean. **E2E not run** (pending the lead's go).

### File List

- `src/services/cover_consistency.rs` (new), `src/services/mod.rs`
- `src/routes/admin_covers.rs` (new), `src/routes/mod.rs`, `src/routes/admin_backups.rs`
- `src/models/metadata_retry.rs` (doc comment of `enqueue_fresh`)
- `templates/fragments/admin_covers_section.html`, `admin_covers_check.html`, `admin_covers_repair_modal.html` (new), `admin_backups_panel.html`
- `locales/{en,fr,de,it}.yml`
- `tests/cover_consistency.rs` (new), `tests/integration.rs`
- `tests/e2e/specs/journeys/admin-cover-check.spec.ts` (new, spec id `KC`)
- `CLAUDE.md`

### Change Log

- 2026-11-26 — create-story: story 19-9 written, decisions D1–D11 for the project lead.
- 2026-11-26 — validate-story: checked against the code; D12, D13, T7 added, AC1, AC4, AC7, D2, T2, T4 corrected (see Validation).
- 2026-11-26 — dev-story: implemented (T1–T3, T5, tests AC1–AC6); E2E spec written, run pending. Status stays in-progress.
- 2026-11-26 — code review pass 1: Lows fixed (see Review pass 1); two copy items wait for the project lead.
