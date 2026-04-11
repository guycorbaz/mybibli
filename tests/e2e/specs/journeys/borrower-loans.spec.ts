import { test, expect, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import {
  createBorrower,
  createLoan,
  scanTitleAndVolume,
} from "../../helpers/loans";

const VALID_ISBN = specIsbn("BL", 1);

test.describe("Borrower Detail & Loan History (Story 4-4)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  /**
   * Return a loan from the borrower detail page (not from /loans).
   * Registers the dialog handler BEFORE clicking and waits for the row to
   * disappear from the borrower's active loans section.
   */
  async function returnLoanFromBorrowerDetail(
    page: Page,
    volumeLabel: string,
  ): Promise<void> {
    page.once("dialog", (dialog) => {
      dialog.accept().catch(() => {});
    });
    // Scope the Return button to the active loans section so loan-history
    // or other sections on the detail page cannot pick up a stale button.
    const activeLoans = page.locator("#active-loans-section");
    const returnBtn = activeLoans
      .locator('button:has-text("Return"), button:has-text("Retourner")')
      .first();
    await expect(returnBtn).toBeVisible({ timeout: 3000 });
    await returnBtn.click();
    // Wait for the volume label to disappear from the ACTIVE LOANS section.
    // Asserting on body would false-fail if the label appears elsewhere
    // (flash message, loan history, breadcrumbs).
    await expect(activeLoans).not.toContainText(volumeLabel, {
      timeout: 10000,
    });
  }

  // AC1: Borrower detail shows active loans section
  test("borrower detail page shows active loans section", async ({ page }) => {
    await createBorrower(page, "BL-Detail Borrower");

    // Navigate to borrower detail
    await page.getByText("BL-Detail Borrower").click();
    await expect(page.locator("h1")).toContainText("BL-Detail Borrower", {
      timeout: 3000,
    });

    // Active loans section should be visible (with empty state)
    await expect(page.locator("body")).toContainText(
      /Active loans|Prêts actifs/i,
      { timeout: 3000 },
    );
    await expect(page.locator("body")).toContainText(
      /no active loans|aucun prêt actif/i,
      { timeout: 3000 },
    );
  });

  // AC2: Loan details shown on borrower page
  test("borrower with loan shows loan details", async ({ page }) => {
    await createBorrower(page, "BL-Loan Detail Borrower");
    await scanTitleAndVolume(page, VALID_ISBN, "V0080");
    await createLoan(page, "V0080", "BL-Loan Detail Borrower");

    // Navigate to borrower detail
    await page.goto("/borrowers");
    await page.getByText("BL-Loan Detail Borrower").click();
    await expect(page.locator("h1")).toContainText("BL-Loan Detail Borrower", {
      timeout: 3000,
    });

    // Should show the loan with volume label and title
    await expect(page.locator("body")).toContainText("V0080", { timeout: 5000 });

    // Should show Return button
    const returnBtn = page.locator(
      'button:has-text("Return"), button:has-text("Retourner")',
    );
    await expect(returnBtn.first()).toBeVisible({ timeout: 3000 });
  });

  // AC3: Return loan from borrower detail
  test("return loan from borrower detail → loan disappears", async ({ page }) => {
    await createBorrower(page, "BL-Return Detail Borrower");
    await scanTitleAndVolume(page, VALID_ISBN, "V0081");
    await createLoan(page, "V0081", "BL-Return Detail Borrower");

    // Navigate to borrower detail
    await page.goto("/borrowers");
    await page.getByText("BL-Return Detail Borrower").click();
    await expect(page.locator("#active-loans-section")).toContainText("V0081", {
      timeout: 5000,
    });

    // Return via the canonical helper
    await returnLoanFromBorrowerDetail(page, "V0081");

    // Reload the borrower detail page and verify the empty state
    await page.reload();
    const activeLoans = page.locator("#active-loans-section");
    await expect(activeLoans).not.toContainText("V0081", { timeout: 5000 });
    await expect(activeLoans).toContainText(
      /no active loans|aucun prêt actif/i,
      { timeout: 3000 },
    );
  });

  // Smoke test: full lifecycle
  test("smoke: login → create borrower → lend → detail → return → verify", async ({
    context,
    page,
  }) => {
    await context.clearCookies();

    // Real login via shared helper (Foundation Rule #7)
    await loginAs(page);

    // Create borrower + title + volume + loan
    await createBorrower(page, "BL-Smoke Detail Borrower");
    await scanTitleAndVolume(page, VALID_ISBN, "V0082");
    await createLoan(page, "V0082", "BL-Smoke Detail Borrower");

    // Navigate to borrower detail — should see loan
    await page.goto("/borrowers");
    await page.getByText("BL-Smoke Detail Borrower").click();
    const activeLoans = page.locator("#active-loans-section");
    await expect(activeLoans).toContainText("V0082", { timeout: 5000 });

    // Return from detail page
    await returnLoanFromBorrowerDetail(page, "V0082");

    // Reload and verify
    await page.reload();
    await expect(page.locator("#active-loans-section")).not.toContainText(
      "V0082",
      { timeout: 5000 },
    );
  });
});
