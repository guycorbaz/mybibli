import { expect, type Page } from "@playwright/test";

/**
 * Story 19-7 (#657, D9) — press "Back up now" on Admin → Backups and return
 * the text of the answer.
 *
 * Since story 19-7 the backup scheduler can hold the one-backup-at-a-time
 * guard for a few seconds (a scheduled check, a rotation), and the answer is
 * then "a backup is already being written" — a real user scenario, not a
 * flake. The click is retried once that guard is released (a few seconds
 * later, `toPass` intervals); any other answer is returned as it is.
 * The page must be on the Backups tab.
 */
const ALREADY_RUNNING =
  /already being written|déjà en cours d.écriture|bereits eine Sicherung geschrieben|già in corso la scrittura/i;

export async function backUpNow(page: Page): Promise<string> {
  const answer = page.locator("#admin-backups-feedback .feedback-entry");
  await expect(async () => {
    const response = page.waitForResponse(
      (r) => new URL(r.url()).pathname === "/admin/backups" && r.request().method() === "POST",
      { timeout: 60000 },
    );
    await page.locator("#admin-backups-run").click();
    const body = await (await response).text();
    expect(body, "the guard was free").not.toMatch(ALREADY_RUNNING);
  }).toPass({ intervals: [5000], timeout: 75000 });
  await expect(answer).toHaveCount(1, { timeout: 10000 });
  await expect(answer).not.toContainText(ALREADY_RUNNING);
  return (await answer.textContent()) ?? "";
}
