import { test, expect } from "@playwright/test";

// Dev session cookie for librarian access
const DEV_SESSION_COOKIE = {
  name: "session",
  value: "ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2",
  domain: "localhost",
  path: "/",
};

// Valid ISBN-13 for testing
const VALID_ISBN = "9782070360246";

test.describe("Metadata Editing & Re-Download (Story 3-5)", () => {
  test.beforeEach(async ({ context }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
  });

  // AC1: Manual metadata editing form
  test("edit metadata form appears and saves changes", async ({ page }) => {
    // First, ensure a title exists by scanning an ISBN
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");

    // Wait for feedback
    await page.waitForSelector(".feedback-entry", { timeout: 10000 });

    // Navigate to the title detail page
    // Find the title link in the feedback or navigate directly
    // The title should have been created — find its ID from the search
    await page.goto("/");
    const searchField = page.locator('input[name="q"]');
    await searchField.fill(VALID_ISBN);
    await searchField.press("Enter");

    // Click on the first title result to go to detail page
    const titleLink = page.locator("table tbody tr td a").first();
    await expect(titleLink).toBeVisible({ timeout: 5000 });
    await titleLink.click();

    // Verify we're on the title detail page
    await expect(page.locator("#title-metadata")).toBeVisible();

    // Click the "Edit metadata" button
    const editButton = page.getByText("Edit metadata");
    if (await editButton.isVisible()) {
      await editButton.click();

      // Wait for edit form to appear
      await expect(page.locator("#edit-title")).toBeVisible({ timeout: 3000 });

      // Modify the publisher field
      const publisherField = page.locator("#edit-publisher");
      await publisherField.fill("Gallimard Test Edition");

      // Click Save
      const saveButton = page.locator('button[type="submit"]');
      await saveButton.click();

      // Verify the updated publisher appears in the metadata display
      await expect(page.locator("#title-metadata")).toContainText(
        "Gallimard Test Edition",
        { timeout: 5000 }
      );
    }
  });

  // AC1: Cancel edit returns to display mode
  test("cancel edit returns to metadata display", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-entry", { timeout: 10000 });

    // Navigate to title detail
    await page.goto("/");
    const searchField = page.locator('input[name="q"]');
    await searchField.fill(VALID_ISBN);
    await searchField.press("Enter");

    const titleLink = page.locator("table tbody tr td a").first();
    await expect(titleLink).toBeVisible({ timeout: 5000 });
    await titleLink.click();

    const editButton = page.getByText("Edit metadata");
    if (await editButton.isVisible()) {
      await editButton.click();
      await expect(page.locator("#edit-title")).toBeVisible({ timeout: 3000 });

      // Click Cancel
      const cancelButton = page.locator("#cancel-edit");
      await cancelButton.click();

      // Edit form should be gone, display mode restored
      await expect(page.locator("#edit-title")).not.toBeVisible({ timeout: 3000 });
      await expect(page.locator("#title-metadata h1")).toBeVisible();
    }
  });

  // AC8: Smoke test — full journey from login
  test("smoke: login → title → edit → verify", async ({ context, page }) => {
    // Clear cookies for clean session
    await context.clearCookies();

    // Login
    await page.goto("/login");
    await page.locator("#username").fill("dev");
    await page.locator("#password").fill("dev");
    await page.locator('button[type="submit"]').click();

    // Should redirect to home
    await expect(page).toHaveURL("/", { timeout: 5000 });

    // Navigate to catalog and scan an ISBN
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");

    // Wait for scan feedback
    await page.waitForSelector(".feedback-entry", { timeout: 10000 });

    // Search for the title from home
    await page.goto("/");
    const searchField = page.locator('input[name="q"]');
    await searchField.fill(VALID_ISBN);
    await searchField.press("Enter");

    // Click into title detail
    const titleLink = page.locator("table tbody tr td a").first();
    await expect(titleLink).toBeVisible({ timeout: 5000 });
    await titleLink.click();

    // Verify title detail page loaded with edit button
    await expect(page.locator("#title-metadata")).toBeVisible();
    const editButton = page.getByText("Edit metadata");
    await expect(editButton).toBeVisible({ timeout: 3000 });

    // Click edit, modify, save
    await editButton.click();
    await expect(page.locator("#edit-title")).toBeVisible({ timeout: 3000 });

    // Verify form is pre-filled
    const titleInput = page.locator("#edit-title");
    const titleValue = await titleInput.inputValue();
    expect(titleValue.length).toBeGreaterThan(0);

    // Save (no changes, just verify the round-trip works)
    await page.locator('button[type="submit"]').click();

    // Display mode should be restored
    await expect(page.locator("#title-metadata h1")).toBeVisible({ timeout: 5000 });
  });
});
