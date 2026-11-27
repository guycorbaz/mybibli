# mybibli v1.29.0 — fewer bugs, built-in backup, and daily-use fixes

The closing release of two epics, shipped together: epic 19, "fewer bugs, and built-in backup" ([#657](https://github.com/guycorbaz/mybibli/issues/657), [#658](https://github.com/guycorbaz/mybibli/issues/658), [#708](https://github.com/guycorbaz/mybibli/issues/708) and a batch of review findings), and epic 20, "daily-use fixes for the wish list, series and locations" ([#714](https://github.com/guycorbaz/mybibli/issues/714), [#715](https://github.com/guycorbaz/mybibli/issues/715), [#716](https://github.com/guycorbaz/mybibli/issues/716), [#524](https://github.com/guycorbaz/mybibli/issues/524), [#700](https://github.com/guycorbaz/mybibli/issues/700)). mybibli now backs itself up — the whole database and every cover in one file — and restores from the admin page.

**Upgrade impact: four additive migrations**, run automatically at the first start (contributor names unique among live rows, the two backup settings, a location description). **One new mount: the backups folder** — read *For operators* below before upgrading. One new optional environment variable, `MYBIBLI_BACKUP_DIR`. **Take a backup first** (the usual way: this release is the first that can do it for you).

## What's new

**Built-in backups.** *Admin → Backups → Back up now* writes `mybibli-<instance id>-<UTC stamp>.tar.gz` into the backups folder: the whole database (Trash included, no session token) and every cover, with a manifest and a `.sha256` beside it. The catalogue keeps working during the backup (consistent snapshot, nothing locked). *Admin → System → Backups* schedules them: *Automatic backup* every hour by default (off, 1, 2, 3, 4, 6, 8, 12 or 24 hours), written only when something changed. The rotation keeps every backup of the last 24 hours, the newest one, and the last backup of each of the most recent days on which a backup was written (*Daily backups to keep*, 1 to 365, default 14). A backup of another instance is never deleted. The Health tab shows the last backup, the next check, and any folder warning.

**Restore from the Backups tab.** The list *Backups in the folder* has *Restore…* on every row. mybibli reads and checks the whole file first (checksum, each member, version); the window states the facts and what follows. Then a **safety backup** of the current state is written, every other page shows a *Maintenance* page for a minute or two, the library is replaced, and everyone is logged out. A restore that fails puts the library back from the safety backup; restoring the safety backup undoes a restore. A backup from an older mybibli is migrated after loading; one from a newer mybibli is refused.

**Cover check and repair** ([#658](https://github.com/guycorbaz/mybibli/issues/658)). At the foot of the Backups tab, *Check covers* lists the cover files no title owns and the titles whose cover file is missing; *Repair…* deletes the first and queues a new download for the second. Each item is checked again just before it changes; every repair is audited. Two guards: no repair when more than half of the covers in use are missing (a covers folder that looks unmounted), nor when an orphan file carries a number this database never gave to a title (a database that looks like another library's).

**Production watching** ([#708](https://github.com/guycorbaz/mybibli/issues/708)). The Health tab counts slow database statements (over 1 s), failed connection waits, CSRF refusals and API-key refusals — last 24 hours and since start — with the client addresses refused most often; a count above its threshold is marked. In memory only, reset at restart.

**Fewer bugs.**
- Deleting a location from the tree goes through mybibli's confirmation window, in your language; the audit now also reads the markup built in Rust ([#698](https://github.com/guycorbaz/mybibli/issues/698)).
- Simultaneous adds: an add and a delete of its title serialise, two posts of one V-code give one volume, a V-code still held by a volume in the Trash is refused instead of reused (so that volume can still be restored), and a contributor name is created once ([#541](https://github.com/guycorbaz/mybibli/issues/541), [#663](https://github.com/guycorbaz/mybibli/issues/663)).
- What the user reads: window backdrops translucent again ([#712](https://github.com/guycorbaz/mybibli/issues/712)), an error above the visible part of a long page scrolled into view ([#689](https://github.com/guycorbaz/mybibli/issues/689)), focus kept after rejecting a proposal ([#538](https://github.com/guycorbaz/mybibli/issues/538)), the series list counting owned tomes and gaps like the dashboard ([#519](https://github.com/guycorbaz/mybibli/issues/519)).
- Trash search by name answers again ([#750](https://github.com/guycorbaz/mybibli/issues/750)).

**Daily use.**
- **Wish list: Remove** ([#714](https://github.com/guycorbaz/mybibli/issues/714)) — next to *Bought*, on the list, the mobile card and the entry's page; the audit log records which.
- **A missing tome to the wish list** ([#715](https://github.com/guycorbaz/mybibli/issues/715)) — each gap of a series grid has *Add to wish list*: the tome becomes an expected tome of the series and a wish list entry in one go.
- **The series total and expected tomes** ([#524](https://github.com/guycorbaz/mybibli/issues/524)) — the number of volumes of a closed series cannot go below a missing expected tome; a title restored from the Trash takes its positions back and says what became of an expected tome there.
- **A description for a location** ([#716](https://github.com/guycorbaz/mybibli/issues/716)) — up to 500 characters of plain text, shown on the location's page and returned by the API.
- **The location tree as a nested list** ([#700](https://github.com/guycorbaz/mybibli/issues/700)) — gone through with Tab, row by row; a fold button before each location that holds others, remembered by the browser.

## For operators

- **Mount the backups folder.** mybibli writes into `/app/backups` and **never creates that folder**. The `docker-compose.yml` of this release has, under the `volumes:` of the `mybibli` service:
  ```yaml
      - ./backups:/app/backups
  ```
  On an existing install, fetch the new compose file or add the line, create the host folder and keep it private — each backup holds the users' password hashes and the provider keys:
  ```bash
  mkdir -p backups && chmod 700 backups
  docker compose pull && docker compose up -d
  ```
  Without the mount, *Back up now* is refused and says why, and the Health tab shows a warning. Give each mybibli instance its own folder (a test instance must never write into production's). Copy the folder off the NAS: a backup on the same disk does not survive the disk. See the manual, chapter 1, step 1d "The backups folder", and chapter 7.
- **The first automatic backup comes 10 minutes after start** (then every hour by default, only when something changed). Set *Admin → System → Backups* to taste.
- **The first cover check on an existing library** lists many files no title owns: the covers of every title purged from the Trash since the library began — the purge leaves them behind ([#744](https://github.com/guycorbaz/mybibli/issues/744), still open). That is expected; repairing removes them.
- **Try a restore on a copy first.** Before you need it, restore a backup on a test instance with its own database and its own backups folder (manual, chapter 7, "Loading a backup into a test database" and "One folder per instance"). Behind a reverse proxy that cuts requests after 60 s, the restore page may show *Server error (504)* while the restore goes on: wait two minutes and reload; better, raise the proxy's read timeout for mybibli to five minutes.
- **Contributor names:** live duplicates that existed before the migration are kept as they are (merging them is [#591](https://github.com/guycorbaz/mybibli/issues/591)); from now on a name exists once among live contributors.
- **Security:** `cookie` 0.18.3 and `url` 2.5.9 (RUSTSEC-2026-0594, RUSTSEC-2026-0592).
- **Builds:** one integration-test binary and a Docker BuildKit cache make heavy builds shorter and lighter ([#717](https://github.com/guycorbaz/mybibli/issues/717)).

## Documentation

Manual (EN + FR): chapter 7 now opens on the built-in backups — writing, schedule and rotation, restoring, a backup from another instance, moving to another NAS, restoring by hand, and the cover check; chapter 1 gains step 1d "The backups folder" and `MYBIBLI_BACKUP_DIR` in the environment table; chapter 2 documents production watching, the location description and the keyboard tree; chapter 3 the wish list *Remove*, a gap sent to the wish list and the series total rule; chapter 8 carries "What's new in 1.29.0". The manual PDFs (EN + FR) are attached to this release.

## Merged pull requests since v1.28.1

- [#711](https://github.com/guycorbaz/mybibli/pull/711) Story 19-1: location tree delete through the modal; audit of Rust-built markup (closes #698)
- [#709](https://github.com/guycorbaz/mybibli/pull/709) docs(planning): epic 19 — fewer bugs, and built-in backup
- [#718](https://github.com/guycorbaz/mybibli/pull/718) Faster, lighter heavy builds: one integration-test binary, Docker build cache (#717)
- [#719](https://github.com/guycorbaz/mybibli/pull/719) Security: cookie 0.18.3 and url 2.5.9 (RUSTSEC-2026-0594, RUSTSEC-2026-0592)
- [#720](https://github.com/guycorbaz/mybibli/pull/720) Story 19-2: a stable E2E suite (closes #685, #499, #528, #674, #554)
- [#721](https://github.com/guycorbaz/mybibli/pull/721) Story 19-3: simultaneous actions (closes #541, #663 items 1–3)
- [#724](https://github.com/guycorbaz/mybibli/pull/724) Story 19-4: what the user reads (closes #519, #538, #712)
- [#726](https://github.com/guycorbaz/mybibli/pull/726) Story 19-5: production watching at a glance (closes #708)
- [#728](https://github.com/guycorbaz/mybibli/pull/728) Story 19-6: writing a backup
- [#730](https://github.com/guycorbaz/mybibli/pull/730) Story 19-7: scheduled backups and rotation
- [#731](https://github.com/guycorbaz/mybibli/pull/731) Epic 20: planning (#714, #715, #716, #524, #700)
- [#736](https://github.com/guycorbaz/mybibli/pull/736) Story 20-1: remove an entry from the wish list
- [#738](https://github.com/guycorbaz/mybibli/pull/738) Backups: rotate at the later of the check time and the new archive's stamp
- [#739](https://github.com/guycorbaz/mybibli/pull/739) Story 20-4: a description for a storage location
- [#741](https://github.com/guycorbaz/mybibli/pull/741) Story 19-8: restoring a backup
- [#742](https://github.com/guycorbaz/mybibli/pull/742) Story 20-2: send a missing series position to the wish list
- [#745](https://github.com/guycorbaz/mybibli/pull/745) Story 20-5: the location tree says what it does
- [#746](https://github.com/guycorbaz/mybibli/pull/746) Saved line names the retention; accessible names start with the visible label
- [#747](https://github.com/guycorbaz/mybibli/pull/747) Story 20-3: expected tomes stay consistent with the series total and Trash restores
- [#748](https://github.com/guycorbaz/mybibli/pull/748) Story 20-6: epic 20 smoke test
- [#751](https://github.com/guycorbaz/mybibli/pull/751) Trash: the name search answers again
- [#753](https://github.com/guycorbaz/mybibli/pull/753) Story 19-9: cover / database consistency
- [#754](https://github.com/guycorbaz/mybibli/pull/754) Story 19-10: epic 19 smoke test
- [#755](https://github.com/guycorbaz/mybibli/pull/755) Stories 19-11 and 20-7: documentation for epics 19 and 20

## Closed issues

- Change requests: [#658](https://github.com/guycorbaz/mybibli/issues/658) cover / database consistency tool, [#708](https://github.com/guycorbaz/mybibli/issues/708) light production watching, [#714](https://github.com/guycorbaz/mybibli/issues/714) wish list remove, [#715](https://github.com/guycorbaz/mybibli/issues/715) a series gap to the wish list, [#716](https://github.com/guycorbaz/mybibli/issues/716) location description, [#717](https://github.com/guycorbaz/mybibli/issues/717) faster, lighter heavy builds.
- Bugs: [#712](https://github.com/guycorbaz/mybibli/issues/712) solid black modal backdrops, [#750](https://github.com/guycorbaz/mybibli/issues/750) Trash search by name answered 500.
- Review findings: [#698](https://github.com/guycorbaz/mybibli/issues/698), [#689](https://github.com/guycorbaz/mybibli/issues/689), [#663](https://github.com/guycorbaz/mybibli/issues/663), [#541](https://github.com/guycorbaz/mybibli/issues/541), [#538](https://github.com/guycorbaz/mybibli/issues/538), [#528](https://github.com/guycorbaz/mybibli/issues/528), [#524](https://github.com/guycorbaz/mybibli/issues/524), [#519](https://github.com/guycorbaz/mybibli/issues/519), [#674](https://github.com/guycorbaz/mybibli/issues/674), [#700](https://github.com/guycorbaz/mybibli/issues/700).
- Known failures: [#685](https://github.com/guycorbaz/mybibli/issues/685), [#554](https://github.com/guycorbaz/mybibli/issues/554), [#499](https://github.com/guycorbaz/mybibli/issues/499).
- Built-in backup and restore ([#657](https://github.com/guycorbaz/mybibli/issues/657)) is delivered by this release; the issue closes with it.

**Full changelog:** https://github.com/guycorbaz/mybibli/compare/v1.28.1...v1.29.0
