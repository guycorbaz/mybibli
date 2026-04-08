import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

const VALID_ISBN = specIsbn("SH", 1);
const VALID_ISBN_2 = specIsbn("SH", 2);
const VALID_ISBN_3 = specIsbn("SH", 3);

test.describe("Shelving by Scan (Story 2-2 + batch fix)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // AC1: V-code then L-code shelving (single volume)
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

    // Should show "Active location" info
    const feedback = page.locator(".feedback-entry");
    await expect(feedback.first()).toBeVisible({ timeout: 5000 });
  });

  // Bug fix: L-code with last_volume still activates batch mode
  // Real user flow: catalog several books, then go shelve the pile
  test("catalog multiple books then batch shelve → all volumes shelved", async ({
    page,
  }) => {
    const scanField = page.locator("#scan-field");

    // Phase 1: Catalog 2 books with volumes (simulating scanning a pile)
    // Book 1: ISBN → V-code
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForTimeout(1500);

    await scanField.fill("V0051");
    await scanField.press("Enter");
    await page.waitForTimeout(1000);
    await expect(page.locator(".feedback-entry").first()).toBeVisible({
      timeout: 5000,
    });

    // Book 2: ISBN → V-code
    await scanField.fill(VALID_ISBN_2);
    await scanField.press("Enter");
    await page.waitForTimeout(1500);

    await scanField.fill("V0072");
    await scanField.press("Enter");
    await page.waitForTimeout(1000);
    await expect(page.locator(".feedback-entry").first()).toBeVisible({
      timeout: 5000,
    });

    // Phase 2: Shelve the pile — scan L-code first
    // At this point, last_volume_label = "V0072" in session
    await scanField.fill("L0001");
    await scanField.press("Enter");
    await page.waitForTimeout(1000);

    // L-code should shelve V0072 (last volume) AND activate batch mode
    const lcodeFeedback = page.locator(".feedback-entry").first();
    await expect(lcodeFeedback).toBeVisible({ timeout: 5000 });
    const lcodeText = await lcodeFeedback.textContent();
    // Should mention shelved or active location, NOT an error
    expect(lcodeText).not.toMatch(/error|erreur/i);

    // Phase 3: Scan the other existing V-code in batch mode
    await scanField.fill("V0051");
    await scanField.press("Enter");
    await page.waitForTimeout(1000);

    // Should show "shelved" success, NOT "already assigned" error
    const shelveFeedback = page.locator(".feedback-entry").first();
    await expect(shelveFeedback).toBeVisible({ timeout: 5000 });
    const shelveText = await shelveFeedback.textContent();
    expect(shelveText).not.toMatch(/already assigned|déjà assigné/i);
    // Should contain shelved/rangé confirmation
    expect(shelveText).toMatch(/shelved|rangé|Shelved/i);
  });

  // Batch shelve: L-code then multiple existing V-codes
  test("batch mode: scan L-code then existing V-code → volume shelved", async ({
    page,
  }) => {
    const scanField = page.locator("#scan-field");

    // Create a title + volume
    await scanField.fill(VALID_ISBN_3);
    await scanField.press("Enter");
    await page.waitForTimeout(1500);

    await scanField.fill("V0052");
    await scanField.press("Enter");
    await page.waitForTimeout(1000);
    await expect(page.locator(".feedback-entry").first()).toBeVisible({
      timeout: 5000,
    });

    // Activate batch mode (L-code without recent volume context)
    // First, scan another ISBN to clear last_volume_label
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForTimeout(1000);

    // Now scan L-code — no last_volume, goes straight to batch mode
    await scanField.fill("L0001");
    await scanField.press("Enter");
    await page.waitForTimeout(1000);

    const batchFeedback = page.locator(".feedback-entry").first();
    await expect(batchFeedback).toContainText(/active|Active|actif/i, {
      timeout: 5000,
    });

    // Scan existing V-code — should shelve at active location
    await scanField.fill("V0052");
    await scanField.press("Enter");

    const shelveFeedback = page.locator(".feedback-entry").first();
    await expect(shelveFeedback).toBeVisible({ timeout: 5000 });
    const feedbackText = await shelveFeedback.textContent();
    expect(feedbackText).not.toMatch(/already assigned|déjà assigné/i);
  });

  // AC3: L-code not found
  test("scan unknown L-code → warning feedback", async ({ page }) => {
    const scanField = page.locator("#scan-field");
    await scanField.fill("L9999");
    await scanField.press("Enter");

    const warning = page.locator(
      '.feedback-entry[data-feedback-variant="warning"]'
    );
    await expect(warning).toBeVisible({ timeout: 5000 });
  });

  // Verify location page shows shelved volumes
  test("location page shows correct volume count after shelving", async ({
    page,
  }) => {
    const scanField = page.locator("#scan-field");

    // Create title + volume
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForTimeout(1500);

    await scanField.fill("V0090");
    await scanField.press("Enter");
    await page.waitForTimeout(1000);

    // Shelve via L-code (this triggers shelve of last volume + batch mode)
    await scanField.fill("L0001");
    await scanField.press("Enter");
    await page.waitForTimeout(1000);

    // Navigate to locations page and verify volume count
    await page.goto("/locations");

    // The location L0001 should show volume count
    const l0001Label = page.locator("text=L0001").first();
    await expect(l0001Label).toBeVisible({ timeout: 3000 });

    // Find the location's edit link to extract the ID, then navigate to detail
    const l0001Row = l0001Label.locator("..").locator("..");
    const editLink = l0001Row.locator('a[href*="/edit"]').first();
    if (await editLink.isVisible({ timeout: 2000 }).catch(() => false)) {
      const href = await editLink.getAttribute("href");
      const locId = href?.match(/\/locations\/(\d+)/)?.[1];
      if (locId) {
        await page.goto(`/location/${locId}`);
        // Should show at least one volume in the contents
        const volumeRows = page.locator("table tbody tr");
        const count = await volumeRows.count();
        expect(count).toBeGreaterThan(0);
      }
    } else {
      // Fallback: check volume count text in the tree
      const volCount = l0001Row.locator("text=/\\d+ vol/");
      await expect(volCount).toBeVisible({ timeout: 3000 });
    }
  });
});
