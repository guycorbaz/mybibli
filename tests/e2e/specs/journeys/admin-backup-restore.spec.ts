import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import {
  backUpNow,
  EVERYONE_LOGGED_OUT,
  FILE_NAME,
  MODAL_TITLE,
  RESTORE_BUTTON,
  THIS_INSTANCE,
} from "../../helpers/backups";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { csrfHeaders } from "../../helpers/csrf";
import { expectHtmxProcessed } from "../../helpers/htmx";
import { expectReallySeen } from "../../helpers/visibility";

/**
 * Story 19-8 (#657, FR181, AC9) — restoring a backup from Admin → Backups.
 *
 * Journey from a blank browser: an admin catalogues a title, backs up,
 * deletes the title, opens the restore modal of the spec's own backup
 * (found by the file name of its own answer, never "the first row"), reads
 * the facts, confirms, reads the success line, follows "Log in again", logs
 * in and finds the title back.
 *
 * specId "RS". **This spec replaces the whole database and logs everyone
 * out**: it runs in its own Playwright project, chained after every other
 * one (`instance-restore`, after `instance-session-timeout`), alone. It
 * leaves the stack usable: the backup it restores was written moments
 * before by the spec itself, so the library afterwards is the one it found
 * (plus its own title, back from the Trash); the seeded users are kept
 * (`MYBIBLI_SEED_DEV_USERS=1` on the E2E stack). Story 19-10's epic smoke
 * restores too: it runs in its own project after this one
 * (`instance-epic19-smoke`). The refusals (damaged file, missing checksum…)
 * cannot be made from a browser: they are covered by route tests
 * (`tests/backup_restore.rs`).
 */

test.describe("Story 19-8 — restoring a backup", () => {
  test("an admin restores a backup of a moment ago and finds a deleted title back", async ({ page }, testInfo) => {
    // A backup, a safety backup and a restore of the whole E2E database.
    test.setTimeout(420000);
    await page.context().clearCookies();
    await loginAs(page, "admin");

    // 1. A title of this spec. One ISBN per attempt: a retry runs after an
    //    earlier attempt's restore may have brought its title back.
    const isbn = specIsbn("RS", 1 + testInfo.retry);
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(isbn);
    await scanField.press("Enter");
    const titleId = await titleIdFromSkeleton(page);

    // 2. Back up; the spec's own backup is the one its answer names.
    await page.goto("/admin?tab=backups", { waitUntil: "domcontentloaded" });
    const answer = await backUpNow(page);
    const file = answer.match(FILE_NAME)?.[0];
    expect(file, `a file name in: ${answer}`).toBeTruthy();

    // 3. Delete the title (to the Trash).
    await page.goto(`/title/${titleId}`, { waitUntil: "domcontentloaded" });
    const deleted = await page.request.delete(`/catalog/title/${titleId}`, {
      headers: { ...(await csrfHeaders(page)), "HX-Request": "true" },
    });
    expect(deleted.ok(), `delete answered ${deleted.status()}`).toBeTruthy();
    expect((await page.request.get(`/title/${titleId}`)).status()).toBe(404);

    // 4. The list shows the spec's backup; "Restore…" opens the modal (a
    //    second press while it is prepared is dropped: `hx-sync`).
    await page.goto("/admin?tab=backups", { waitUntil: "domcontentloaded" });
    const row = page.locator(`#admin-backups-list tr[data-backup-file="${file}"]`);
    await expect(row).toBeVisible();
    await expect(row).toContainText(THIS_INSTANCE);
    const open = row.getByRole("button", { name: RESTORE_BUTTON });
    await expectHtmxProcessed(open);
    await open.click();
    const dialog = page.locator("#modal-slot dialog[open]");
    await expect(dialog).toContainText(MODAL_TITLE);
    await expect(dialog).toContainText(THIS_INSTANCE);
    await expect(dialog).toContainText(/mybibli \d+\.\d+\.\d+/);

    // 5. Confirm: while it runs the "Restoring…" line is really seen and the
    //    modal cannot be closed (review pass 1); then the line replaces the
    //    modal and "Log in again" takes the focus. Review pass 2 (L-3): the
    //    restore's answer is held back by the test until the busy state has
    //    been checked, so those checks never race the end of the restore.
    let release!: () => void;
    const released = new Promise<void>((resolve) => {
      release = resolve;
    });
    await page.route(`**/admin/backups/restore/${file}`, async (route) => {
      if (route.request().method() !== "POST") return route.continue();
      const response = await route.fetch({ timeout: 300000 });
      await released;
      await route.fulfill({ response });
    });
    const confirm = dialog.locator("[data-modal-confirm]");
    await expectHtmxProcessed(confirm);
    await confirm.click();
    await expectReallySeen(page.locator("#admin-backups-restoring"));
    await expect(dialog.locator("[data-modal-cancel]")).toBeDisabled();
    await page.keyboard.press("Escape");
    await expect(page.locator("#admin-backups-restoring")).toBeAttached();
    await expect(dialog).toBeVisible();
    release();
    const done = page.locator("#admin-backups-restore-done-text");
    await expect(done).toContainText(file!, { timeout: 300000 });
    await expect(done).toContainText(EVERYONE_LOGGED_OUT);
    const again = page.locator("#admin-backups-login-again");
    await expect(again).toBeFocused();
    await expect(again).toHaveAttribute("href", "/login");
    // Rule #21 (review pass 1): Escape closes the line and gives the focus
    // back to the button that opened the modal, not to <body>.
    await page.keyboard.press("Escape");
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);
    await expect(open).toBeFocused();
    await page.goto("/login");
    await expect(page).toHaveURL(/\/login/);

    // 6. Everyone was logged out; logging in again, the title is back.
    await loginAs(page, "admin");
    const back = await page.goto(`/title/${titleId}`, { waitUntil: "domcontentloaded" });
    expect(back?.status()).toBe(200);
    await expect(page.locator("#title-id")).toContainText(`#${titleId}`);
    await expect(page.locator("body")).toContainText(isbn);
  });
});
