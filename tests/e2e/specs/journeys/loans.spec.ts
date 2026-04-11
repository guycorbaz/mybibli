import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import {
  createBorrower,
  createLoan,
  scanTitleAndVolume,
} from "../../helpers/loans";

const VALID_ISBN = specIsbn("LN", 1);

test.describe("Loan Registration & Validation (Story 4-2)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // AC1: Loans page renders
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
    await scanTitleAndVolume(page, VALID_ISBN, "V0060");
    await createBorrower(page, "LN-Loan Test Borrower");
    await createLoan(page, "V0060", "LN-Loan Test Borrower");

    // createLoan leaves the page on /loans with the row asserted. Double-check
    // the row also shows the borrower name.
    await expect(page.locator("#loans-table-body")).toContainText(
      "LN-Loan Test Borrower",
    );
  });

  // AC3: Prevent loan of non-loanable volume
  test("attempt to lend non-loanable volume → verify error", async ({ page }) => {
    await scanTitleAndVolume(page, VALID_ISBN, "V0063");

    // Find the volume ID by scanning volume detail pages for label V0063.
    // The range is bounded by the number of volumes seeded in this test run.
    const volumeId = await page.evaluate(async () => {
      for (let id = 1; id <= 100; id++) {
        try {
          const resp = await fetch(`/volume/${id}`);
          if (!resp.ok) continue;
          const html = await resp.text();
          if (html.includes("V0063")) return id;
        } catch {
          continue;
        }
      }
      return null;
    });
    expect(volumeId).not.toBeNull();

    // Set the volume condition to "Endommagé" (non-loanable)
    await page.goto(`/volume/${volumeId}/edit`);
    const conditionSelect = page.locator('select[name="condition_state_id"]');
    await expect(conditionSelect).toBeVisible({ timeout: 3000 });
    await conditionSelect.selectOption({ label: "Endommagé" });
    await page.locator('button[type="submit"]').click();
    // Positive assertion on the volume detail URL — the handler returns
    // `Redirect::to("/volume/{id}")` on success (src/routes/catalog.rs).
    // Negative assertions like `not.toHaveURL(/\/edit$/)` false-pass on 4xx
    // error pages whose URL carries a query string. Tail allows `$`, query
    // string, or fragment so a future flash-message redirect like
    // `?updated=1` still passes.
    await expect(page).toHaveURL(
      new RegExp(`/volume/${volumeId}(?:$|[?#])`),
      { timeout: 5000 },
    );

    // Create a borrower for the loan attempt
    await createBorrower(page, "LN-NonLoanable Borrower");

    // Attempt to lend the non-loanable volume — should be blocked
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0063");
    await page.locator("#loan-borrower-search").fill("LN-NonLoanable");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    const match = page
      .locator("#borrower-dropdown div")
      .filter({ hasText: "LN-NonLoanable Borrower" });
    await expect(match.first()).toBeVisible({ timeout: 3000 });
    await match.first().click();
    await page.waitForFunction(
      () =>
        (document.getElementById("loan-borrower-id") as HTMLInputElement | null)
          ?.value !== "",
      { timeout: 3000 },
    );
    await page.locator("#loan-create-form button[type='submit']").click();

    // Should show error about non-loanable condition
    await expect(page.locator("#loan-feedback")).toContainText(
      /condition does not allow|ne permet pas le prêt/i,
      { timeout: 5000 },
    );
  });

  // AC4: Prevent double loan
  test("attempt to lend volume already on loan → verify error", async ({ page }) => {
    await scanTitleAndVolume(page, VALID_ISBN, "V0061");
    await createBorrower(page, "LN-Double Loan Borrower");

    // Register first loan (idempotent — may already be on loan from a prior
    // partial run if the DB wasn't wiped, but the assertion accepts both)
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0061");
    await page.locator("#loan-borrower-search").fill("LN-Double");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    const match1 = page
      .locator("#borrower-dropdown div")
      .filter({ hasText: "LN-Double Loan Borrower" });
    await expect(match1.first()).toBeVisible({ timeout: 3000 });
    await match1.first().click();
    await page.waitForFunction(
      () =>
        (document.getElementById("loan-borrower-id") as HTMLInputElement | null)
          ?.value !== "",
      { timeout: 3000 },
    );
    await page.locator("#loan-create-form button[type='submit']").click();
    await expect(page.locator("#loan-feedback")).toContainText(
      /created|créé|V0061|already on loan|déjà en prêt/i,
      { timeout: 10000 },
    );

    // Attempt another loan on same volume — should always fail
    await page.goto("/loans");
    await page.getByText(/New loan|Nouveau prêt/i).click();
    await page.locator("#loan-volume-label").fill("V0061");
    await page.locator("#loan-borrower-search").fill("LN-Double");
    await page.waitForSelector("#borrower-dropdown div", { timeout: 5000 });
    const match2 = page
      .locator("#borrower-dropdown div")
      .filter({ hasText: "LN-Double Loan Borrower" });
    await expect(match2.first()).toBeVisible({ timeout: 3000 });
    await match2.first().click();
    await page.waitForFunction(
      () =>
        (document.getElementById("loan-borrower-id") as HTMLInputElement | null)
          ?.value !== "",
      { timeout: 3000 },
    );
    await page.locator("#loan-create-form button[type='submit']").click();

    // Should show error feedback
    await expect(page.locator("#loan-feedback")).toContainText(
      /already on loan|déjà en prêt/i,
      { timeout: 10000 },
    );
  });

  // AC5: Scan V-code on loans page
  test("scan V-code on loans page → verify loan row or feedback", async ({ page }) => {
    await page.goto("/loans");

    const scanField = page.locator("#loan-scan-field");
    await expect(scanField).toBeVisible({ timeout: 3000 });

    // Scan a non-existent V-code via HTMX
    await scanField.click();
    await scanField.fill("V9999");
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
      { timeout: 5000 },
    );
  });

  // Smoke test: login → /loans → register loan → verify in list
  test("smoke: login → loans → register loan → verify", async ({ context, page }) => {
    await context.clearCookies();

    // Real login via shared helper (Foundation Rule #7 — no cookie injection)
    await loginAs(page);

    // Create the loan chain via canonical helpers
    await scanTitleAndVolume(page, VALID_ISBN, "V0062");
    await createBorrower(page, "LN-Smoke Loan Borrower");
    await createLoan(page, "V0062", "LN-Smoke Loan Borrower");

    // Verify in list — createLoan already asserts the row; double-check borrower
    await expect(page.locator("#loans-table-body")).toContainText(
      "LN-Smoke Loan Borrower",
    );
  });

  // Regression: TIMESTAMP column decoding — loans page must render when active loans exist
  // Bug: dynamic sqlx::query() could not decode MariaDB TIMESTAMP into NaiveDateTime.
  // Fix: CAST(loaned_at AS DATETIME) in all dynamic loan queries.
  test("regression: loans page renders with active loan (TIMESTAMP fix)", async ({ page }) => {
    await scanTitleAndVolume(page, VALID_ISBN, "V0090");
    await createBorrower(page, "LN-TIMESTAMP Borrower");
    await createLoan(page, "V0090", "LN-TIMESTAMP Borrower");

    // Verify the page rendered fully (not a 500 error page) with borrower and
    // duration columns — this is what the TIMESTAMP decoding regression check
    // specifically validates.
    await expect(page.locator("h1")).toContainText(/Active loans|Prêts actifs/i, {
      timeout: 5000,
    });
    await expect(page.locator("#loans-table-body")).toContainText(
      "LN-TIMESTAMP Borrower",
    );
    // Duration column: must contain a number + days/jours, proving TIMESTAMP decoded
    await expect(page.locator("#loans-table-body")).toContainText(/\d+ days|\d+ jours/i);
  });
});
