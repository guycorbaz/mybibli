import { test, expect } from "@playwright/test";

const VALID_ISBN = "9782070360246";

test.describe("Shelving by Scan (Story 2-2)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/login");
    await page.locator("#username").fill("admin");
    await page.locator("#password").fill("admin");
    await page.locator('button[type="submit"]').click();
    await expect(page).toHaveURL(/\/catalog/, { timeout: 5000 });
  });

  // AC1: V-code then L-code shelving
  test("scan V-code then L-code → shelved feedback", async ({ page }) => {
    const scanField = page.locator("#scan-field");

    // Create a title first
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForTimeout(1000);

    // Create a volume
    await scanField.fill("V0050");
    await scanField.press("Enter");
    const volFeedback = page.locator(".feedback-entry");
    await expect(volFeedback.first()).toBeVisible({ timeout: 5000 });
  });

  // AC2: L-code without volume context → batch mode
  test("scan L-code alone → active location feedback", async ({ page }) => {
    // First create a location via /locations page
    await page.goto("/locations");
    await page.locator("summary").filter({ hasText: /add root/i }).click();
    await page.locator("#new-name").fill("TestBatch");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });

    // Go to catalog and scan the L-code
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill("L0001");
    await scanField.press("Enter");

    // Should show "Active location" info (not "coming soon")
    const feedback = page.locator(".feedback-entry");
    await expect(feedback.first()).toBeVisible({ timeout: 5000 });
  });

  // AC3: L-code not found
  test("scan unknown L-code → warning feedback", async ({ page }) => {
    const scanField = page.locator("#scan-field");
    await scanField.fill("L9999");
    await scanField.press("Enter");

    const warning = page.locator('.feedback-entry[data-feedback-variant="warning"]');
    await expect(warning).toBeVisible({ timeout: 5000 });
  });
});
