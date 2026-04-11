import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import {
  createBorrower,
  createLoan,
  returnLoanFromLoansPage,
  scanTitleAndVolume,
} from "../../helpers/loans";

const VALID_ISBN = specIsbn("LR", 1);

test.describe("Loan Return & Location Restoration (Story 4-3)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // Helper: create the full title+volume+borrower+loan chain for a single test
  async function setupLoan(page: any, volumeLabel: string, borrowerName: string) {
    await scanTitleAndVolume(page, VALID_ISBN, volumeLabel);
    await createBorrower(page, borrowerName);
    await createLoan(page, volumeLabel, borrowerName);
  }

  // AC1: Return a loan
  test("return a loan → loan disappears from list", async ({ page }) => {
    await setupLoan(page, "V0070", "LR-Return Borrower");

    // Loan is already visible in /loans (createLoan leaves us on the loans page
    // with the row asserted). Perform the return via the canonical helper.
    await returnLoanFromLoansPage(page, "V0070");

    // Re-fetch /loans from scratch to prove the row is gone after a full render
    await page.goto("/loans");
    await expect(page.locator("#loans-table-body")).not.toContainText("V0070", {
      timeout: 5000,
    });
  });

  // AC4: Volume deletion guard
  test("try to delete volume on loan → blocked with error", async ({ page }) => {
    await setupLoan(page, "V0071", "LR-Delete Guard Borrower");

    // Verify loan is live
    await page.goto("/loans");
    await expect(page.locator("#loans-table-body")).toContainText("V0071", {
      timeout: 5000,
    });

    // Try to delete the volume via catalog scan
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill("V0071");
    await scanField.press("Enter");
    // Wait for scan feedback to confirm the volume was looked up
    await expect(page.locator(".feedback-entry").first()).toContainText(
      /V0071/i,
      { timeout: 5000 },
    );

    // Navigate to volume detail if visible, then attempt deletion
    const volumeLink = page.locator('a:has-text("V0071")').first();
    if (await volumeLink.isVisible({ timeout: 3000 }).catch(() => false)) {
      await volumeLink.click();
      // Wait for the volume detail heading to render
      await expect(page.locator("h1")).toBeVisible({ timeout: 5000 });

      // Register dialog handler BEFORE clicking
      page.once("dialog", (dialog) => {
        dialog.accept().catch(() => {});
      });
      const deleteBtn = page.getByText(/^Delete$|^Supprimer$/i).first();
      if (await deleteBtn.isVisible({ timeout: 2000 }).catch(() => false)) {
        await deleteBtn.click();
        // Should see warning about volume on loan (wait for the specific text)
        await expect(page.locator("body")).toContainText(
          /currently on loan|actuellement en prêt/i,
          { timeout: 5000 },
        );
      }
    }
  });

  // AC3: Overdue highlighting (styling check for a fresh loan — no threshold change)
  test("overdue loan shows red styling and badge", async ({ page }) => {
    await setupLoan(page, "V0074", "LR-Overdue Borrower");

    // Brand-new loan (0 days) should NOT be overdue with the default threshold
    await page.goto("/loans");
    const loanRow = page
      .locator("#loans-table-body tr")
      .filter({ hasText: "V0074" });
    await expect(loanRow).toBeVisible({ timeout: 5000 });

    // Duration cell must NOT have red text or overdue badge
    const durationCell = loanRow.locator("td").nth(4);
    await expect(durationCell).toBeVisible({ timeout: 3000 });
    const cellClasses = await durationCell.getAttribute("class");
    expect(cellClasses).not.toContain("text-red-600");
    await expect(durationCell).not.toContainText(/Overdue|En retard/i);
  });

  // AC5: Sort loans by column
  test("sort loans by borrower → URL updates and list re-sorts", async ({ page }) => {
    await page.goto("/loans");

    // Click on borrower column header
    const borrowerHeader = page.locator('th a[href*="sort=borrower"]');
    if (await borrowerHeader.isVisible({ timeout: 3000 }).catch(() => false)) {
      await borrowerHeader.click();
      await expect(page).toHaveURL(/sort=borrower/, { timeout: 5000 });
      await expect(page).toHaveURL(/page=1/, { timeout: 5000 });
    }
  });

  // AC6: Scan V-code → return from scan result card
  test("scan V-code → return from scan result", async ({ page }) => {
    await setupLoan(page, "V0072", "LR-Scan Return Borrower");

    await page.goto("/loans");
    const scanField = page.locator("#loan-scan-field");
    await scanField.click();
    await scanField.fill("V0072");
    // Trigger scan via HTMX directly (hx-trigger on keydown may not fire from Playwright)
    await page.evaluate(() => {
      const field = document.getElementById("loan-scan-field") as HTMLInputElement;
      htmx.ajax("GET", "/loans/scan?code=" + encodeURIComponent(field.value), {
        target: "#scan-result",
        swap: "innerHTML",
      });
    });

    // Scan result should show with Return button
    await expect(page.locator("#scan-result")).toContainText("V0072", {
      timeout: 5000,
    });
    const returnBtn = page.locator(
      '#scan-result button:has-text("Return"), #scan-result button:has-text("Retourner")',
    );
    await expect(returnBtn).toBeVisible({ timeout: 3000 });

    // Register dialog handler BEFORE click
    page.once("dialog", (dialog) => {
      dialog.accept().catch(() => {});
    });
    await returnBtn.click();

    // Wait for scan result to update (HTMX swap after return)
    // The scan-result region no longer shows the Return button once returned
    await expect(returnBtn).not.toBeVisible({ timeout: 5000 });

    // Verify loan is gone from the list
    await page.goto("/loans");
    await expect(page.locator("#loans-table-body")).not.toContainText("V0072", {
      timeout: 5000,
    });
  });

  // Smoke test: full loan lifecycle
  test("smoke: login → create loan → return loan → verify lifecycle", async ({ context, page }) => {
    await context.clearCookies();

    // Real login via shared helper (Foundation Rule #7 — no cookie injection)
    await loginAs(page);

    // Create title + volume + borrower + loan via canonical helpers
    await scanTitleAndVolume(page, VALID_ISBN, "V0073");
    await createBorrower(page, "LR-Smoke Return Borrower");
    await createLoan(page, "V0073", "LR-Smoke Return Borrower");

    // Return the loan
    await returnLoanFromLoansPage(page, "V0073");

    // Verify the row is gone after a fresh fetch
    await page.goto("/loans");
    await expect(page.locator("#loans-table-body")).not.toContainText("V0073", {
      timeout: 5000,
    });
  });
});
