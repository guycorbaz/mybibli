import { test, expect } from "@playwright/test";

// Dev session cookie for librarian access
const DEV_SESSION_COOKIE = {
  name: "session",
  value: "ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2",
  domain: "localhost",
  path: "/",
};

// ISBN known to BnF mock (primary provider)
const BNF_ISBN = "9782070360246";
// ISBN known only to Google Books mock (fallback provider — not in BnF)
const GOOGLE_BOOKS_ISBN = "9780134685991";
// ISBN unknown to all providers (tests all-fail scenario)
const UNKNOWN_ISBN = "9780000000002";

test.describe("Provider Chain & Fallback (Story 3-1)", () => {
  test.beforeEach(async ({ context }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
  });

  // AC2, AC3: Fallback to Google Books when BnF returns no result
  test("scan ISBN unknown to BnF resolves metadata from Google Books fallback", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Scan ISBN that only Google Books knows
    await scanField.fill(GOOGLE_BOOKS_ISBN);
    await scanField.press("Enter");

    // Should see skeleton feedback (async fetch in progress)
    const anyFeedback = page.locator(
      "#feedback-list .feedback-skeleton, #feedback-list .feedback-entry"
    );
    await expect(anyFeedback.first()).toBeVisible({ timeout: 5000 });

    // Wait for async metadata fetch to complete
    await page.waitForTimeout(4000);

    // Trigger OOB delivery by scanning again
    await scanField.fill(GOOGLE_BOOKS_ISBN);
    await scanField.press("Enter");

    // Should see "already exists" info feedback (title was created)
    const infoEntry = page.locator(
      '.feedback-entry[data-feedback-variant="info"]'
    );
    await expect(infoEntry).toBeVisible({ timeout: 5000 });

    // Verify metadata from Google Books appeared on page
    // The mock returns "Effective Java" by "Joshua Bloch"
    await page.waitForTimeout(1000);
    const pageContent = await page.textContent("body");
    expect(
      pageContent?.includes("Effective Java") ||
        pageContent?.includes("Bloch")
    ).toBeTruthy();
  });

  // AC8: All providers fail — title exists with no metadata, no blocking error
  test("scan unknown ISBN creates title even when all providers fail", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Scan ISBN that no provider knows
    await scanField.fill(UNKNOWN_ISBN);
    await scanField.press("Enter");

    // Should see skeleton feedback (async fetch attempted)
    const anyFeedback = page.locator(
      "#feedback-list .feedback-skeleton, #feedback-list .feedback-entry"
    );
    await expect(anyFeedback.first()).toBeVisible({ timeout: 5000 });

    // Wait for all providers to fail
    await page.waitForTimeout(4000);

    // Scan again to confirm title was created despite no metadata
    await scanField.fill(UNKNOWN_ISBN);
    await scanField.press("Enter");

    // Should see "already exists" info — title was created even without metadata
    const infoEntry = page.locator(
      '.feedback-entry[data-feedback-variant="info"]'
    );
    await expect(infoEntry).toBeVisible({ timeout: 5000 });

    // No error feedback should be present — chain failure is silent to user
    const errorEntries = page.locator(
      '.feedback-entry[data-feedback-variant="error"]'
    );
    await expect(errorEntries).toHaveCount(0);
  });

  // AC1, AC2: Primary provider (BnF) still works
  test("scan ISBN known to BnF resolves metadata from primary provider", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill(BNF_ISBN);
    await scanField.press("Enter");

    // Should see feedback
    const anyFeedback = page.locator(
      "#feedback-list .feedback-skeleton, #feedback-list .feedback-entry"
    );
    await expect(anyFeedback.first()).toBeVisible({ timeout: 5000 });

    // Context banner should appear
    const banner = page.locator("#context-banner");
    await expect(banner).not.toHaveClass(/hidden/, { timeout: 5000 });

    // Wait for metadata resolution
    await page.waitForTimeout(3000);
    await scanField.fill(BNF_ISBN);
    await scanField.press("Enter");

    // Verify BnF metadata: "L'Étranger" by "Albert Camus"
    await page.waitForTimeout(1000);
    const pageContent = await page.textContent("body");
    expect(
      pageContent?.includes("tranger") || pageContent?.includes("Camus")
    ).toBeTruthy();
  });
});
