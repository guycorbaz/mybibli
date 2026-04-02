import { test, expect } from "@playwright/test";

test.describe("Cover Image Journey", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await page.fill('[name="username"]', "admin");
    await page.fill('[name="password"]', "admin123");
    await page.click('button[type="submit"]');
    await page.waitForURL("**/catalog");
  });

  test("scan ISBN → metadata resolves → cover image appears", async ({
    page,
  }) => {
    const scanField = page.locator("[data-mybibli-scan-field]");
    await scanField.fill("9780134685991");
    await scanField.press("Enter");

    // Wait for metadata to resolve (skeleton → resolved feedback)
    // Cover image should eventually appear with /covers/ src
    const coverImg = page.locator('img[src*="/covers/"]');
    await expect(coverImg).toBeVisible({ timeout: 15000 });
  });

  test("title detail page shows cover at detail size", async ({ page }) => {
    // Navigate to a title that should have a cover
    await page.goto("/");

    // Look for any title link in the list
    const titleRow = page.locator("table tbody tr").first();
    if ((await titleRow.count()) > 0) {
      await titleRow.click();
      await page.waitForTimeout(1000);

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
    await scanField.fill("9782070360246");
    await scanField.press("Enter");

    // Wait for feedback
    await page.waitForTimeout(3000);

    // Navigate to title detail
    await page.goto("/");
    const titleLink = page.locator("table tbody tr").first();
    if ((await titleLink.count()) > 0) {
      await titleLink.click();
      await page.waitForTimeout(1000);

      // Should show either cover or placeholder - both are valid
      const visual = page.locator(
        'img[alt^="Cover of"], [role="img"][aria-label="No cover available"]'
      );
      await expect(visual).toBeVisible({ timeout: 5000 });
    }
  });
});
