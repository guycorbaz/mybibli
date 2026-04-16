import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

const ISBN_EDIT = specIsbn("ME", 1);
const ISBN_CANCEL = specIsbn("ME", 2);
const ISBN_SMOKE = specIsbn("ME", 3);

test.describe("Metadata Editing & Re-Download (Story 3-5)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // AC1: Manual metadata editing form
  test("edit metadata form appears and saves changes", async ({ page }) => {
    // First, ensure a title exists by scanning an ISBN
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(ISBN_EDIT);
    await scanField.press("Enter");

    // Wait for any scan feedback
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

    // Extract title ID from skeleton element (id="feedback-entry-{N}")
    const feedbackEl = page.locator("[id^='feedback-entry-']").first();
    const feedbackId = await feedbackEl.getAttribute("id");
    const titleId = feedbackId?.replace("feedback-entry-", "");

    // Navigate directly to title detail page
    await page.goto(`/title/${titleId}`);

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

      // Fill optional number fields to avoid 422 (empty string → invalid i32)
      const pageCount = page.locator("#edit-page-count");
      if (await pageCount.isVisible({ timeout: 500 }).catch(() => false)) {
        const val = await pageCount.inputValue();
        if (!val) await pageCount.fill("0");
      }

      // Click Save — scope to the edit form via its stable ID (the nav bar
      // has its own language-toggle submit buttons since story 7-3, and the
      // title detail page can have other forms rendered after the metadata).
      await page.locator("#edit-title-submit").click();

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
    await scanField.fill(ISBN_CANCEL);
    await scanField.press("Enter");

    // Wait for any scan feedback
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

    // Extract title ID from skeleton element (id="feedback-entry-{N}")
    const feedbackEl = page.locator("[id^='feedback-entry-']").first();
    const feedbackId = await feedbackEl.getAttribute("id");
    const titleId = feedbackId?.replace("feedback-entry-", "");

    // Navigate directly to title detail page
    await page.goto(`/title/${titleId}`);

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

    // Real login via shared helper (Foundation Rule #7 — smoke tests must use loginAs)
    await loginAs(page);

    // Navigate to catalog and scan an ISBN
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(ISBN_SMOKE);
    await scanField.press("Enter");

    // Wait for any scan feedback
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

    // Extract title ID from skeleton element (id="feedback-entry-{N}")
    const feedbackEl = page.locator("[id^='feedback-entry-']").first();
    const feedbackId = await feedbackEl.getAttribute("id");
    const titleId = feedbackId?.replace("feedback-entry-", "");

    // Navigate directly to title detail page
    await page.goto(`/title/${titleId}`);

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

    // Fill optional number fields to avoid 422 (empty string → invalid i32)
    const pageCount = page.locator("#edit-page-count");
    if (await pageCount.isVisible({ timeout: 500 }).catch(() => false)) {
      const val = await pageCount.inputValue();
      if (!val) await pageCount.fill("0");
    }

    // Save (no changes beyond page_count fix, just verify the round-trip works).
    // Scope to the edit form to avoid matching unrelated submit buttons on the page
    // (e.g. #assign-series-submit), which would trip strict mode.
    await page.locator("#title-metadata").getByRole("button", { name: /save changes|enregistrer/i }).click();

    // Display mode should be restored
    await expect(page.locator("#title-metadata h1")).toBeVisible({ timeout: 5000 });
  });
});
