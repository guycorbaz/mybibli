import { test, expect } from "@playwright/test";

test.describe("Media Type Scanning Journey", () => {
  test.beforeEach(async ({ page }) => {
    // Login as librarian
    await page.goto("/");
    await page.fill('[name="username"]', "admin");
    await page.fill('[name="password"]', "admin123");
    await page.click('button[type="submit"]');
    await page.waitForURL("**/catalog");
  });

  test("UPC scan shows MediaTypeSelector disambiguation", async ({ page }) => {
    const scanField = page.locator("[data-mybibli-scan-field]");
    await scanField.fill("0093624738626");
    await scanField.press("Enter");

    // Should see disambiguation buttons (not direct metadata fetch)
    const selector = page.locator('[role="group"]');
    await expect(selector).toBeVisible({ timeout: 5000 });
    await expect(selector).toContainText("CD");
    await expect(selector).toContainText("DVD");
    await expect(selector).toContainText("Book");
  });

  test("UPC scan → select CD → MusicBrainz metadata loads", async ({
    page,
  }) => {
    const scanField = page.locator("[data-mybibli-scan-field]");
    await scanField.fill("0093624738626");
    await scanField.press("Enter");

    // Wait for MediaTypeSelector
    const cdButton = page.locator('button[role="radio"]', { hasText: "CD" });
    await expect(cdButton).toBeVisible({ timeout: 5000 });
    await cdButton.click();

    // Should see skeleton feedback (metadata fetching)
    const feedback = page.locator(".feedback-skeleton");
    await expect(feedback).toBeVisible({ timeout: 5000 });
  });

  test("UPC scan → select DVD → OMDb metadata loads", async ({ page }) => {
    const scanField = page.locator("[data-mybibli-scan-field]");
    await scanField.fill("5051889004578");
    await scanField.press("Enter");

    // Wait for MediaTypeSelector
    const dvdButton = page.locator('button[role="radio"]', { hasText: "DVD" });
    await expect(dvdButton).toBeVisible({ timeout: 5000 });
    await dvdButton.click();

    // Should see skeleton feedback
    const feedback = page.locator(".feedback-skeleton");
    await expect(feedback).toBeVisible({ timeout: 5000 });
  });

  test("ISBN scan auto-detects Book, no disambiguation needed", async ({
    page,
  }) => {
    const scanField = page.locator("[data-mybibli-scan-field]");
    await scanField.fill("9782070360246");
    await scanField.press("Enter");

    // Should see skeleton feedback directly (no disambiguation)
    const feedback = page.locator("#feedback-list");
    await expect(feedback).toBeVisible({ timeout: 5000 });

    // Should NOT see disambiguation buttons
    const selector = page.locator('[role="group"]');
    await expect(selector).not.toBeVisible({ timeout: 2000 });
  });

  test("MediaTypeSelector buttons have accessible attributes", async ({
    page,
  }) => {
    const scanField = page.locator("[data-mybibli-scan-field]");
    await scanField.fill("0093624738626");
    await scanField.press("Enter");

    const group = page.locator('[role="group"]');
    await expect(group).toBeVisible({ timeout: 5000 });
    await expect(group).toHaveAttribute("aria-label");

    const buttons = page.locator('button[role="radio"]');
    const count = await buttons.count();
    expect(count).toBe(6); // Book, BD, CD, DVD, Magazine, Report
  });
});

test.describe("Media Type Scanning Smoke Test", () => {
  test("blank browser → login → scan UPC → select type → verify", async ({
    page,
  }) => {
    // Start from scratch (no injected cookies)
    await page.goto("/");

    // Login
    await page.fill('[name="username"]', "admin");
    await page.fill('[name="password"]', "admin123");
    await page.click('button[type="submit"]');
    await page.waitForURL("**/catalog");

    // Scan UPC
    const scanField = page.locator("[data-mybibli-scan-field]");
    await scanField.fill("0093624738626");
    await scanField.press("Enter");

    // Select CD type
    const cdButton = page.locator('button[role="radio"]', { hasText: "CD" });
    await expect(cdButton).toBeVisible({ timeout: 5000 });
    await cdButton.click();

    // Verify: should see some feedback (skeleton or resolved)
    const feedbackList = page.locator("#feedback-list");
    await expect(feedbackList).toBeVisible();
  });
});
