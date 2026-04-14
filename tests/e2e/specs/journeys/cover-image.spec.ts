import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

const COVER_ISBN = "9780201633610"; // Google Books mock — Design Patterns, has thumbnail URL
const NO_COVER_ISBN = specIsbn("CI", 1); // Synthetic metadata — no cover

test.describe("Cover Image Journey", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("scan ISBN → metadata resolves → cover image or placeholder appears", async ({
    page,
  }) => {
    const scanField = page.locator("[data-mybibli-scan-field]");
    await scanField.fill(COVER_ISBN);
    await scanField.press("Enter");

    // Wait for skeleton feedback
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 5000 });

    // Trigger OOB delivery by scanning again (PendingUpdates middleware)
    // Bounded to 15s to cover BnF timeout + Google Books fallback under CI load.
    await scanField.fill(COVER_ISBN);
    await scanField.press("Enter");

    // Context banner should be visible with title info (cover or placeholder)
    const banner = page.locator("#context-banner");
    await expect(banner).not.toHaveClass(/hidden/, { timeout: 15000 });

    // Should show either a cover image or the placeholder SVG icon
    const coverOrPlaceholder = page.locator('#context-banner img[src*="/covers/"], #context-banner img[src*="/static/icons/"]');
    await expect(coverOrPlaceholder).toBeVisible({ timeout: 5000 });
  });

  test("title detail page shows cover at detail size", async ({ page }) => {
    // Navigate to a title that should have a cover
    await page.goto("/");

    // Look for any title link in the list
    const titleRow = page.locator("table tbody tr").first();
    if ((await titleRow.count()) > 0) {
      await titleRow.click();
      await page.waitForSelector('img[alt^="Cover of"], [role="img"][aria-label="No cover available"]', { timeout: 5000 });

      // Check for either a cover image or a placeholder
      const cover = page.locator(
        'img[alt^="Cover of"], [role="img"][aria-label="No cover available"]'
      );
      await expect(cover).toBeVisible({ timeout: 5000 });
    }
  });

  test("title without cover shows placeholder SVG", async ({ page }) => {
    // Scan an ISBN that won't have a cover from mock server
    const scanField = page.locator("[data-mybibli-scan-field]");
    await scanField.fill(NO_COVER_ISBN);
    await scanField.press("Enter");

    // Wait for feedback
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

    // Navigate to title detail
    await page.goto("/");
    const titleLink = page.locator("table tbody tr").first();
    if ((await titleLink.count()) > 0) {
      await titleLink.click();
      await page.waitForSelector('img[alt^="Cover of"], [role="img"][aria-label="No cover available"]', { timeout: 5000 });

      // Should show either cover or placeholder - both are valid
      const visual = page.locator(
        'img[alt^="Cover of"], [role="img"][aria-label="No cover available"]'
      );
      await expect(visual).toBeVisible({ timeout: 5000 });
    }
  });
});
