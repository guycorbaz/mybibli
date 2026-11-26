/**
 * Story 19-9 E2E (#658, AC7) — the cover check and its repair.
 *
 * Real journey from a blank browser (loginAs admin): the spec makes an
 * orphan cover file of its own with existing flows only — a title of its
 * own ISBN (specIsbn("KC", 1)), a cover uploaded on its page, the title
 * deleted, then permanently deleted from the Trash (D13: the purge leaves
 * the file, named after the title id). Admin → Backups → "Check covers"
 * lists that file; "Repair…" opens the UX-DR8 modal; "Repair" deletes it
 * (one retry when a backup or restore holds the guard, D11 — parallel specs
 * write backups); a new check no longer lists it.
 *
 * The repair acts on the whole instance, so this spec runs in its own
 * Playwright project after every other test (`instance-cover-check`,
 * playwright.config.ts); run it alone with `--no-deps`.
 *
 * Rule #23: assertions only on this spec's own file, never on counts. A
 * missing file cannot be made from the browser (the covers folder is inside
 * the container): tests/cover_consistency.rs covers it.
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import {
  deleteTitleFromPage,
  scanTitleWithMetadata,
  uploadCoverByHand,
} from "../../helpers/catalog";
import { expectHtmxProcessed } from "../../helpers/htmx";

const CHECK_BTN = /Check covers|Contrôler les couvertures|Cover prüfen|Controlla le copertine/i;
const REPAIR_OPEN = /^(Repair…|Réparer…|Reparieren…|Ripara…)$/;
const BUSY =
  /backup or a restore is already running|sauvegarde ou une restauration est déjà en cours|Sicherung oder eine Wiederherstellung|già in corso un backup o un ripristino/i;
const REPAIRED = /Covers repaired|Couvertures réparées|Cover repariert|Copertine riparate/;

/**
 * Permanently delete title `id` from the Trash (type-to-confirm modal).
 * The Trash lists the newest deletion first, and this spec runs alone after
 * every other test (`instance-cover-check`), so its title is on the first
 * page. Neither a direct `/admin/trash?…` navigation (it drops the filters)
 * nor the name search (#750: it answers 500) can narrow the list.
 */
async function purgeTitle(page: Page, id: string, isbn: string): Promise<void> {
  await page.goto("/admin?tab=trash");
  const button = page.locator(`button[hx-get^="/admin/trash/titles/${id}/permanent-delete"]`);
  await expect(button).toHaveCount(1, { timeout: 10000 });
  const row = page.locator("tbody tr").filter({ has: button });
  const name = ((await row.locator("td").first().textContent()) ?? "").trim();
  expect(name).toContain(isbn);
  await button.click();
  const modal = page.locator("#modal-slot dialog[open]");
  await expect(modal).toBeVisible({ timeout: 10000 });
  await modal.locator('input[type="text"]').fill(name);
  const confirm = modal.locator("[data-modal-confirm]");
  await expect(confirm).toBeEnabled();
  await confirm.click();
  await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0, { timeout: 10000 });
  await expect(page.locator(`button[hx-get^="/admin/trash/titles/${id}/permanent-delete"]`)).toHaveCount(0);
}

/** Press "Check covers" on the Backups tab; the result region. */
async function checkCovers(page: Page) {
  const response = page.waitForResponse(
    (r) => new URL(r.url()).pathname === "/admin/covers/check" && r.request().method() === "POST",
  );
  await page.getByRole("button", { name: CHECK_BTN }).click();
  expect((await response).ok()).toBe(true);
  return page.locator("#admin-covers-result");
}

test.describe("Story 19-9 — cover check and repair", () => {
  test("an orphan cover file is listed, repaired through the modal, and gone", async ({ page }) => {
    // A whole journey (scan, metadata, upload, delete, purge, check, repair
    // with one possible D11 retry): longer than the default 30 s.
    test.setTimeout(240000);
    await page.context().clearCookies();
    await loginAs(page, "admin");

    // A title whose cover stays: the repair must leave a used cover alone
    // (checked after it). The orphan below is a purged title's file, an id
    // the database handed out, so the wrong-database guard lets it through.
    const kept = await scanTitleWithMetadata(page, specIsbn("KC", 2));
    await uploadCoverByHand(page, kept);

    const isbn = specIsbn("KC", 1);
    const id = await scanTitleWithMetadata(page, isbn);
    await uploadCoverByHand(page, id);
    await deleteTitleFromPage(page, id);
    await purgeTitle(page, id, isbn);

    // The check lists the file the purge left behind (D13). Two tabs check:
    // the second repairs, the first keeps its stale list for Rule #21.
    const own = `${id}.jpg · `;
    await page.goto("/admin?tab=backups");
    let result = await checkCovers(page);
    await expect(result.locator("#admin-covers-orphans")).toContainText(own);
    const other = await page.context().newPage();
    await other.goto("/admin?tab=backups");
    const otherResult = await checkCovers(other);
    await expect(otherResult.locator("#admin-covers-orphans")).toContainText(own);

    // "Repair…" opens the UX-DR8 modal; "Repair" deletes. One retry when
    // a backup or a restore holds the guard (D11); a second refusal fails.
    const open = other.getByRole("button", { name: REPAIR_OPEN });
    await expectHtmxProcessed(open);
    await open.click();
    const modal = other.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 10000 });
    const confirm = modal.locator("[data-modal-confirm]");
    await expectHtmxProcessed(confirm);
    let presses = 0;
    let busy = true;
    await expect(async () => {
      if (presses === 2) return;
      presses += 1;
      const answer = other.waitForResponse(
        (r) => new URL(r.url()).pathname === "/admin/covers/repair" && r.request().method() === "POST",
        { timeout: 60000 },
      );
      await confirm.click();
      const body = await (await answer).text();
      busy = BUSY.test(body);
      expect(busy, "the guard was free").toBe(false);
    }).toPass({ intervals: [5000], timeout: 150000 });
    expect(busy, `a backup or a restore still held the guard after ${presses} presses`).toBe(false);
    await expect(other.locator("#modal-slot dialog[open]")).toHaveCount(0, { timeout: 10000 });
    await expect(other.locator("#admin-covers-result")).toContainText(REPAIRED);
    await expect(other.locator("#admin-covers-result")).not.toContainText(own);
    await other.close();

    // Rule #21 (review pass 2, L5): in the first tab, "Repair…" now has
    // nothing left to repair — the refreshed check comes back instead of
    // the modal, and the focus goes to "Check covers", never to <body>.
    const stale = page.getByRole("button", { name: REPAIR_OPEN });
    await expectHtmxProcessed(stale);
    const refreshed = page.waitForResponse(
      (r) => new URL(r.url()).pathname === "/admin/covers/repair/confirm" && r.request().method() === "POST",
    );
    await stale.click();
    expect((await refreshed).ok()).toBe(true);
    await expect(page.locator("#admin-covers-result")).not.toContainText(own);
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);
    await expect(page.locator("#admin-covers-check")).toBeFocused();

    // A new check no longer lists it; the kept cover is still in use and
    // really loads.
    result = await checkCovers(page);
    await expect(result).not.toContainText(REPAIRED);
    await expect(result).not.toContainText(own);
    await page.goto(`/title/${kept}`);
    const keptImg = page.locator(`img[src*="/covers/${kept}.jpg"]`).first();
    await expect(keptImg).toBeVisible();
    await expect
      .poll(() => keptImg.evaluate((el) => (el as HTMLImageElement).naturalWidth), {
        message: "the kept cover file is served",
      })
      .toBeGreaterThan(0);
  });
});
