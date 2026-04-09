import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

const VALID_ISBN = specIsbn("BL", 1);

test.describe("Borrower Detail & Loan History (Story 4-4)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
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
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });
    await scanField.fill(volumeLabel);
    await scanField.press("Enter");
    // Wait for the specific volume's feedback (not a stale entry from ISBN scan)
    await expect(page.locator(".feedback-entry").first()).toContainText(
      new RegExp(volumeLabel, "i"),
      { timeout: 5000 }
    );

    // Register loan
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill(volumeLabel);
    await page.locator("#loan-borrower-search").fill(borrowerName.substring(0, 8));
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.waitForFunction(() => document.getElementById("loan-borrower-id")?.value !== "", { timeout: 3000 });
    // Submit and navigate: capture borrower_id, remove HTMX, submit as regular POST
    await page.evaluate(() => {
      const f = document.getElementById("loan-create-form") as HTMLFormElement;
      if (!f) throw new Error("loan-create-form not found in DOM");
      f.removeAttribute("hx-post");
      f.removeAttribute("hx-target");
      f.removeAttribute("hx-swap");
      f.requestSubmit();
    });
    await page.waitForURL(/\/loans/, { timeout: 10000 });
  }

  // AC1: Borrower detail shows active loans section
  test("borrower detail page shows active loans section", async ({ page }) => {
    await createBorrower(page, "BL-Detail Borrower");

    // Navigate to borrower detail
    await page.getByText("BL-Detail Borrower").click();
    await expect(page.locator("h1")).toContainText("BL-Detail Borrower", { timeout: 3000 });

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
    await createBorrower(page, "BL-Loan Detail Borrower");
    await createLoanForBorrower(page, "V0080", "BL-Loan D");

    // Navigate to borrower detail
    await page.goto("/borrowers");
    await page.getByText("BL-Loan Detail Borrower").click();
    await expect(page.locator("h1")).toContainText("BL-Loan Detail Borrower", { timeout: 3000 });

    // Should show the loan with volume label and title
    await expect(page.locator("body")).toContainText("V0080", { timeout: 5000 });

    // Should show Return button
    const returnBtn = page.locator('button:has-text("Return"), button:has-text("Retourner")');
    await expect(returnBtn.first()).toBeVisible({ timeout: 3000 });
  });

  // AC3: Return loan from borrower detail
  test("return loan from borrower detail → loan disappears", async ({ page }) => {
    await createBorrower(page, "BL-Return Detail Borrower");
    await createLoanForBorrower(page, "V0081", "BL-Return");

    // Navigate to borrower detail
    await page.goto("/borrowers");
    await page.getByText("BL-Return Detail Borrower").click();
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

    // Real login via shared helper (Foundation Rule #7)
    await loginAs(page);

    // Create borrower
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("BL-Smoke Detail Borrower");
    await page.locator('button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Create title + volume + loan
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });
    await scanField.fill("V0082");
    await scanField.press("Enter");
    await expect(page.locator(".feedback-entry").first()).toContainText(/V0082/i, { timeout: 10000 });

    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0082");
    await page.locator("#loan-borrower-search").fill("BL-Smoke");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    await page.locator("#borrower-dropdown div").first().click();
    await page.waitForFunction(() => document.getElementById("loan-borrower-id")?.value !== "", { timeout: 3000 });
    await page.locator('button[type="submit"]').last().click();
    await page.waitForTimeout(1000);

    // Navigate to borrower detail — should see loan
    await page.goto("/borrowers");
    await page.getByText("BL-Smoke Detail Borrower").click();
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
