/**
 * Story 20-3 (#524; D7, D8) — expected tomes stay consistent with the
 * series total and with Trash restores.
 *
 * - The series total lowered under an expected tome is refused next to the
 *   total field; the form's other refusals sit under their fields too.
 * - A title restored from the Trash onto a position an expected tome took
 *   meanwhile is restored, and a warning line links to the series, where the
 *   tome is listed under "Expected tomes to review".
 *
 * Data: specId "ST" (ISBNs from specIsbn("ST", …), sequence derived from the
 * run stamp and the attempt's slot so a retry or a re-run never scans an
 * ISBN it already catalogued); series names run-stamped; assertions only on
 * this spec's rows (Rule #23).
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { vcodeSlot } from "../../helpers/vcode";
import { simulateScan } from "../../helpers/scanner";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { expectHtmxProcessed } from "../../helpers/htmx";
import { checkAccessibility } from "../../helpers/accessibility";

const RUN = Math.floor(Date.now() / 1000) % 20000;

/** ISBN `n` (< 10, unique across this spec's tests) of this attempt. */
const isbnFor = (n: number) => specIsbn("ST", ((RUN * 4 + vcodeSlot()) % 9000) * 10 + n);

async function createSeries(page: Page, name: string, total: number | null): Promise<string> {
  await page.goto("/series/new");
  await page.locator("#series-name").fill(name);
  if (total !== null) {
    await page.locator("#series-type").selectOption("closed");
    await page.locator("#series-total").fill(String(total));
  }
  await page.locator("#series-form-submit").click();
  await page.waitForURL(/\/series\/\d+$/);
  return new URL(page.url()).pathname;
}

/** Name the gap at `position` on the series page as an expected tome. */
async function nameGap(page: Page, seriesPath: string, position: number, title: string) {
  await page.goto(seriesPath);
  await page.locator(`[data-name-gap="${position}"]`).click();
  await expect(page.locator("#expected-tome-position")).toHaveValue(String(position));
  await page.locator("#expected-tome-title").fill(title);
  await page.locator('main button[type="submit"]').last().click();
  await page.waitForURL((url) => url.pathname === seriesPath);
}

test.describe("Story 20-3 — expected tomes and the series total", () => {
  test("the total lowered under an expected tome is refused next to the field", async ({ page }) => {
    await loginAs(page, "librarian");
    const name = `ST-Total-${RUN}-${vcodeSlot()}`;
    const seriesPath = await createSeries(page, name, 6);
    await nameGap(page, seriesPath, 5, `Cinquième ${RUN}`);

    await page.goto(`${seriesPath}/edit`);
    await page.locator("#series-total").fill("4");
    await page.locator("#series-form-submit").click();

    await expect(page.locator("#series-total-error")).toContainText(
      /Total must be at least 5|Le total doit être d’au moins 5/,
    );
    const total = page.locator("#series-total");
    await expect(total).toHaveValue("4");
    await expect(total).toBeFocused();
    await expect(total).toHaveAttribute("aria-invalid", "true");
    await expect(total).toHaveAttribute("aria-describedby", "series-total-error");
    await checkAccessibility(page);

    // Nothing was saved: the stored total is still 6.
    await page.goto(`${seriesPath}/edit`);
    await expect(page.locator("#series-total")).toHaveValue("6");
  });

  test("the other refusals of the series form sit under their fields", async ({ page }) => {
    await loginAs(page, "librarian");
    const taken = `ST-Taken-${RUN}-${vcodeSlot()}`;
    await createSeries(page, taken, null);

    // A whitespace-only name passes the browser's `required`.
    await page.goto("/series/new");
    await page.locator("#series-name").fill("   ");
    await page.locator("#series-form-submit").click();
    await expect(page.locator("#series-name-error")).toContainText(
      /Series name is required|Le nom de la série est obligatoire/,
    );
    await expect(page.locator("#series-name")).toBeFocused();

    // A duplicate name.
    await page.locator("#series-name").fill(taken);
    await page.locator("#series-form-submit").click();
    await expect(page.locator("#series-name-error")).toContainText(taken);
    await expect(page.locator("#series-name")).toHaveValue(taken);

    // Closed with an empty total.
    const fresh = `ST-Fresh-${RUN}-${vcodeSlot()}`;
    await page.locator("#series-name").fill(fresh);
    await page.locator("#series-type").selectOption("closed");
    await page.locator("#series-total").fill("");
    await page.locator("#series-form-submit").click();
    await expect(page.locator("#series-total-error")).toBeVisible();
    await expect(page.locator("#series-total")).toBeFocused();
    await expect(page.locator("#series-name")).toHaveValue(fresh);

    // A total below the owned count: two titles assigned, total 1.
    const ownedName = `ST-Owned-${RUN}-${vcodeSlot()}`;
    const ownedPath = await createSeries(page, ownedName, 3);
    for (const [n, position] of [[1, 1], [2, 2]] as const) {
      await page.goto("/catalog");
      await simulateScan(page, "#scan-field", isbnFor(n));
      const id = await titleIdFromSkeleton(page);
      await page.goto(`/title/${id}`);
      await page.locator("#assign-series").selectOption({ label: ownedName });
      await page.locator("#assign-position").fill(String(position));
      await page.locator("#assign-series-submit").click();
      await page.waitForURL(/\/title\/\d+/);
    }
    await page.goto(`${ownedPath}/edit`);
    await page.locator("#series-total").fill("1");
    await page.locator("#series-form-submit").click();
    await expect(page.locator("#series-total-error")).toContainText(/\(1\).*\(2\)/);
    await expect(page.locator("#series-total")).toHaveValue("1");
  });

  test("a title restored onto an expected tome's position points to the series", async ({ page }) => {
    await loginAs(page, "admin");
    const name = `ST-Restore-${RUN}-${vcodeSlot()}`;
    const seriesPath = await createSeries(page, name, 4);
    const seriesId = seriesPath.split("/").pop()!;

    // A title of the series at position 3.
    await page.goto("/catalog");
    const isbn = isbnFor(5);
    await simulateScan(page, "#scan-field", isbn);
    const titleId = await titleIdFromSkeleton(page);
    await page.goto(`/title/${titleId}`);
    await page.locator("#assign-series").selectOption({ label: name });
    await page.locator("#assign-position").fill("3");
    await page.locator("#assign-series-submit").click();
    await page.waitForURL(/\/title\/\d+/);

    // Deleted from its page (no volume): the position is free again.
    await page.locator("#title-delete-button").click();
    const dialog = page.locator("#modal-slot dialog[open]");
    await expect(dialog).toBeVisible({ timeout: 10000 });
    await dialog.locator("[data-modal-confirm]").click();
    await page.waitForURL((url) => url.pathname === "/", { timeout: 10000 });

    // An expected tome is recorded there meanwhile (no ISBN).
    const tomeTitle = `Tome trois ${RUN}`;
    await nameGap(page, seriesPath, 3, tomeTitle);

    // Restored from the Trash, filtered to titles. The row is found by the
    // ISBN: the title is the ISBN until the metadata arrives, then
    // "Test Title <isbn>" (mock catch-all).
    await page.goto("/admin?tab=trash", { waitUntil: "domcontentloaded" });
    await page.locator("#filter-entity-type").selectOption("titles");
    const row = page.locator("tbody tr").filter({ hasText: isbn });
    const restore = row.getByRole("button", { name: /^(restore|restaurer)$/i });
    await expect(restore).toHaveAttribute("hx-post", /entity_type=titles/, { timeout: 10000 });
    await expectHtmxProcessed(page.locator("#admin-trash-panel"));
    await restore.click();

    const warning = page
      .locator('#feedback-list .feedback-entry[data-feedback-variant="warning"]')
      .filter({ hasText: name });
    await expect(warning).toContainText(/position 3/, { timeout: 10000 });
    await expect(warning).toContainText(/Expected tomes to review|Tomes attendus à revoir/);
    const link = warning.getByRole("link", { name: /Open the series|Ouvrir la série/ });
    await expect(link).toHaveAttribute("href", `/series/${seriesId}`);
    await link.click();
    await page.waitForURL((url) => url.pathname === seriesPath);

    await expect(page.locator("#expected-tomes-review")).toContainText(tomeTitle);
  });
});
