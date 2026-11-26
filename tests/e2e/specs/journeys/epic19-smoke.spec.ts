/**
 * Story 19-10 — Epic 19 smoke test (Foundation Rule #7): backup, restore
 * and production watching, end to end, from a blank browser. If this test
 * fails, epic 19 is not done.
 *
 * Journey:
 *   blank context → loginAs(admin) → a title scanned on /catalog, its
 *   metadata in, a cover uploaded by hand → Admin → Backups, "Back up now"
 *   (19-6): the answer names the spec's backup → the title deleted from its
 *   page through the modal; its page answers 404 → the spec's backup row
 *   (found by its file name, never "the first row"), "Restore…", the modal,
 *   Confirm (19-8): the success line names the backup and the safety
 *   backup, everyone is logged out, "Log in again" has the focus and leads
 *   to /login → logged in again, the title is back with its cover served →
 *   the Health tab shows the last restore, the last backup (19-7) and the
 *   watching panel's four rows (19-5) → the cover check (19-9, read only,
 *   D3: "Repair…" is never pressed) lists the title's cover neither as an
 *   orphan nor as missing.
 *
 * Data: spec id "E9". ISBN `specIsbn("E9", …)` derived from the run stamp
 * and the attempt (D4): a retry runs after an earlier attempt's restore may
 * have brought its title back, and a re-run on a stack that was not reset
 * must not re-scan a live ISBN. No V-code is created. Assertions only on
 * this spec's title, backup and safety backup (Rule #23): no count of the
 * Health tab or of the cover check.
 *
 * **This spec replaces the whole database and logs everyone out**: it runs
 * in its own Playwright project, last of all (`instance-epic19-smoke`,
 * after `instance-restore`, playwright.config.ts). Run it alone with
 * `npx playwright test --no-deps specs/journeys/epic19-smoke.spec.ts`.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import {
  backUpNow,
  EVERYONE_LOGGED_OUT,
  FILE_NAME,
  MODAL_TITLE,
  RESTORE_BUTTON,
  THIS_INSTANCE,
} from "../../helpers/backups";
import {
  deleteTitleFromPage,
  scanTitleWithMetadata,
  uploadCoverByHand,
} from "../../helpers/catalog";
import { expectHtmxProcessed } from "../../helpers/htmx";

const RUN = Math.floor(Date.now() / 1000) % 20000;

const COVER_RESULT_LINES = [
  "#admin-covers-match",
  "#admin-covers-orphans",
  "#admin-covers-missing",
  "#admin-covers-unmounted",
  "#admin-covers-another-database",
  "#admin-covers-left-alone",
].join(", ");

test.describe("Epic 19 smoke — backup, restore, watching", () => {
  test("a backed-up title deleted, the backup restored, the title back with its cover", async ({ page }, testInfo) => {
    // Metadata wait, upload, backup, safety backup + restore of the whole
    // E2E database, a cover check (D6).
    test.setTimeout(480000);

    // 1. Blank browser, real login (AC1).
    await page.context().clearCookies();
    await loginAs(page, "admin");

    // 2. A title of this spec with a cover (AC2).
    const isbn = specIsbn("E9", ((RUN * 3 + testInfo.retry) % 9000) * 10 + 1);
    const id = await scanTitleWithMetadata(page, isbn);
    await uploadCoverByHand(page, id);

    // 3. Back up; the spec's backup is the one its answer names (AC3).
    await page.goto("/admin?tab=backups", { waitUntil: "domcontentloaded" });
    const answer = await backUpNow(page);
    const file = answer.match(FILE_NAME)?.[0];
    expect(file, `a file name in: ${answer}`).toBeTruthy();

    // 4. The title deleted from its page (AC4).
    await deleteTitleFromPage(page, id);
    expect((await page.request.get(`/title/${id}`)).status()).toBe(404);

    // 5. The restore of the spec's backup through the modal (AC5).
    await page.goto("/admin?tab=backups", { waitUntil: "domcontentloaded" });
    const row = page.locator(`#admin-backups-list tr[data-backup-file="${file}"]`);
    await expect(row).toBeVisible();
    await expect(row).toContainText(THIS_INSTANCE);
    const open = row.getByRole("button", { name: RESTORE_BUTTON });
    await expectHtmxProcessed(open);
    await open.click();
    const dialog = page.locator("#modal-slot dialog[open]");
    await expect(dialog).toContainText(MODAL_TITLE);
    const confirm = dialog.locator("[data-modal-confirm]");
    await expectHtmxProcessed(confirm);
    // Accepted window: a scheduled check falling due right here would hold
    // the backup guard and refuse ("already running"); just after this
    // spec's own backup the scheduler is not due, so no retry is made.
    await confirm.click();

    // 6. The success line names both backups; "Log in again" (AC5).
    const done = page.locator("#admin-backups-restore-done-text");
    await expect(done).toContainText(file!, { timeout: 300000 });
    await expect(done).toContainText(EVERYONE_LOGGED_OUT);
    const named = [...((await done.textContent()) ?? "").matchAll(new RegExp(FILE_NAME.source, "g"))].map(
      (m) => m[0],
    );
    const safety = named.find((name) => name !== file);
    expect(safety, `a safety backup named in: ${named.join(", ")}`).toBeTruthy();
    const again = page.locator("#admin-backups-login-again");
    await expect(again).toBeFocused();
    await expect(again).toHaveAttribute("href", "/login");
    await again.click();
    await expect(page).toHaveURL(/\/login/);

    // 7. Logged in again: the title is back, its cover served (AC6).
    await loginAs(page, "admin");
    const back = await page.goto(`/title/${id}`, { waitUntil: "domcontentloaded" });
    expect(back?.status()).toBe(200);
    await expect(page.locator("#title-id")).toContainText(`#${id}`);
    await expect(page.locator("body")).toContainText(isbn);
    const cover = page.locator(`img[src*="/covers/${id}.jpg"]`).first();
    await expect(cover).toBeVisible();
    await expect
      .poll(() => cover.evaluate((el) => (el as HTMLImageElement).naturalWidth), {
        message: "the restored cover file is served",
      })
      .toBeGreaterThan(0);
    // Past the browser cache: the restored file itself is served.
    const served = await page.request.get(`/covers/${id}.jpg`);
    expect(served.status()).toBe(200);
    expect(served.headers()["content-type"]).toMatch(/^image\//);

    // 8. The Health tab: the last restore, the last backup, the watching
    //    panel (AC7). The last backup is the safety backup or a newer
    //    scheduled one: only its shape is asserted.
    await page.goto("/admin?tab=health", { waitUntil: "domcontentloaded" });
    const lastRestore = page.locator("#health-backup-last-restore");
    await expect(lastRestore).toContainText(file!);
    await expect(lastRestore).toContainText(safety!);
    await expect(page.locator("#health-backup-last")).toContainText(FILE_NAME);
    await expect(page.locator("#health-watch")).toBeVisible();
    for (const rowId of [
      "#watch-slow-statements",
      "#watch-pool-timeouts",
      "#watch-csrf-refusals",
      "#watch-api-key-refusals",
    ]) {
      await expect(page.locator(rowId)).toBeAttached();
    }

    // 9. The cover check, read only (AC8, D3): the title's cover is
    //    neither an orphan nor missing. "Repair…" is never pressed.
    //    The page is loaded in full first: HTMX wires the button on load.
    await page.goto("/admin?tab=backups");
    const check = page.locator("#admin-covers-check");
    const checked = page.waitForResponse(
      (r) => new URL(r.url()).pathname === "/admin/covers/check" && r.request().method() === "POST",
    );
    await check.click();
    expect((await checked).ok()).toBe(true);
    const result = page.locator("#admin-covers-result");
    await expect(result.locator(COVER_RESULT_LINES).first()).toBeAttached();
    await expect(
      result.locator("#admin-covers-orphans li").filter({ hasText: new RegExp(`^${id}\\.jpg · `) }),
    ).toHaveCount(0);
    await expect(result.locator(`#admin-covers-missing a[href="/title/${id}"]`)).toHaveCount(0);
  });
});
