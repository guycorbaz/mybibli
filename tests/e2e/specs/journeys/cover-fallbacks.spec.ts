import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

/**
 * Issue #427 — cover-resolution fallbacks: BnF Couvertures + Inventaire.io.
 *
 * Both spec ISBNs resolve their METADATA through the BnF SRU catch-all
 * (which never carries a cover URL); the cover then comes from the new
 * fallbacks, each mocked in mock-metadata-server/server.py:
 *   - specIsbn("BN", 1) = 9786678000016 → BnF Couvertures serves the image
 *     (any other EAN gets the real API's "no cover = HTTP 500 HTML" quirk).
 *   - specIsbn("IV", 1) = 9787386000015 → Inventaire.io by-uris answers with
 *     the redirect shape (isbn: → inv:) and an invp:P2 image hash.
 *
 * The assertion target is the title-detail page's cover <img> pointing at
 * the locally stored /covers/<id>.jpg — proof the whole pipeline ran
 * (probe → download → resize → DB update), not just the URL resolution.
 */
test.describe("Issue #427 — BnF + Inventaire cover fallbacks", () => {
  /** Scan an ISBN, then poll the title-detail page until the locally
   *  downloaded cover appears (metadata fetch is async). */
  async function scanAndExpectCover(page: Page, isbn: string) {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(isbn);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
      timeout: 10000,
    });

    // Resolve the title-detail URL via home search (CR #250 scoped
    // list-mode selector — the same shape title-detail-volumes uses).
    await page.goto(`/?q=${isbn}`);
    const titleLink = page
      .locator(
        '#browse-results table.browse-table tbody tr td a[href^="/title/"]',
      )
      .first();
    await expect(titleLink).toBeVisible({ timeout: 15000 });
    const titleHref = (await titleLink.getAttribute("href"))!;

    // Poll the title-detail page until the locally downloaded cover
    // appears — the metadata + cover chain is async, and the page needs
    // a reload to reflect it.
    await expect(async () => {
      await page.goto(titleHref);
      await expect(
        page.locator('img[src*="/covers/"]').first(),
      ).toBeVisible({ timeout: 2000 });
    }).toPass({ timeout: 60000, intervals: [3000] });
  }

  test("cover recovered via the BnF Couvertures fallback", async ({ page }) => {
    await loginAs(page, "admin");
    await scanAndExpectCover(page, specIsbn("BN", 1));
  });

  test("cover recovered via the Inventaire.io fallback", async ({ page }) => {
    await loginAs(page, "admin");
    await scanAndExpectCover(page, specIsbn("IV", 1));
  });
});
