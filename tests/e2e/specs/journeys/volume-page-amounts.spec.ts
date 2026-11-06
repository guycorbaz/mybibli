/**
 * Story 16-2 E2E (#630 item 7, FR167) — the volume page shows its amounts.
 *
 * Real journey from a blank browser: loginAs(librarian) → catalogue a title
 * and a volume by scan → the volume page reads the two amounts as not set →
 * the edit form sets them → the page shows both, the value with its date →
 * a blank browser (anonymous) sees the volume page without any amount.
 *
 * Spec ID "VA" for ISBNs; V-code band V8500–V8999 (unused by any other
 * spec); assertions only on rows this test creates (Rule #23).
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";
import { volumeIdByLabel } from "../../helpers/catalog";
import { checkAccessibility } from "../../helpers/accessibility";

test.describe("Story 16-2 — amounts on the volume page", () => {
  test("a librarian sees the amounts, an anonymous visitor does not", async ({ page, browser, baseURL }) => {
    test.setTimeout(60000);
    await page.context().clearCookies();
    await loginAs(page, "librarian");

    const isbn = specIsbn("VA", 1 + Math.floor(Math.random() * 89999));
    const vcode = `V${8500 + Math.floor(Math.random() * 500)}`;
    await scanTitleAndVolume(page, isbn, vcode);
    await page.goto(`/?q=${isbn}`);
    await page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first()
      .click();
    await expect(page).toHaveURL(/\/title\/\d+/);
    const titleId = /\/title\/(\d+)/.exec(page.url())![1]!;
    const volumeId = await volumeIdByLabel(page, titleId, vcode);

    await page.goto(`/volume/${volumeId}`);
    await expect(page.locator("#volume-purchase-price")).toHaveText("—");
    await expect(page.locator("#volume-current-value")).toHaveText("—");
    await expect(page.locator("#volume-current-value-date")).toHaveCount(0);

    await page.goto(`/volume/${volumeId}/edit`);
    await page.locator('input[name="purchase_price"]').fill("24.90");
    await page.locator('input[name="current_value"]').fill("18.50");
    await page.locator('form[action$="/update"] button[type="submit"]').click();
    await expect(page).toHaveURL(new RegExp(`/volume/${volumeId}$`));

    await expect(page.locator("#volume-purchase-price")).toHaveText(/^24[.,]90( [A-Z]{3})?$/);
    await expect(page.locator("#volume-current-value")).toHaveText(/^18[.,]50( [A-Z]{3})?$/);
    await expect(page.locator("#volume-current-value-date")).toContainText(/\d{4}-\d{2}-\d{2}/);
    await checkAccessibility(page);

    const anonymous = await browser.newContext({ baseURL: baseURL! });
    const anonymousPage = await anonymous.newPage();
    await anonymousPage.goto(`/volume/${volumeId}`);
    await expect(anonymousPage.locator("h1")).toContainText(vcode);
    await expect(anonymousPage.locator("#volume-purchase-price")).toHaveCount(0);
    await expect(anonymousPage.locator("#volume-current-value")).toHaveCount(0);
    await anonymous.close();
  });
});
