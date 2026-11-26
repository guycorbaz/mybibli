import { expect, type Page } from "@playwright/test";

/**
 * Story 19-7 (#657, D9) — press "Back up now" on Admin → Backups and return
 * the text of the answer.
 *
 * Since story 19-7 the backup scheduler can hold the one-backup-at-a-time
 * guard for a few seconds (a scheduled check, a rotation), and the answer is
 * then "a backup is already being written" — a real user scenario, not a
 * flake. The press is then retried exactly once, 5 s later (the guard is
 * held for seconds); a second "already running" fails the step. Any other
 * answer is returned as it is. The page must be on the Backups tab.
 */
const ALREADY_RUNNING =
  /already being written|déjà en cours d.écriture|bereits eine Sicherung geschrieben|già in corso la scrittura/i;

/** One press; the body of the server's answer. */
async function press(page: Page): Promise<string> {
  const response = page.waitForResponse(
    (r) => new URL(r.url()).pathname === "/admin/backups" && r.request().method() === "POST",
    { timeout: 60000 },
  );
  await page.locator("#admin-backups-run").click();
  return (await response).text();
}

export async function backUpNow(page: Page): Promise<string> {
  let presses = 0;
  let body = "";
  // `toPass` only spaces the attempts (5 s); the counter bounds them to two
  // presses: a third attempt returns without pressing, and the check below
  // fails on a second "already running".
  await expect(async () => {
    if (presses === 2) return;
    presses += 1;
    body = await press(page);
    expect(body, "the guard was free").not.toMatch(ALREADY_RUNNING);
  }).toPass({ intervals: [5000], timeout: 150000 });
  expect(body, "still running after one retry").not.toMatch(ALREADY_RUNNING);
  const answer = page.locator("#admin-backups-feedback .feedback-entry");
  await expect(answer).toHaveCount(1, { timeout: 10000 });
  await expect(answer).not.toContainText(ALREADY_RUNNING);
  return (await answer.textContent()) ?? "";
}
