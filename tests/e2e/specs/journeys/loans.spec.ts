import { test, expect } from "@playwright/test";
import { specIsbn } from "../../helpers/isbn";

const DEV_SESSION_COOKIE = {
  name: "session",
  value: "ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2",
  domain: "localhost",
  path: "/",
};

const VALID_ISBN = specIsbn("LN", 1);

test.describe("Loan Registration & Validation (Story 4-2)", () => {
  test.beforeEach(async ({ context, page }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
  });

  // AC1: Loans page with active loans list
  test("navigate to /loans → see list or empty state", async ({ page }) => {
    await page.goto("/loans");
    await expect(page.locator("h1")).toContainText(/Active loans|Prêts actifs/i);
  });

  // AC1: Anonymous users redirected
  test("anonymous users are redirected to login", async ({ context, page }) => {
    await context.clearCookies();
    await page.goto("/loans");
    await expect(page).toHaveURL(/\/login/, { timeout: 5000 });
  });

  // AC2: Register a loan
  test("register a loan → verify loan appears in list", async ({ page }) => {
    // First, create a title and volume via catalog
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

    // Create a volume
    await scanField.fill("V0060");
    await scanField.press("Enter");
    await page.waitForSelector('.feedback-entry[data-feedback-variant="success"]', { timeout: 5000 });

    // Create a borrower
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("Loan Test Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Get borrower ID from the link
    const borrowerLink = page.getByText("Loan Test Borrower");
    await expect(borrowerLink).toBeVisible({ timeout: 3000 });

    // Navigate to loans page
    await page.goto("/loans");
    await expect(page.locator("h1")).toContainText(/Active loans|Prêts actifs/i);

    // Click New loan
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await expect(page.locator("#loan-volume-label")).toBeVisible({ timeout: 3000 });

    // Fill in volume label
    await page.locator("#loan-volume-label").fill("V0060");

    // Search for borrower
    await page.locator("#loan-borrower-search").fill("Loan Test");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();

    // Submit the loan form
    await page.locator('button[type="submit"]').last().click();

    // Wait for feedback or page refresh
    await page.waitForTimeout(1000);

    // Reload loans page to verify loan appears
    await page.goto("/loans");
    await expect(page.locator("body")).toContainText("V0060", { timeout: 5000 });
    await expect(page.locator("body")).toContainText("Loan Test Borrower");
  });

  // AC3: Prevent loan of non-loanable volume
  test("attempt to lend non-loanable volume → verify error", async ({ page }) => {
    // Create title + volume via catalog
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

    await scanField.fill("V0063");
    await scanField.press("Enter");
    await page.waitForSelector('.feedback-entry[data-feedback-variant="success"]', { timeout: 5000 });

    // Navigate to volume edit page to set condition to "Endommagé" (non-loanable)
    // Find the volume edit page by scanning V0063 context, then navigating via title
    const feedbackSkel = page.locator("[id^='feedback-entry-']").first();
    const skelId = await feedbackSkel.getAttribute("id").catch(() => null);
    const titleId = skelId?.replace("feedback-entry-", "");

    if (titleId) {
      // Navigate to title detail → find volume → click edit
      await page.goto(`/title/${titleId}`);
      const volEditLink = page.locator('a[href*="/volume/"][href*="/edit"]').first();
      if (await volEditLink.isVisible({ timeout: 3000 }).catch(() => false)) {
        await volEditLink.click();
      } else {
        // Try volume detail link then edit
        const volLink = page.locator('a[href*="/volume/"]').first();
        if (await volLink.isVisible({ timeout: 3000 }).catch(() => false)) {
          await volLink.click();
          const editBtn = page.locator('a[href*="/edit"]').first();
          await expect(editBtn).toBeVisible({ timeout: 3000 });
          await editBtn.click();
        }
      }
    }

    // Set condition to "Endommagé" if we're on the edit page
    const conditionSelect = page.locator('select[name="condition_state_id"]');
    if (await conditionSelect.isVisible({ timeout: 3000 }).catch(() => false)) {
      await conditionSelect.selectOption({ label: "Endommagé" });
      await page.locator('button[type="submit"]').click();
      await page.waitForLoadState("networkidle");
    }

    // Create a borrower for the loan attempt
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("NonLoanable Test Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Attempt to lend the non-loanable volume
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0063");
    await page.locator("#loan-borrower-search").fill("NonLoanable");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.locator('button[type="submit"]').last().click();

    // Should show error about non-loanable condition
    await expect(page.locator("#loan-feedback")).toContainText(
      /condition does not allow|ne permet pas le prêt/i,
      { timeout: 5000 }
    );
  });

  // AC4: Prevent double loan
  test("attempt to lend volume already on loan → verify error", async ({ page }) => {
    // Setup: create title, volume, borrower and first loan
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

    await scanField.fill("V0061");
    await scanField.press("Enter");
    await page.waitForSelector('.feedback-entry[data-feedback-variant="success"]', { timeout: 5000 });

    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("Double Loan Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Register first loan
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0061");
    await page.locator("#loan-borrower-search").fill("Double Loan");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.locator('button[type="submit"]').last().click();

    // Wait for success feedback confirming first loan was created
    await expect(page.locator("#loan-feedback")).toContainText(/created|créé|V0061/i, { timeout: 5000 });

    // Attempt second loan on same volume
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0061");
    await page.locator("#loan-borrower-search").fill("Double Loan");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.locator('button[type="submit"]').last().click();

    // Should show error feedback
    await expect(page.locator("#loan-feedback")).toContainText(/already on loan|déjà en prêt/i, { timeout: 5000 });
  });

  // AC5: Scan V-code on loans page
  test("scan V-code on loans page → verify loan row or feedback", async ({ page }) => {
    await page.goto("/loans");

    const scanField = page.locator("#loan-scan-field");
    await expect(scanField).toBeVisible({ timeout: 3000 });

    // Scan a non-existent V-code via HTMX
    await scanField.click();
    await scanField.fill("V9999");
    // Trigger the scan via HTMX (hx-trigger on keydown may not fire from Playwright)
    await page.evaluate(() => {
      const field = document.getElementById("loan-scan-field") as HTMLInputElement;
      htmx.ajax("GET", "/loans/scan?code=" + encodeURIComponent(field.value), {
        target: "#scan-result",
        swap: "innerHTML",
      });
    });

    // Should show not found or not on loan
    await expect(page.locator("#scan-result")).toContainText(
      /not found|introuvable|not currently on loan|pas en prêt|Volume not found/i,
      { timeout: 5000 }
    );
  });

  // Smoke test: login → /loans → register loan → verify in list
  test("smoke: login → loans → register loan → verify", async ({ context, page }) => {
    await context.clearCookies();

    // Login
    await page.goto("/login");
    await page.locator("#username").fill("admin");
    await page.locator("#password").fill("admin");
    await page.locator('button[type="submit"]').click();
    await expect(page).toHaveURL(/\/catalog/, { timeout: 5000 });

    // Create a title+volume via catalog
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

    await scanField.fill("V0062");
    await scanField.press("Enter");
    await page.waitForSelector('.feedback-entry[data-feedback-variant="success"]', { timeout: 5000 });

    // Create borrower
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("Smoke Loan Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Navigate to loans
    await page.goto("/loans");
    await expect(page.locator("h1")).toContainText(/Active loans|Prêts actifs/i);

    // Register loan
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0062");
    await page.locator("#loan-borrower-search").fill("Smoke Loan");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.locator('button[type="submit"]').last().click();
    await page.waitForTimeout(1000);

    // Verify in list
    await page.goto("/loans");
    await expect(page.locator("body")).toContainText("V0062", { timeout: 5000 });
    await expect(page.locator("body")).toContainText("Smoke Loan Borrower");
  });

  // Regression: TIMESTAMP column decoding — loans page must render when active loans exist
  // Bug: dynamic sqlx::query() could not decode MariaDB TIMESTAMP into NaiveDateTime.
  // Fix: CAST(loaned_at AS DATETIME) in all dynamic loan queries.
  test("regression: loans page renders with active loan (TIMESTAMP fix)", async ({ page }) => {
    // Create a title + volume (no location assigned — volume stays unshelved)
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

    await scanField.fill("V0090");
    await scanField.press("Enter");
    await page.waitForSelector('.feedback-entry[data-feedback-variant="success"]', { timeout: 5000 });

    // Create borrower
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("TIMESTAMP Regression Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Register loan (volume has no location — previous_location_id will be NULL)
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0090");
    await page.locator("#loan-borrower-search").fill("TIMESTAMP");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.locator('button[type="submit"]').last().click();
    await page.waitForSelector('.feedback-entry[data-feedback-variant="success"]', { timeout: 5000 });

    // Navigate to /loans — page must render without 500 Internal Server Error
    await page.goto("/loans");
    await expect(page.locator("h1")).toContainText(/Active loans|Prêts actifs/i, { timeout: 5000 });

    // Verify the loan appears in the table (not an error page)
    await expect(page.locator("#loans-table-body")).toContainText("V0090", { timeout: 5000 });
    await expect(page.locator("#loans-table-body")).toContainText("TIMESTAMP Regression Borrower");

    // Verify the page has loan data columns (duration, date) — confirms TIMESTAMP decoded correctly
    await expect(page.locator("#loans-table-body")).toContainText(/\d+ days|\d+ jours/i);
  });
});
