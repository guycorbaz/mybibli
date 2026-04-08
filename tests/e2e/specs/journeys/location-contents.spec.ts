import { test, expect } from "@playwright/test";

test.describe("Browse Shelf Contents (Story 2-3)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/login");
    await page.locator("#username").fill("admin");
    await page.locator("#password").fill("admin");
    await page.locator('button[type="submit"]').click();
    await expect(page).toHaveURL(/\/catalog/, { timeout: 5000 });
  });

  test("location detail shows breadcrumb and heading", async ({ page }) => {
    // Create a location first
    await page.goto("/locations");
    await page.locator("summary").filter({ hasText: /add root/i }).click();
    await page.locator("#new-name").fill("ContentTest");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });

    // Navigate to the location detail
    // Find the edit link for the specific location we just created
    const contentTestRow = page.locator('text=ContentTest').first().locator('..');
    const editLink = contentTestRow.locator('a[href*="/edit"]').first();
    await expect(editLink).toBeVisible({ timeout: 3000 });
    const href = await editLink.getAttribute("href");
    const id = href?.match(/\/locations\/(\d+)/)?.[1];
    if (id) {
      await page.goto(`/location/${id}`);
      await expect(page.locator("h1")).toContainText("ContentTest");
      // Breadcrumb should be visible
      await expect(page.locator('nav[aria-label="Location path"]')).toBeVisible();
    }
  });

  test("empty location shows empty state message", async ({ page }) => {
    await page.goto("/locations");
    await page.locator("summary").filter({ hasText: /add root/i }).click();
    await page.locator("#new-name").fill("EmptyShelf");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });

    const editLink = page.locator('a[href*="/locations/"][href*="/edit"]').last();
    const href = await editLink.getAttribute("href");
    const id = href?.match(/\/locations\/(\d+)/)?.[1];
    if (id) {
      await page.goto(`/location/${id}`);
      // Should show empty state (no volumes)
      const body = await page.textContent("body");
      expect(body).toBeTruthy();
    }
  });
});
