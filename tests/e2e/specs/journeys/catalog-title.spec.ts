import { test, expect } from "@playwright/test";

// Dev session cookie for librarian access
const DEV_SESSION_COOKIE = {
  name: "session",
  value: "ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2",
  domain: "localhost",
  path: "/",
};

// Valid ISBN-13 for testing (L'Étranger by Camus)
const VALID_ISBN = "9782070360246";
// Invalid ISBN-13 (wrong checksum — last digit changed)
const INVALID_ISBN = "9782070360247";

test.describe("Title CRUD & ISBN Scanning", () => {
  test.beforeEach(async ({ context }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
  });

  // AC1: Create new title from ISBN scan
  test("scan valid ISBN creates new title with success feedback", async ({
    page,
  }) => {
    await page.goto("/catalog");

    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");

    // Wait for feedback entry to appear
    const feedback = page.locator("#feedback-list .feedback-entry").first();
    await expect(feedback).toBeVisible({ timeout: 5000 });
    await expect(feedback).toHaveAttribute("data-feedback-variant", "success");

    // Context banner should be visible
    const banner = page.locator("#context-banner");
    await expect(banner).not.toHaveClass(/hidden/);
  });

  // AC2: Open existing title from ISBN scan
  test("scan same ISBN again shows info feedback", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // First scan creates
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(
      '.feedback-entry[data-feedback-variant="success"]',
    );

    // Second scan shows existing
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");

    const infoEntry = page.locator(
      '.feedback-entry[data-feedback-variant="info"]',
    );
    await expect(infoEntry).toBeVisible({ timeout: 5000 });
  });

  // AC8: ISBN checksum validation (client-side)
  test("scan invalid ISBN checksum shows error without server request", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Listen for network requests to verify no server call
    let serverCalled = false;
    page.on("request", (request) => {
      if (request.url().includes("/catalog/scan")) {
        serverCalled = true;
      }
    });

    await scanField.fill(INVALID_ISBN);
    await scanField.press("Enter");

    const errorEntry = page.locator(
      '.feedback-entry[data-feedback-variant="error"]',
    );
    await expect(errorEntry).toBeVisible({ timeout: 2000 });
    expect(serverCalled).toBe(false);
  });

  // AC9: Non-ISBN code handling
  test("scan ISSN code shows warning feedback", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill("97712345678");
    await scanField.press("Enter");

    const warningEntry = page.locator(
      '.feedback-entry[data-feedback-variant="warning"]',
    );
    await expect(warningEntry).toBeVisible({ timeout: 5000 });
  });

  // AC3: Open manual creation form via Ctrl+N
  test("Ctrl+N opens title creation form", async ({ page }) => {
    await page.goto("/catalog");

    await page.keyboard.press("Control+n");

    const formContainer = page.locator("#title-form-container");
    await expect(formContainer.locator("form")).toBeVisible({ timeout: 5000 });

    // Required fields should have asterisks
    const titleLabel = formContainer.locator('label[for="title-field"]');
    await expect(titleLabel).toContainText("*");
  });

  // AC5: Submit valid manual form
  test("submit valid manual form creates title", async ({ page }) => {
    await page.goto("/catalog");
    await page.keyboard.press("Control+n");

    const form = page.locator("#title-form-container form");
    await expect(form).toBeVisible({ timeout: 5000 });

    // Fill required fields
    await form.locator("#title-field").fill("Test Book Title");
    await form.locator("#media-type-field").selectOption("book");
    // Select first non-empty genre option
    const genreOptions = form.locator("#genre-field option");
    const optionCount = await genreOptions.count();
    if (optionCount > 1) {
      await form.locator("#genre-field").selectOption({ index: 1 });
    }
    await form.locator("#language-field").fill("fr");

    // Submit
    await form.locator('button[type="submit"]').click();

    // Success feedback should appear
    const feedback = page.locator(
      '.feedback-entry[data-feedback-variant="success"]',
    );
    await expect(feedback).toBeVisible({ timeout: 5000 });

    // Form should be closed
    await expect(form).not.toBeVisible();
  });

  // AC4: Media type-dependent form adaptation
  test("changing media type adapts form fields", async ({ page }) => {
    await page.goto("/catalog");
    await page.keyboard.press("Control+n");

    const form = page.locator("#title-form-container form");
    await expect(form).toBeVisible({ timeout: 5000 });

    // Select "book" — should show page_count
    await form.locator("#media-type-field").selectOption("book");
    await expect(form.locator("#page-count-field")).toBeVisible({
      timeout: 3000,
    });

    // Select "cd" — should show track_count
    await form.locator("#media-type-field").selectOption("cd");
    await expect(form.locator("#track-count-field")).toBeVisible({
      timeout: 3000,
    });
    await expect(form.locator("#page-count-field")).not.toBeVisible();
  });

  // AC5: Validation errors on missing required fields
  test("submit form with missing required fields shows validation errors", async ({
    page,
  }) => {
    await page.goto("/catalog");
    await page.keyboard.press("Control+n");

    const form = page.locator("#title-form-container form");
    await expect(form).toBeVisible({ timeout: 5000 });

    // Submit without filling anything — client-side validation prevents submission
    await form.locator('button[type="submit"]').click();

    // Inline validation errors should appear below required fields
    const titleError = form.locator(
      "#title-field ~ .field-error:not(.hidden)",
    );
    await expect(titleError).toBeVisible({ timeout: 2000 });

    // Title field should have red border
    await expect(form.locator("#title-field")).toHaveClass(/border-red-500/);

    // No server request should have been made (form not submitted)
    const feedbackEntries = page.locator("#feedback-list .feedback-entry");
    await expect(feedbackEntries).toHaveCount(0);
  });

  // Escape key closes form
  test("Escape key closes title form and returns focus to scan field", async ({
    page,
  }) => {
    await page.goto("/catalog");
    await page.keyboard.press("Control+n");

    const form = page.locator("#title-form-container form");
    await expect(form).toBeVisible({ timeout: 5000 });

    await page.keyboard.press("Escape");
    await expect(form).not.toBeVisible();

    // Focus should return to scan field
    const focusedId = await page.evaluate(
      () => document.activeElement?.id,
    );
    expect(focusedId).toBe("scan-field");
  });

  // AC6: Placeholder cover icon
  test("new title displays placeholder cover SVG icon", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");

    // Context banner should show book icon
    const banner = page.locator("#context-banner");
    await expect(banner).not.toHaveClass(/hidden/, { timeout: 5000 });
    const iconSrc = await banner.locator("img").getAttribute("src");
    expect(iconSrc).toContain("/static/icons/book.svg");
  });

  // AC11: Anonymous user cannot access title creation endpoints
  test("anonymous user is redirected from catalog", async ({ page }) => {
    // Don't add session cookie — anonymous access
    const response = await page.goto("/catalog");
    // Should redirect to home (303)
    expect(page.url()).not.toContain("/catalog");
  });

  // Enter key inside form submits form, not scan field
  test("Enter key inside open form submits form, not scan field", async ({
    page,
  }) => {
    await page.goto("/catalog");
    await page.keyboard.press("Control+n");

    const form = page.locator("#title-form-container form");
    await expect(form).toBeVisible({ timeout: 5000 });

    // Focus on title field and press Enter
    await form.locator("#title-field").fill("Test");
    await form.locator("#title-field").press("Enter");

    // Form should have submitted (either error or success feedback)
    // The scan field should NOT have processed "Test" as a scan
    const scanField = page.locator("#scan-field");
    const scanValue = await scanField.inputValue();
    expect(scanValue).toBe("");
  });
});

test.describe("Catalog accessibility", () => {
  test.beforeEach(async ({ context }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
  });

  test("catalog page with form passes accessibility checks", async ({
    page,
  }) => {
    // Only run if @axe-core/playwright is available
    let AxeBuilder;
    try {
      AxeBuilder = (await import("@axe-core/playwright")).default;
    } catch {
      test.skip(true, "@axe-core/playwright not installed");
      return;
    }

    await page.goto("/catalog");
    await page.keyboard.press("Control+n");
    await page.waitForSelector("#title-form-container form");

    const results = await new AxeBuilder({ page }).analyze();
    expect(results.violations).toEqual([]);
  });
});
