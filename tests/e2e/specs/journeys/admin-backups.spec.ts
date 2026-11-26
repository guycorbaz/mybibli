import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

/**
 * Story 19-6 (#657, FR179) — writing a backup from Admin → Backups.
 *
 * Journey from a blank browser: an admin logs in, opens the Backups tab,
 * sees the folder ready (the E2E stack mounts a tmpfs on /app/backups),
 * clicks "Back up now" and reads the success line naming the file. The
 * Health tab shows the instance id the file name carries.
 *
 * specId "BK". The backups folder is shared by the workers: the spec
 * asserts only on the answer its own click receives (Rule #23). The
 * refusal for a missing folder is covered by a route test
 * (`tests/backup_write.rs`): this stack always has the folder.
 */

const BACKUPS_TAB = /^(Backups|Sauvegardes|Sicherungen|Backup)$/i;
const READY = /— (ready|prêt|bereit|pronta)/i;
const FILE_NAME = /mybibli-([0-9a-f]{16})-\d{8}T\d{6}Z\.tar\.gz/;

test.describe("Story 19-6 — Admin → Backups", () => {
  test("an admin backs up from a blank browser and sees the file written", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");
    await page.goto("/admin", { waitUntil: "domcontentloaded" });
    await page.getByRole("tab", { name: BACKUPS_TAB }).click();
    await expect(page).toHaveURL(/\/admin\?tab=backups/);

    const folder = page.locator("#admin-backups-folder");
    await expect(folder).toContainText("/app/backups");
    await expect(folder).toContainText(READY);

    await page.locator("#admin-backups-run").click();
    const answer = page.locator("#admin-backups-feedback .feedback-entry");
    await expect(answer).toHaveCount(1, { timeout: 30000 });
    await expect(answer).toContainText(FILE_NAME);
    const text = (await answer.textContent()) ?? "";
    const match = text.match(FILE_NAME);
    expect(match, `a file name in: ${text}`).not.toBeNull();
    const instanceId = match![1]!;

    // The Health tab shows the id that names the backup.
    await page.goto("/admin?tab=health", { waitUntil: "domcontentloaded" });
    await expect(page.locator("#health-instance-id")).toHaveText(instanceId);
  });
});
