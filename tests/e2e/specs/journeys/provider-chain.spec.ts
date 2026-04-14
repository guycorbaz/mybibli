import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

// ISBN known to BnF mock (primary provider)
const BNF_ISBN = "9782070360246";
// ISBN known only to Google Books mock (fallback provider — not in BnF)
const GOOGLE_BOOKS_ISBN = "9780134685991";
// ISBN unknown to all providers (tests all-fail scenario)
const UNKNOWN_ISBN = "9780000000002";

test.describe("Provider Chain & Fallback (Story 3-1)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
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

    // Trigger OOB delivery by scanning again.
    // Bounded to 15s to cover BnF timeout + Google Books fallback under CI load.
    await scanField.fill(GOOGLE_BOOKS_ISBN);
    await scanField.press("Enter");

    // Should see "already exists" info feedback (title was created)
    const infoEntry = page.locator(
      '.feedback-entry[data-feedback-variant="info"]'
    );
    await expect(infoEntry).toBeVisible({ timeout: 15000 });

    // Mock returns "Effective Java" by "Joshua Bloch"
    await expect(page.locator("body")).toContainText(/Effective Java|Bloch/i, { timeout: 15000 });
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

    // Scan again to confirm title was created despite no metadata.
    // Bounded to 15s for all-providers-fail scenario under CI load.
    await scanField.fill(UNKNOWN_ISBN);
    await scanField.press("Enter");

    // Should see "already exists" info — title was created even without metadata
    const infoEntry = page.locator(
      '.feedback-entry[data-feedback-variant="info"]'
    );
    await expect(infoEntry).toBeVisible({ timeout: 15000 });

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

    // Trigger OOB delivery; bounded to 15s for BnF resolution under CI load.
    await scanField.fill(BNF_ISBN);
    await scanField.press("Enter");

    // Verify BnF metadata: "L'Étranger" by "Albert Camus"
    await expect(page.locator("body")).toContainText(/tranger|Camus/i, { timeout: 15000 });
  });
});
