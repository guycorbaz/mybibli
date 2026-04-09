import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

const VALID_ISBN = specIsbn("LN", 1);

test.describe("Loan Registration & Validation (Story 4-2)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
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
    await expect(page.locator(".feedback-entry").first()).toContainText(/V0060/i, { timeout: 10000 });

    // Create a borrower
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("LN-Loan Test Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Get borrower ID from the link
    const borrowerLink = page.getByText("LN-Loan Test Borrower");
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
    await page.locator("#loan-borrower-search").fill("LN-Loan");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.waitForFunction(() => document.getElementById("loan-borrower-id")?.value !== "", { timeout: 3000 });

    // Submit the loan form
    await page.locator("#loan-create-form button[type='submit']").click();

    // Wait for loan creation feedback
    await expect(page.locator("#loan-feedback")).toContainText(/created|créé|V0060/i, { timeout: 10000 });

    // Reload loans page to verify loan appears
    await page.goto("/loans");
    await expect(page.locator("body")).toContainText("V0060", { timeout: 5000 });
    await expect(page.locator("body")).toContainText("LN-Loan Test Borrower");
  });

  // AC3: Prevent loan of non-loanable volume
  test("attempt to lend non-loanable volume → verify error", async ({ page }) => {
    // Create title + volume via catalog (idempotent: handles repeat runs)
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

    await scanField.fill("V0063");
    await scanField.press("Enter");
    // Accept any feedback: success (first run) or error (V-code exists from prior run)
    await page.waitForSelector(".feedback-entry", { timeout: 5000 });

    // Find the volume ID by searching volume detail pages for label V0063
    const volumeId = await page.evaluate(async () => {
      for (let id = 1; id <= 100; id++) {
        try {
          const resp = await fetch(`/volume/${id}`);
          if (!resp.ok) continue;
          const html = await resp.text();
          if (html.includes("V0063")) return id;
        } catch { continue; }
      }
      return null;
    });
    expect(volumeId).not.toBeNull();

    // Navigate to volume edit page and set condition to "Endommagé" (non-loanable)
    await page.goto(`/volume/${volumeId}/edit`);
    const conditionSelect = page.locator('select[name="condition_state_id"]');
    await expect(conditionSelect).toBeVisible({ timeout: 3000 });
    await conditionSelect.selectOption({ label: "Endommagé" });
    await page.locator('button[type="submit"]').click();
    await page.waitForLoadState("networkidle");

    // Create a borrower for the loan attempt (unique name per run not needed — borrower search is prefix-based)
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("LN-NonLoanable Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Attempt to lend the non-loanable volume
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0063");
    await page.locator("#loan-borrower-search").fill("LN-NonLoanable");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.waitForFunction(() => document.getElementById("loan-borrower-id")?.value !== "", { timeout: 3000 });
    await page.locator("#loan-create-form button[type='submit']").click();

    // Should show error about non-loanable condition
    await expect(page.locator("#loan-feedback")).toContainText(
      /condition does not allow|ne permet pas le prêt/i,
      { timeout: 5000 }
    );
  });

  // AC4: Prevent double loan
  test("attempt to lend volume already on loan → verify error", async ({ page }) => {
    // Setup: create title, volume, borrower (idempotent: handles repeat runs)
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

    await scanField.fill("V0061");
    await scanField.press("Enter");
    // Accept any feedback: success (first run) or error (V-code exists from prior run)
    await page.waitForSelector(".feedback-entry", { timeout: 5000 });

    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("LN-Double Loan Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Register first loan (may already be on loan from prior repeat run — that's OK)
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0061");
    await page.locator("#loan-borrower-search").fill("LN-Double");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.waitForFunction(() => document.getElementById("loan-borrower-id")?.value !== "", { timeout: 3000 });
    await page.locator("#loan-create-form button[type='submit']").click();

    // Wait for any feedback (success on first run, "already on loan" on repeat runs)
    await expect(page.locator("#loan-feedback")).toContainText(/created|créé|V0061|already on loan|déjà en prêt/i, { timeout: 10000 });

    // Attempt another loan on same volume — should always fail
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0061");
    await page.locator("#loan-borrower-search").fill("LN-Double");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.waitForFunction(() => document.getElementById("loan-borrower-id")?.value !== "", { timeout: 3000 });
    await page.locator("#loan-create-form button[type='submit']").click();

    // Should show error feedback
    await expect(page.locator("#loan-feedback")).toContainText(/already on loan|déjà en prêt/i, { timeout: 10000 });
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
    await expect(page.locator(".feedback-entry").first()).toContainText(/V0062/i, { timeout: 10000 });

    // Create borrower
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("LN-Smoke Loan Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Navigate to loans
    await page.goto("/loans");
    await expect(page.locator("h1")).toContainText(/Active loans|Prêts actifs/i);

    // Register loan
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0062");
    await page.locator("#loan-borrower-search").fill("LN-Smoke");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.waitForFunction(() => document.getElementById("loan-borrower-id")?.value !== "", { timeout: 3000 });
    await page.locator("#loan-create-form button[type='submit']").click();

    // Wait for loan creation feedback
    await expect(page.locator("#loan-feedback")).toContainText(/created|créé|V0062/i, { timeout: 10000 });

    // Verify in list
    await page.goto("/loans");
    await expect(page.locator("body")).toContainText("V0062", { timeout: 5000 });
    await expect(page.locator("body")).toContainText("LN-Smoke Loan Borrower");
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
    await expect(page.locator(".feedback-entry").first()).toContainText(/V0090/i, { timeout: 10000 });

    // Create borrower
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("LN-TIMESTAMP Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Register loan (volume has no location — previous_location_id will be NULL)
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0090");
    await page.locator("#loan-borrower-search").fill("LN-TIMESTAMP");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.waitForFunction(() => document.getElementById("loan-borrower-id")?.value !== "", { timeout: 3000 });
    await page.locator("#loan-create-form button[type='submit']").click();
    // Wait for loan feedback (on loans page, #loan-feedback gets HTMX swap)
    await expect(page.locator("#loan-feedback")).toContainText(/V0090|created|créé/i, { timeout: 10000 });

    // Navigate to /loans — page must render without 500 Internal Server Error
    await page.goto("/loans");
    await expect(page.locator("h1")).toContainText(/Active loans|Prêts actifs/i, { timeout: 5000 });

    // Verify the loan appears in the table (not an error page)
    await expect(page.locator("#loans-table-body")).toContainText("V0090", { timeout: 5000 });
    await expect(page.locator("#loans-table-body")).toContainText("LN-TIMESTAMP Borrower");

    // Verify the page has loan data columns (duration, date) — confirms TIMESTAMP decoded correctly
    await expect(page.locator("#loans-table-body")).toContainText(/\d+ days|\d+ jours/i);
  });
});
