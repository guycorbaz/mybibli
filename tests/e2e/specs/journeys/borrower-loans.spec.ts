import { test, expect } from "@playwright/test";

const DEV_SESSION_COOKIE = {
  name: "session",
  value: "ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2",
  domain: "localhost",
  path: "/",
};

const VALID_ISBN = "9782070360246";

test.describe("Borrower Detail & Loan History (Story 4-4)", () => {
  test.beforeEach(async ({ context }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
  });

  // Helper: create borrower and return their detail page URL
  async function createBorrower(page: any, name: string): Promise<void> {
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill(name);
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });
  }

  // Helper: create a loan for a borrower
  async function createLoanForBorrower(page: any, volumeLabel: string, borrowerName: string) {
    // Create title + volume
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-entry", { timeout: 10000 });
    await scanField.fill(volumeLabel);
    await scanField.press("Enter");
    await page.waitForSelector('.feedback-entry[data-feedback-variant="success"]', { timeout: 5000 });

    // Register loan
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill(volumeLabel);
    await page.locator("#loan-borrower-search").fill(borrowerName.substring(0, 8));
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.locator('button[type="submit"]').last().click();
    await page.waitForTimeout(1000);
  }

  // AC1: Borrower detail shows active loans section
  test("borrower detail page shows active loans section", async ({ page }) => {
    await createBorrower(page, "Detail Test Borrower");

    // Navigate to borrower detail
    await page.getByText("Detail Test Borrower").click();
    await expect(page.locator("h1")).toContainText("Detail Test Borrower", { timeout: 3000 });

    // Active loans section should be visible (with empty state)
    await expect(page.locator("body")).toContainText(
      /Active loans|Prêts actifs/i,
      { timeout: 3000 }
    );
    await expect(page.locator("body")).toContainText(
      /no active loans|aucun prêt actif/i,
      { timeout: 3000 }
    );
  });

  // AC2: Loan details shown on borrower page
  test("borrower with loan shows loan details", async ({ page }) => {
    await createBorrower(page, "Loan Detail Borrower");
    await createLoanForBorrower(page, "V0080", "Loan Detail");

    // Navigate to borrower detail
    await page.goto("/borrowers");
    await page.getByText("Loan Detail Borrower").click();
    await expect(page.locator("h1")).toContainText("Loan Detail Borrower", { timeout: 3000 });

    // Should show the loan with volume label and title
    await expect(page.locator("body")).toContainText("V0080", { timeout: 5000 });

    // Should show Return button
    const returnBtn = page.locator('button:has-text("Return"), button:has-text("Retourner")');
    await expect(returnBtn.first()).toBeVisible({ timeout: 3000 });
  });

  // AC3: Return loan from borrower detail
  test("return loan from borrower detail → loan disappears", async ({ page }) => {
    await createBorrower(page, "Return Detail Borrower");
    await createLoanForBorrower(page, "V0081", "Return Detail");

    // Navigate to borrower detail
    await page.goto("/borrowers");
    await page.getByText("Return Detail Borrower").click();
    await expect(page.locator("body")).toContainText("V0081", { timeout: 5000 });

    // Click Return
    page.on("dialog", (dialog) => dialog.accept());
    const returnBtn = page.locator('button:has-text("Return"), button:has-text("Retourner")').first();
    await returnBtn.click();

    // Wait for reload
    await page.waitForTimeout(2000);
    await page.reload();

    // Loan should be gone, empty state shown
    await expect(page.locator("body")).not.toContainText("V0081", { timeout: 5000 });
    await expect(page.locator("body")).toContainText(
      /no active loans|aucun prêt actif/i,
      { timeout: 3000 }
    );
  });

  // Smoke test: full lifecycle
  test("smoke: login → create borrower → lend → detail → return → verify", async ({ context, page }) => {
    await context.clearCookies();

    // Login
    await page.goto("/login");
    await page.locator("#username").fill("admin");
    await page.locator("#password").fill("admin");
    await page.locator('button[type="submit"]').click();
    await expect(page).toHaveURL(/\/catalog/, { timeout: 5000 });

    // Create borrower
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("Smoke Detail Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Create title + volume + loan
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-entry", { timeout: 10000 });
    await scanField.fill("V0082");
    await scanField.press("Enter");
    await page.waitForSelector('.feedback-entry[data-feedback-variant="success"]', { timeout: 5000 });

    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0082");
    await page.locator("#loan-borrower-search").fill("Smoke De");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.locator('button[type="submit"]').last().click();
    await page.waitForTimeout(1000);

    // Navigate to borrower detail — should see loan
    await page.goto("/borrowers");
    await page.getByText("Smoke Detail Borrower").click();
    await expect(page.locator("body")).toContainText("V0082", { timeout: 5000 });

    // Return from detail page
    page.on("dialog", (dialog) => dialog.accept());
    const returnBtn = page.locator('button:has-text("Return"), button:has-text("Retourner")').first();
    await returnBtn.click();
    await page.waitForTimeout(2000);
    await page.reload();

    // Verify loan is gone
    await expect(page.locator("body")).not.toContainText("V0082", { timeout: 5000 });
  });
});
