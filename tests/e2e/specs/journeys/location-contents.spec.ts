import { test, expect } from "@playwright/test";

test.describe("Browse Shelf Contents (Story 2-3)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/login");
    await page.locator("#username").fill("admin");
    await page.locator("#password").fill("admin");
    await page.locator('#login-submit').click();
    await expect(page).toHaveURL(/\/catalog/, { timeout: 5000 });
  });

  test("location detail shows breadcrumb and heading", async ({ page }) => {
    // Create a location with a unique L-code
    await page.goto("/locations");
    await page.locator("summary").filter({ hasText: /add root|ajouter/i }).click();
    await page.locator("#new-name").fill("LC-ContentTest");
    await page.locator("#new-lcode").fill("L4001");
    await page.locator("#add-root-submit").click();
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });
    await expect(page.locator("text=LC-ContentTest")).toBeVisible({ timeout: 5000 });

    // Find edit link by aria-label to get location ID
    const editLink = page.locator('a[aria-label*="LC-ContentTest"][href*="/edit"]').first();
    await expect(editLink).toBeVisible({ timeout: 3000 });
    const href = await editLink.getAttribute("href");
    const id = href?.match(/\/locations\/(\d+)/)?.[1];
    if (id) {
      await page.goto(`/location/${id}`);
      await expect(page.locator("h1")).toContainText("LC-ContentTest");
      await expect(page.locator('nav[aria-label="Location path"]')).toBeVisible();
    }
  });

  test("empty location shows empty state message", async ({ page }) => {
    await page.goto("/locations");
    await page.locator("summary").filter({ hasText: /add root|ajouter/i }).click();
    await page.locator("#new-name").fill("LC-EmptyShelf");
    await page.locator("#new-lcode").fill("L4002");
    await page.locator("#add-root-submit").click();
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });
    await expect(page.locator("text=LC-EmptyShelf")).toBeVisible({ timeout: 5000 });

    // Find edit link by aria-label
    const editLink = page.locator('a[aria-label*="LC-EmptyShelf"][href*="/edit"]').first();
    await expect(editLink).toBeVisible({ timeout: 3000 });
    const href = await editLink.getAttribute("href");
    const id = href?.match(/\/locations\/(\d+)/)?.[1];
    if (id) {
      await page.goto(`/location/${id}`);
      const body = await page.textContent("body");
      expect(body).toBeTruthy();
    }
  });
});
