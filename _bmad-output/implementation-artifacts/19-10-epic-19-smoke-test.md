# Story 19.10: Epic 19 smoke test (Foundation Rule #7)

Status: done

No GitHub issue. Covers: Foundation Rule #7. Epic 19; ships in v1.29.0 (with Epic 20). Sprint-status key: `19-10-epic-19-smoke-test`.

## Story

As the project,
I want one end-to-end journey from a blank browser over what the epic built,
so that the epic is proven done.

## Context

The epic AC (`epics.md`, Story 19.10): `loginAs` → a title created by the spec → a backup written from the Backups tab → the title deleted → the backup restored through the modal → login again → the title back, its cover shown → the Health tab shows the backup and the watching panel; no `waitForTimeout`; green under `CI=true npx playwright test --workers=2`.

What the journey crosses, by story:
- **19-6** "Back up now" (`helpers/backups.ts::backUpNow`, one retry on "already running"), answer in `#admin-backups-feedback` naming the file.
- **19-7** the Health tab backups block (`#health-backup`, `#health-backup-last`).
- **19-8** the restore: the list `#admin-backups-list tr[data-backup-file="…"]`, "Restore…" → UX-DR8 modal in `#modal-slot`, Confirm, success line `#admin-backups-restore-done-text` naming the backup **and** the safety backup, "Log in again" `#admin-backups-login-again` (focused, `href="/login"`); everyone logged out; covers folder = the backup's covers (D11); Health line `#health-backup-last-restore` (D13); the "Last backup" line then shows the safety backup.
- **19-5** the watching panel `#health-watch` and its four rows.
- **19-9** the cover check (read only, Backups tab, `#admin-covers-check` → `#admin-covers-result`) — see D3.
- 19-1 (location delete modal), 19-2 (suite hygiene), 19-3 (simultaneous adds), 19-4 (feedback reveal) have their own specs and are not part of the epic AC chain; the smoke does not repeat them.

`admin-backup-restore.spec.ts` (19-8, specId `RS`) already walks backup → delete → restore → login → title back, with the busy state, Escape and focus details. The smoke differs on purpose: the title gets a **cover** (uploaded by hand, as `admin-cover-check.spec.ts` does) so "its cover shown" proves the restore put the covers folder back; the title is deleted through the **title page's modal** (the real user path; the 19-8 spec uses `page.request.delete`); the journey ends on the **Health tab** and the **cover check**.

## Acceptance Criteria

1. **AC1 — blank browser.** The test starts with `page.context().clearCookies()` and a real login through `loginAs(page, "admin")` (backups, restore, Health tab and cover check are Admin-only); no `DEV_SESSION_COOKIE`, no injected cookie.
2. **AC2 — a title with a cover.** The spec scans its own ISBN on `/catalog` (`simulateScan`, `titleIdFromSkeleton`), waits for the mock metadata (`h1` containing the ISBN, as 19-9's `scanTitle`), uploads a 1×1 PNG through "Upload cover" and its modal, and sees `img[src*="/covers/{id}.jpg"]` on `/title/{id}`.
3. **AC3 — a backup.** On `/admin?tab=backups`, `backUpNow(page)` answers a line naming a file `mybibli-<16 hex>-<YYYYMMDD>T<HHMMSS>Z.tar.gz`; that name is the spec's backup.
4. **AC4 — the title deleted.** On `/title/{id}`, `#title-delete-button` opens the modal; Confirm lands on `/`; `/title/{id}` then answers 404.
5. **AC5 — the restore through the modal.** On `/admin?tab=backups`, the row `tr[data-backup-file="{file}"]` (never "the first row") shows "This instance"; its "Restore…" button opens `#modal-slot dialog[open]` titled "Restore this backup?"; Confirm; `#admin-backups-restore-done-text` contains the file name and "Everyone has been logged out" (timeout 300 s); the safety backup's name is read from that line (the other file name in it). `#admin-backups-login-again` is focused and its `href` is `/login`; the spec **clicks** it and lands on `/login`.
6. **AC6 — login again, the title back with its cover.** `loginAs(page, "admin")`; `/title/{id}` answers 200, `#title-id` shows `#{id}`, the page contains the ISBN, and `img[src*="/covers/{id}.jpg"]` is visible with `naturalWidth > 0` (the file is served, not only referenced).
7. **AC7 — the Health tab.** `/admin?tab=health`: `#health-backup-last-restore` contains the spec's file and the safety backup's name; `#health-backup-last` contains a backup file name (the safety backup or a newer scheduled one — not asserted equal, the scheduler may write meanwhile); `#health-watch` is visible with its four rows `#watch-slow-statements`, `#watch-pool-timeouts`, `#watch-csrf-refusals`, `#watch-api-key-refusals` (no count asserted: instance-wide, Rule #23).
8. **AC8 — the cover check (D3).** `/admin?tab=backups`: "Check covers" (`#admin-covers-check`, response of `POST /admin/covers/check` is ok); `#admin-covers-result` is not empty (one of `#admin-covers-match`, `#admin-covers-orphans`, `#admin-covers-missing`, `#admin-covers-unmounted`, `#admin-covers-another-database`, `#admin-covers-left-alone` is present); it does not contain `{id}.jpg · ` (not an orphan) and `#admin-covers-missing a[href="/title/{id}"]` has count 0 (not missing). No repair is pressed.
9. **AC9 — where it runs (D1).** The spec runs in its own Playwright project `instance-epic19-smoke`, chained after `instance-restore` (last of all), and is in `INSTANCE_WIDE_SPECS` so `chromium` ignores it.
10. **AC10 — hygiene.** Assertions only on the spec's own rows (Rule #23); no `waitForTimeout`; i18n-aware matchers (EN | FR | DE | IT, the regexes of the 19-8 and 19-9 specs); `npx tsc --noEmit` green; green under `CI=true npx playwright test --workers=2`. If this test fails, the epic is not done. File `tests/e2e/specs/journeys/epic19-smoke.spec.ts`, specIsbn id `"E9"`.

## Decisions

- **D1 — its own project after `instance-restore`, not inside it.** The 19-8 comment says the smoke "joins this project"; taken literally (a second `testMatch` in `instance-restore`) the two files would run **at the same time** on two workers (`fullyParallel: true`, `--workers=2`): one restore logs the other spec out mid-journey, replaces its database, and the backup guard answers the second "already running". 19-2 already settled that `mode: "serial"` only serialises one file, hence one project per instance-wide file, chained. So: `EPIC19_SMOKE_SPEC = /journeys\/epic19-smoke\.spec\.ts/`, added to `INSTANCE_WIDE_SPECS`, project `instance-epic19-smoke` with `dependencies: ["instance-restore"]`. Last of all because it too replaces the database and logs everyone out; after the cover-check project because that one's repair would otherwise run on a database the smoke has just replaced (and the smoke's check reads what a repair could change). Same cost as the other chained projects: a failure upstream skips it (the job is red anyway); alone with `npx playwright test --no-deps specs/journeys/epic19-smoke.spec.ts`. The "joins this project" comments (config and the 19-8 spec header) are rewritten to name the new project.
- **D2 — spec id `"E9"`.** Left for 19-10 by 20-6 (D2 there); used by no spec (grep of `specIsbn("E9"` under `tests/e2e/`); its prefix `9786957…` is shared by no other id ('E' is the only printable ASCII ≡ 69 mod 100, '9' the only one ≡ 57). No V-code is created: no `VCODE_BLOCKS` entry.
- **D3 — the cover check is run, read only. Settled 2026-11-26: A (recommended) — AC8 kept, "Repair…" never pressed.** The epic AC stops at the Health tab, and 19-9's couplings say "19-10 does not use this tool" (written to keep the stories independent). Running the **check** (never the repair) costs one click, changes nothing, and is the only place the journey can prove that the restore left the spec's cover consistent with the database (D11 of 19-8 vs D2 of 19-9). **A (recommended):** run it as AC8. **B:** leave it out (AC8 removed; 19-9's own spec covers the tool). Open question for the project lead — see below.
- **D4 — ISBN per attempt and run.** `RUN = Math.floor(Date.now() / 1000) % 20000`; `specIsbn("E9", ((RUN * 3 + testInfo.retry) % 9000) * 10 + 1)`: a retry runs after an earlier attempt's restore may have brought its title back (19-8's reason for one ISBN per attempt), and a re-run on a stack that was not reset must not re-scan a live ISBN (`titleIdFromSkeleton` needs a fresh skeleton).
- **D5 — the cover is uploaded, not fetched.** The mock metadata server's synthetic answer is not relied on for a cover; the 1×1 PNG upload of `admin-cover-check.spec.ts` gives a deterministic `<id>.jpg` (re-encoded by the server). DRY (Rule #1): `scanTitle`, `uploadCover` and `deleteTitle` of `admin-cover-check.spec.ts` (19-9) move to `tests/e2e/helpers/catalog.ts` (`scanTitleWithMetadata`, `uploadCoverByHand`, `deleteTitleFromPage`) and both specs import them; 19-9's spec keeps its behaviour. The PNG constant goes with `uploadCoverByHand`.
- **D6 — no hold-back of the restore answer.** The 19-8 spec intercepts the restore POST to check the busy state; the smoke does not (that detail is 19-8's), it waits for the success line with a 300 s timeout. `test.setTimeout(480000)` (19-8 used 420 s; the smoke adds a metadata wait, an upload and a check).
- **D7 — no visible change.** The story adds one spec, moves three test helpers and changes the Playwright config and one `CLAUDE.md` sentence; no template, Rust, locale or JS file changes.

## Dependency

**19-9 must be merged first** (or the spec developed on `story/19-9-covers-consistency` under the Rule #18 local-work exception and rebased with `--onto main` after its merge). AC8, D5's helper move and D1's chaining need 19-9's files: `templates/fragments/admin_covers_section.html` / `admin_covers_check.html`, `src/routes/admin_covers.rs`, `tests/e2e/specs/journeys/admin-cover-check.spec.ts` and the `instance-cover-check` project — `main` has none of them. Nothing is pushed before 19-9 is green and merged.

## Tasks / Subtasks

- [x] **T1 — helpers (D5).** `tests/e2e/helpers/catalog.ts`: `scanTitleWithMetadata(page, isbn): Promise<string>`, `uploadCoverByHand(page, id): Promise<void>`, `deleteTitleFromPage(page, id): Promise<void>` (bodies of 19-9's `scanTitle` / `uploadCover` / `deleteTitle`, its `UPLOAD_BTN` regex and `PNG_1X1`). `admin-cover-check.spec.ts` imports them and drops its local copies.
- [x] **T2 — `tests/e2e/playwright.config.ts` (D1, AC9).** `EPIC19_SMOKE_SPEC`, added to `INSTANCE_WIDE_SPECS`; project `instance-epic19-smoke` (`use: { browserName: "chromium" }`, `testMatch: EPIC19_SMOKE_SPEC`, `dependencies: ["instance-restore"]`) with a comment saying why (D1). The `instance-restore` comment and the header of `admin-backup-restore.spec.ts` name the new project instead of "joins this project".
- [x] **T3 — `tests/e2e/specs/journeys/epic19-smoke.spec.ts` (AC1–AC8, AC10).** Header comment: story, journey, data, project (as the other smokes). One `test.describe("Epic 19 smoke — backup, restore, watching")`, one test, `test.setTimeout(480000)`:
  1. `page.context().clearCookies()`; `loginAs(page, "admin")`.
  2. `id = await scanTitleWithMetadata(page, isbn)`; `uploadCoverByHand(page, id)`.
  3. `/admin?tab=backups` (`waitUntil: "domcontentloaded"`); `answer = await backUpNow(page)`; `file = answer.match(FILE_NAME)[0]`.
  4. `deleteTitleFromPage(page, id)`; `page.request.get("/title/{id}")` status 404.
  5. `/admin?tab=backups`; `row = #admin-backups-list tr[data-backup-file="{file}"]` visible, contains `THIS_INSTANCE`; `open = row.getByRole("button", { name: RESTORE_BUTTON })`, `expectHtmxProcessed(open)`, click; `dialog = #modal-slot dialog[open]` contains `MODAL_TITLE`; `confirm = dialog.locator("[data-modal-confirm]")`, `expectHtmxProcessed(confirm)`, click.
  6. `done = #admin-backups-restore-done-text` contains `file` (timeout 300000) and `EVERYONE_LOGGED_OUT`; `safety` = the match of `FILE_NAME` (global) in its text that is not `file` (expected present); `#admin-backups-login-again` focused, `href="/login"`; click it; `toHaveURL(/\/login/)`.
  7. `loginAs(page, "admin")`; `page.goto("/title/{id}")` status 200; `#title-id` contains `#{id}`; body contains `isbn`; `img[src*="/covers/{id}.jpg"]` visible; `expect.poll(naturalWidth) > 0`.
  8. `/admin?tab=health`: `#health-backup-last-restore` contains `file` and `safety`; `#health-backup-last` matches `FILE_NAME`; `#health-watch` visible; the four `#watch-…` rows attached.
  9. `/admin?tab=backups`: wait for the `POST /admin/covers/check` response while clicking `#admin-covers-check` (ok); `#admin-covers-result` children check (AC8); `not.toContainText("{id}.jpg · ")`; `#admin-covers-missing a[href="/title/{id}"]` count 0.
  Regexes `FILE_NAME`, `RESTORE_BUTTON`, `MODAL_TITLE`, `THIS_INSTANCE`, `EVERYONE_LOGGED_OUT` copied from `admin-backup-restore.spec.ts` — or, better, exported from `helpers/backups.ts` and imported by both specs (Rule #1; the 19-8 spec then imports them).
- [x] **T4 — `CLAUDE.md`.** In the 19-8 paragraph, "E2E: `admin-backup-restore.spec.ts` runs in its own project `instance-restore`, after every other test." gains: "the epic smoke `epic19-smoke.spec.ts` in `instance-epic19-smoke`, after it."
- [x] **T5 — gates.** `cd tests/e2e && npx tsc --noEmit`; the `waitForTimeout` grep gate; `tests/e2e/vcode-literals.sh` (no V-code literal); the smoke alone with `--no-deps` against a fresh stack (`./scripts/e2e-reset.sh` under `flock -w 10800 /tmp/station-heavy.lock mem-guard`), then `admin-cover-check.spec.ts` and `admin-backup-restore.spec.ts` alone with `--no-deps` (helper move, header edit); then the full suite once, `CI=true npx playwright test --workers=2` under the same lock + `mem-guard` (checks the chain: `instance-cover-check` → `instance-restore` → `instance-epic19-smoke`). No Rust change: no `cargo` gate.

## Dev Notes

### Files

| File | Change |
|---|---|
| `tests/e2e/specs/journeys/epic19-smoke.spec.ts` | new |
| `tests/e2e/helpers/catalog.ts` | three helpers moved from `admin-cover-check.spec.ts` (D5) |
| `tests/e2e/helpers/backups.ts` | the restore regexes exported (T3, optional DRY) |
| `tests/e2e/specs/journeys/admin-cover-check.spec.ts` | uses the helpers |
| `tests/e2e/specs/journeys/admin-backup-restore.spec.ts` | header names the new project; regexes imported if T3's DRY is taken |
| `tests/e2e/playwright.config.ts` | project `instance-epic19-smoke` |
| `CLAUDE.md` | one sentence (T4) |
| this file, `sprint-status.yaml` | status line + `last_updated` |

### Selectors and copy the spec relies on

| Selector / copy | Where it is rendered |
|---|---|
| `#scan-field` | `templates/components/scan_field.html` |
| `[id^='feedback-entry-']` (title id) | `src/routes/catalog.rs::skeleton_feedback_html` via `helpers/catalog.ts::titleIdFromSkeleton` |
| "Upload cover" (`cover.upload_button`, label `label_upload_cover`, shown only while the title has no cover), `#modal-slot input[type="file"]` (`#cover-upload-file`), `[data-modal-confirm]` | `templates/pages/title_detail.html`, `templates/fragments/cover_upload_modal.html` |
| `img[src*="/covers/{id}.jpg"]` | `templates/components/cover.html` (macro called by `title_detail.html`) |
| `#title-delete-button` (only with no volume) | `templates/pages/title_detail.html` |
| `#title-id` | `templates/pages/title_detail.html` |
| `#admin-backups-run`, `#admin-backups-feedback`, `#admin-backups-list` | `templates/fragments/admin_backups_panel.html` |
| `tr[data-backup-file]`, "This instance" | `templates/fragments/admin_backups_list.html` |
| `#admin-backups-restore-done-text`, `#admin-backups-login-again` | `templates/fragments/admin_backups_restore_done.html` |
| `#modal-slot`, `[data-modal-confirm]`, delete answer `HX-Redirect: /` | `templates/layouts/base.html`, `templates/components/modal.html`, `src/routes/title_lifecycle.rs` |
| success copy "Backup %{file} restored (…). The safety backup of the previous state is %{safety}. Everyone has been logged out." / « … Tout le monde a été déconnecté. » | `locales/{en,fr}.yml` `admin.backups.restore.success` |
| `#health-backup`, `#health-backup-last`, `#health-backup-last-restore` | `templates/fragments/admin_health_backup.html`, ids from `src/routes/admin_health_backup.rs` |
| `#health-watch`, `#watch-slow-statements`, `#watch-pool-timeouts`, `#watch-csrf-refusals`, `#watch-api-key-refusals` | `templates/fragments/admin_health_watch.html`, ids from `src/routes/admin_health_watch.rs` |
| `#admin-covers-check`, `#admin-covers-result` | `templates/fragments/admin_covers_section.html` **(19-9 branch)** |
| `#admin-covers-match`, `-orphans`, `-missing`, `-unmounted`, `-another-database`; orphan row `{name} · {size} · {when}`; missing row `href="/title/{id}"` | `templates/fragments/admin_covers_check.html`, `src/routes/admin_covers.rs` **(19-9 branch)** |

### Guardrails

- Never press "Repair…": the repair acts on the whole instance (19-9's own project).
- Never assert a count of the Health tab or of the cover check (instance-wide, Rule #23); only the spec's file, its safety backup, its title id.
- Never look for "the first row" of the backups list: the spec's file name selects the row.
- The restore can meet the backup guard held by the scheduler (10-minute warm-up, interval 1 h on the E2E stack): accepted as in the 19-8 spec; `backUpNow` already retries once on "already running".
- No `waitForTimeout`; `expectHtmxProcessed` before clicking a control HTMX just swapped in (restore button, modal confirm).

### Open questions for the project lead

- ~~**D3**~~ — settled: A (the read-only check runs at the end; no repair).
- No visible change; nothing else to decide.

### References

- `_bmad-output/planning-artifacts/epics.md` — Epic 19, Story 19.10.
- `_bmad-output/implementation-artifacts/19-6-writing-a-backup.md`, `19-7-scheduled-backups.md`, `19-8-restoring-a-backup.md`, `19-5-production-watching.md`; 19-9 on `story/19-9-covers-consistency`.
- `tests/e2e/specs/journeys/admin-backup-restore.spec.ts`, `admin-cover-check.spec.ts` (19-9 branch), `admin-health-watch.spec.ts`, `epic20-smoke.spec.ts`; `tests/e2e/helpers/backups.ts`, `catalog.ts`, `auth.ts`, `htmx.ts`, `scanner.ts`, `isbn.ts`.

## Validation

Checked on 2026-11-26 against `main` (`db7597af`, 19-1…19-8 and epic 20 merged) and `story/19-9-covers-consistency` (in CI), by grep of templates, routes, locales and specs.

- **Selectors on `main`:** `#scan-field` (`components/scan_field.html`); skeleton `feedback-entry-{title_id}` (`catalog.rs::skeleton_feedback_html`); `#cover-upload-file` + `[data-modal-confirm]` (`fragments/cover_upload_modal.html`); `<img src="{{ cover_url }}">` (`components/cover.html`), the stored address `/covers/{id}.jpg` (`services/cover.rs`); `#title-delete-button` (`title_detail.html:154`, only when `volume_count == 0` — the smoke creates no volume), its modal posts to `/title/{id}` and answers `HX-Redirect: /` (`routes/title_lifecycle.rs`); `#title-id` with `<span data-title-id>#{id}</span>`; `#modal-slot` (`layouts/base.html`); `#admin-backups-run`, `#admin-backups-feedback`, `#admin-backups-list` (`admin_backups_panel.html`); `tr data-backup-file` (`admin_backups_list.html`); `#admin-backups-restore-done-text`, `#admin-backups-login-again` (`admin_backups_restore_done.html`); `#health-backup-last`, `#health-backup-last-restore` (`routes/admin_health_backup.rs`); `#health-watch` and the four `watch-…` row ids (`admin_health_watch.html`, `routes/admin_health_watch.rs:47-50`); both blocks included by `admin_health_panel.html`.
- **Selectors on the 19-9 branch only:** `#admin-covers-check`, `#admin-covers-result` (`admin_covers_section.html`, included by its `admin_backups_panel.html`); `#admin-covers-match`, `-orphans`, `-missing`, `-unmounted`, `-another-database`, `-left-alone` (`admin_covers_check.html`); orphan row `format!("{} · {} · {}", name, size, when)` and missing `href: format!("/title/{}", id)` (`routes/admin_covers.rs:111, 173`). Hence the Dependency section.
- **Copy:** `admin.backups.list.origin_own` "This instance"; `admin.backups.restore.title` "Restore this backup?"; `admin.backups.restore.success` names `%{file}` and `%{safety}` (so the safety name is readable from the line, AC5); `admin.health.backup.last_restore` names both too (AC7); `cover.upload_button` in all four locales. The regexes of `admin-backup-restore.spec.ts` and `admin-cover-check.spec.ts` match them.
- **Spec id:** `specIsbn("E9"` appears nowhere under `tests/e2e/`; 20-6 D2 reserved it for this story.
- **Config:** on the 19-9 branch the chain is `chromium` → `instance-settings` → `instance-session-timeout` → `instance-cover-check` → `instance-restore`; the new project depends on `instance-restore`. A per-project `workers: 1` inside `instance-restore` was considered and set aside: one file per chained project is the pattern 19-2 set and the other instance-wide specs follow (D1).
- **Corrections made at validation:** the upload button's locale key is `cover.upload_button` (not `title_detail.cover.…`), and it renders only while the title has no cover; `#admin-covers-left-alone` added to AC8's non-empty check (it can be the only line besides the match line); the modal / `HX-Redirect` row added to the selector table.
- **Visible change:** none (D7). The only open question is D3.

## Dev Agent Record

### Completion Notes (2026-11-26)

- T1: `scanTitleWithMetadata`, `uploadCoverByHand`, `deleteTitleFromPage` (+ the upload button regex and `PNG_1X1`) moved to `tests/e2e/helpers/catalog.ts`; `admin-cover-check.spec.ts` imports them, behaviour unchanged.
- T2: project `instance-epic19-smoke` after `instance-restore`; `EPIC19_SMOKE_SPEC` in `INSTANCE_WIDE_SPECS`; the "joins this project" comments (config, 19-8 spec header) now name the new project.
- T3: `epic19-smoke.spec.ts` (specIsbn id `E9`). T3's optional DRY taken: `FILE_NAME`, `RESTORE_BUTTON`, `MODAL_TITLE`, `THIS_INSTANCE`, `EVERYONE_LOGGED_OUT` exported from `helpers/backups.ts`, imported by the 19-8 spec and the smoke.
- Deviation (small): before the cover check, the Backups tab is loaded with the default `load` wait (not `domcontentloaded`) so HTMX has wired `#admin-covers-check`; `expectHtmxProcessed` is not used on it (it is page markup, not a swap).
- T4: CLAUDE.md sentence added.
- T5 gates: `npx tsc --noEmit` green; `waitForTimeout` grep clean; `tests/e2e/vcode-literals.sh` green. Image rebuilt `--no-cache` (#735), stack reset on 18080. Alone with `--no-deps` (CI=true): epic19-smoke, admin-cover-check, admin-backup-restore — 1/1 each. After a reset, full suite `CI=true npx playwright test --workers=2`: 523 passed, 2 skipped, 0 failed, 0 flaky (4.0 min); chain `instance-cover-check` → `instance-restore` → `instance-epic19-smoke` ran in that order. No Rust change, no cargo gate.

### File List

- `tests/e2e/specs/journeys/epic19-smoke.spec.ts` (new)
- `tests/e2e/helpers/catalog.ts`
- `tests/e2e/helpers/backups.ts`
- `tests/e2e/specs/journeys/admin-cover-check.spec.ts`
- `tests/e2e/specs/journeys/admin-backup-restore.spec.ts`
- `tests/e2e/playwright.config.ts`
- `CLAUDE.md`
- `_bmad-output/implementation-artifacts/19-10-epic-19-smoke-test.md`, `_bmad-output/implementation-artifacts/sprint-status.yaml`

### Change Log

- 2026-11-26 — dev-story: smoke spec, helpers moved, project chained; E2E green; Status → review.

## Review (2026-11-27)

Independent reviewer: no Medium+. Lows applied: the restored cover is also fetched with `page.request` (past the browser cache: 200 and an image type); the orphan check is anchored on the list item (`^<id>.jpg · `); a comment records the accepted "already running" window at the restore Confirm. Accepted: the ISBN sequence repeats after ~50 minutes on a stack that was not reset (1 in 3000). The spec alone passes after the changes; the full suite passed before them (523, 0 flaky).
