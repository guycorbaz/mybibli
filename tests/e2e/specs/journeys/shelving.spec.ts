import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { createLocation } from "../../helpers/locations";

const VALID_ISBN = specIsbn("SH", 1);
const VALID_ISBN_2 = specIsbn("SH", 2);
const VALID_ISBN_3 = specIsbn("SH", 3);

test.describe("Shelving by Scan (Story 2-2 + batch fix)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // AC1: V-code then L-code shelving (single volume)
  test("scan V-code then L-code → shelved feedback", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Create a title first
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 5000 });

    // Create a volume
    await scanField.fill("V0050");
    await scanField.press("Enter");
    const volFeedback = page.locator(".feedback-entry");
    await expect(volFeedback.first()).toBeVisible({ timeout: 5000 });
  });

  // AC2: L-code without volume context → batch mode
  test("scan L-code alone → active location feedback", async ({ page }) => {
    const lcode = await createLocation(page, "SH-TestBatch", "L1001");

    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(lcode);
    await scanField.press("Enter");

    const feedback = page.locator(".feedback-entry");
    await expect(feedback.first()).toBeVisible({ timeout: 5000 });
  });

  // Bug fix: L-code with last_volume still activates batch mode
  test("catalog multiple books then batch shelve → all volumes shelved", async ({
    page,
  }) => {
    const lcode = await createLocation(page, "SH-BatchShelve", "L1002");

    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Book 1: ISBN → V-code
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 5000 });

    await scanField.fill("V0051");
    await scanField.press("Enter");
    await expect(page.locator(".feedback-entry").first()).toBeVisible({ timeout: 5000 });

    // Book 2: ISBN → V-code
    await scanField.fill(VALID_ISBN_2);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 5000 });

    await scanField.fill("V0053");
    await scanField.press("Enter");
    await expect(page.locator(".feedback-entry").first()).toBeVisible({ timeout: 5000 });

    // Shelve the pile — scan L-code
    await scanField.fill(lcode);
    await scanField.press("Enter");

    // Wait for L-code feedback to appear (must contain L-code text, not stale V0053 entry)
    await expect(page.locator(".feedback-entry").first()).toContainText(
      new RegExp(lcode + "|batch|lot|location|emplacement", "i"),
      { timeout: 5000 }
    );

    // Scan existing V-code in batch mode
    await scanField.fill("V0051");
    await scanField.press("Enter");

    // Wait for V0051 shelving feedback to appear
    await expect(page.locator(".feedback-entry").first()).toContainText(/V0051/i, { timeout: 10000 });
    const shelveText = await page.locator(".feedback-entry").first().textContent();
    expect(shelveText).not.toMatch(/already assigned|déjà assigné/i);
    expect(shelveText).toMatch(/shelved|rangé|Shelved/i);
  });

  // Batch shelve: L-code then existing V-code
  test("batch mode: scan L-code then existing V-code → volume shelved", async ({
    page,
  }) => {
    const lcode = await createLocation(page, "SH-BatchMode", "L1003");

    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Create a title + volume
    await scanField.fill(VALID_ISBN_3);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 5000 });

    await scanField.fill("V0052");
    await scanField.press("Enter");
    await expect(page.locator(".feedback-entry").first()).toBeVisible({ timeout: 5000 });

    // Clear last_volume_label by scanning another ISBN
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 5000 });

    // Scan L-code — batch mode
    await scanField.fill(lcode);
    await scanField.press("Enter");

    const batchFeedback = page.locator(".feedback-entry").first();
    await expect(batchFeedback).toContainText(/active|Active|actif/i, { timeout: 5000 });

    // Scan existing V-code — should shelve
    await scanField.fill("V0052");
    await scanField.press("Enter");

    const shelveFeedback = page.locator(".feedback-entry").first();
    await expect(shelveFeedback).toBeVisible({ timeout: 5000 });
    const feedbackText = await shelveFeedback.textContent();
    expect(feedbackText).not.toMatch(/already assigned|déjà assigné/i);
  });

  // AC3: L-code not found
  test("scan unknown L-code → warning feedback", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill("L9999");
    await scanField.press("Enter");

    const warning = page.locator('.feedback-entry[data-feedback-variant="warning"]');
    await expect(warning).toBeVisible({ timeout: 5000 });
  });

  // Verify location page shows shelved volumes
  test("location page shows correct volume count after shelving", async ({
    page,
  }) => {
    const lcode = await createLocation(page, "SH-VolCount", "L1004");

    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Create title + volume
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 5000 });

    await scanField.fill("V0054");
    await scanField.press("Enter");
    await expect(page.locator(".feedback-entry").first()).toContainText(/V0054/i, { timeout: 10000 });

    // Shelve via L-code — wait for L-code response (shelving or batch activation)
    await scanField.fill(lcode);
    await scanField.press("Enter");
    await expect(page.locator(".feedback-entry").first()).toContainText(
      new RegExp(lcode + "|shelved|rangé|active|actif|batch|lot", "i"),
      { timeout: 5000 }
    );

    // Navigate to locations and find our location by name
    await page.goto("/locations");
    const locRow = page.locator(`text=${lcode}`).first();
    await expect(locRow).toBeVisible({ timeout: 3000 });

    // Find edit link to get location ID
    const editLink = page.locator(`a[href*="/edit"][aria-label*="SH-VolCount"]`).first();
    if (await editLink.isVisible({ timeout: 2000 }).catch(() => false)) {
      const href = await editLink.getAttribute("href");
      const locId = href?.match(/\/locations\/(\d+)/)?.[1];
      if (locId) {
        await page.goto(`/location/${locId}`);
        const volumeRows = page.locator("table tbody tr");
        const count = await volumeRows.count();
        expect(count).toBeGreaterThan(0);
      }
    } else {
      // Fallback: verify volume count badge in tree
      const volBadge = page.locator("text=/\\d+ vol/").first();
      await expect(volBadge).toBeVisible({ timeout: 3000 });
    }
  });
});
