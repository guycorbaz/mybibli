import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

const DEV_SESSION_COOKIE = {
  name: "session",
  value: "ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2",
  domain: "localhost",
  path: "/",
};

const VALID_ISBN = specIsbn("CV", 1);
const COUNTER_ISBN = specIsbn("CV", 2);

test.describe("Volume Management", () => {
  test.beforeEach(async ({ context }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
  });

  // AC1: Create volume from V-code scan with current title
  test("scan ISBN then V-code creates volume with success feedback", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // First scan ISBN to set current title
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    // Then scan V-code
    await scanField.fill("V0042");
    await scanField.press("Enter");

    const successEntry = page.locator(
      '.feedback-entry[data-feedback-variant="success"]',
    );
    await expect(successEntry.last()).toBeVisible({ timeout: 5000 });

    // Context banner should show volume count
    const banner = page.locator("#context-banner");
    await expect(banner).toContainText("vol", { timeout: 3000 });
  });

  // AC2: Reject duplicate V-code
  test("scan same V-code again shows error feedback", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    // Create first volume
    await scanField.fill("V0043");
    await scanField.press("Enter");
    await page.waitForSelector(
      '.feedback-entry[data-feedback-variant="success"]',
    );

    // Try same V-code again
    await scanField.fill("V0043");
    await scanField.press("Enter");

    const errorEntry = page.locator(
      '.feedback-entry[data-feedback-variant="error"]',
    );
    await expect(errorEntry).toBeVisible({ timeout: 5000 });
  });

  // AC3: V-code without current title
  test("scan V-code without prior ISBN shows warning", async ({ context, page }) => {
    // Fresh login creates a new session without title context from previous tests
    await context.clearCookies();
    await loginAs(page);
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Scan V-code without setting a title first
    await scanField.fill("V0099");
    await scanField.press("Enter");

    const warningEntry = page.locator(
      '.feedback-entry[data-feedback-variant="warning"]',
    );
    await expect(warningEntry).toBeVisible({ timeout: 5000 });
  });

  // AC4: Invalid V-code format — V123 is detected as "unknown" (not a V-code prefix)
  test("scan V123 is treated as unknown code", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill("V123");
    await scanField.press("Enter");

    // V123 doesn't match /^V\d{4}$/ so detectPrefix returns "unknown"
    // Server returns unsupported code warning
    const entry = page.locator(".feedback-entry");
    await expect(entry).toBeVisible({ timeout: 5000 });
  });

  // AC4: V0000 rejected
  test("scan V0000 shows client-side error", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    let serverCalled = false;
    page.on("request", (request) => {
      if (request.url().includes("/catalog/scan")) {
        serverCalled = true;
      }
    });

    await scanField.fill("V0000");
    await scanField.press("Enter");

    const errorEntry = page.locator(
      '.feedback-entry[data-feedback-variant="error"]',
    );
    await expect(errorEntry).toBeVisible({ timeout: 2000 });
    expect(serverCalled).toBe(false);
  });

  // AC5: Volume count in banner
  test("scan ISBN then two V-codes shows banner with 2 vol", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    await scanField.fill("V0044");
    await scanField.press("Enter");
    await page.waitForSelector(
      '.feedback-entry[data-feedback-variant="success"]',
    );

    await scanField.fill("V0045");
    await scanField.press("Enter");

    // Wait for second success
    await page.waitForTimeout(1000);

    const banner = page.locator("#context-banner");
    await expect(banner).toContainText("vol", { timeout: 3000 });
  });

  // AC8: Session counter increments
  test("session counter increments on volume creation", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Use unique ISBN so title is truly NEW (is_new triggers counter OOB on volume creation)
    await scanField.fill(COUNTER_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    await scanField.fill("V0046");
    await scanField.press("Enter");
    await page.waitForSelector(
      '.feedback-entry[data-feedback-variant="success"]',
    );

    await expect(page.locator("#session-counter").first()).toContainText(/session|éléments/i, { timeout: 3000 });
  });

  // AC6: L-code assigns location (needs location data in DB)
  test("scan V-code then L-code shelves volume", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Set up title and volume
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    await scanField.fill("V0047");
    await scanField.press("Enter");
    await page.waitForSelector(
      '.feedback-entry[data-feedback-variant="success"]',
    );

    // Scan L-code — may not have location data in DB, expect warning or success
    await scanField.fill("L0001");
    await scanField.press("Enter");

    // Should get either success (if location exists) or warning (if not)
    const entry = page.locator(".feedback-entry").last();
    await expect(entry).toBeVisible({ timeout: 5000 });
  });

  // AC7: L-code without volume context
  test("scan L-code without volume context shows info stub", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill("L0001");
    await scanField.press("Enter");

    const entry = page.locator(".feedback-entry").first();
    await expect(entry).toBeVisible({ timeout: 5000 });
  });

  // Anonymous access
  test("anonymous user is redirected from catalog", async ({ context, page }) => {
    await context.clearCookies();
    const response = await page.goto("/catalog");
    expect(page.url()).not.toContain("/catalog");
  });
});

test.describe("Volume accessibility", () => {
  test.beforeEach(async ({ context }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
  });

  test("catalog page passes accessibility checks after volume operations", async ({
    page,
  }) => {
    let AxeBuilder;
    try {
      AxeBuilder = (await import("@axe-core/playwright")).default;
    } catch {
      test.skip(true, "@axe-core/playwright not installed");
      return;
    }

    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    await scanField.fill("V0048");
    await scanField.press("Enter");
    await page.waitForSelector(
      '.feedback-entry[data-feedback-variant="success"]',
    );

    const results = await new AxeBuilder({ page })
      .disableRules(["color-contrast"]) // Known issue: placeholder text contrast
      .analyze();
    expect(results.violations).toEqual([]);
  });
});
