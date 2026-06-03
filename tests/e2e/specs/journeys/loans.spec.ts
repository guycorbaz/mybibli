import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import {
  createBorrower,
  createLoan,
  scanTitleAndVolume,
} from "../../helpers/loans";
import { titleIdFromSkeleton, volumeIdByLabel } from "../../helpers/catalog";

// CR #300: per-test unique ISBNs — each test gets a fresh title so the
// V-code scan never hits the phantom-volume confirmation modal.

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
    await scanTitleAndVolume(page, specIsbn("LN", 3), "V0060");
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
    await scanTitleAndVolume(page, specIsbn("LN", 4), "V0063");

    // Resolve the volume id deterministically from the title detail page's
    // volume link, instead of brute-force scanning /volume/{1..N} (#22).
    const titleId = await titleIdFromSkeleton(page);
    const volumeId = await volumeIdByLabel(page, titleId, "V0063");

    // Set the volume condition to the damaged (non-loanable) state. Select by
    // the value of whichever option's label matches in either language rather
    // than hardcoding the French "Endommagé" string (#22). Reference-data
    // values aren't localized (NFR41), so the bilingual regex tolerates either
    // a French- or English-seeded state name without depending on the option
    // ordering or the UI locale.
    await page.goto(`/volume/${volumeId}/edit`);
    const conditionSelect = page.locator('select[name="condition_state_id"]');
    await expect(conditionSelect).toBeVisible({ timeout: 3000 });
    const damagedValue = await conditionSelect
      .locator("option")
      .filter({ hasText: /Damaged|Endommagé/i })
      .first()
      .getAttribute("value");
    expect(damagedValue).toBeTruthy();
    await conditionSelect.selectOption(damagedValue!);
    await page.locator('main button[type="submit"]').last().click();
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
    await scanTitleAndVolume(page, specIsbn("LN", 5), "V0061");
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
    await scanTitleAndVolume(page, specIsbn("LN", 6), "V0062");
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
    await scanTitleAndVolume(page, specIsbn("LN", 7), "V0090");
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

// Story 9-8 — Volume detail page loan-status row, role-aware (FR59).
// Anonymous sees "On loan since {date}" without borrower name;
// librarian sees "On loan to {borrower} since {date}" with a clickable
// link to /borrower/:id. AC8 LOAD-BEARING SECURITY contract: the
// borrower's name MUST NOT appear ANYWHERE in the anonymous render.
// Story 9-8 — per-test unique CHAR(5) V-code generator. Combines a
// process-local counter with `Date.now() % 100` so two parallel tests
// in the same wall-clock millisecond can't collide on the
// `volumes.label` UNIQUE constraint. Code-review fix vs the prior
// `Date.now() % 10000` formula which collided with sibling specs'
// hardcoded V-codes (V0042, V0070, V0099) under fullyParallel: true.
let vcodeCounter = 0;
function uniqueVcode(): string {
  vcodeCounter = (vcodeCounter + 1) % 100;
  const counter = vcodeCounter.toString().padStart(2, "0");
  const time = (Date.now() % 100).toString().padStart(2, "0");
  return `V${counter}${time}`;
}

test.describe("Volume detail — loan status role-aware (FR59)", () => {
  test("anonymous sees 'On loan' without borrower name; librarian sees borrower link", async ({
    browser,
    page,
  }) => {
    await loginAs(page);

    // Per-invocation unique suffix protects against Playwright retry-
    // induced collisions:
    // - borrowerName: a duplicate name would trigger strict-mode
    //   "resolved to N elements" in createBorrower's link assertion.
    // - volumeLabel: V-codes are CHAR(5) UNIQUE; uniqueVcode()
    //   combines a per-test counter + Date.now() % 100 (code-review
    //   fix — was Date.now() % 10000, collision-prone vs sibling specs'
    //   hardcoded V0042/V0070/V0099 under fullyParallel: true).
    const uniq = Date.now();
    const borrowerName = `LN-9-8 Alice Tremblay ${uniq}`;
    const volumeLabel = uniqueVcode();
    const ANON_ISBN = specIsbn("LN", 8);

    // Seed: title + volume + borrower + active loan. Capture the title id from
    // the scan skeleton up front — createBorrower/createLoan navigate away, so
    // the skeleton is gone by the time we need the volume id.
    await scanTitleAndVolume(page, ANON_ISBN, volumeLabel);
    const titleId = await titleIdFromSkeleton(page);
    await createBorrower(page, borrowerName);
    await createLoan(page, volumeLabel, borrowerName);

    // Resolve the volume id deterministically from the title detail page's
    // volume link instead of brute-force scanning /volume/{1..N} (#22). The
    // loans table renders V-codes as plain text, so the title detail page is
    // the stable source for the id.
    const volumeId = await volumeIdByLabel(page, titleId, volumeLabel);
    const volumeUrl = `/volume/${volumeId}`;

    // 1. Anonymous render via a FRESH browser context — defeats the
    //    HTMX GET-cache risk where a librarian-cached HTML response
    //    could satisfy the assertion (code-review fix vs the prior
    //    `clearCookies()` on the same context — cookies cleared but
    //    cache preserved).
    const anonContext = await browser.newContext();
    const anonPage = await anonContext.newPage();
    await anonPage.goto(volumeUrl);
    const anonContent = await anonPage.content();
    expect(anonContent).not.toContain(borrowerName);
    expect(anonContent).not.toContain("Alice Tremblay");
    expect(anonContent).not.toContain("/borrower/");
    // Anonymous still sees the badge text.
    await expect(anonPage.locator("body")).toContainText(/On loan since|En prêt depuis/i);
    await anonContext.close();

    // 2. Librarian render — borrower name + clickable link to /borrower/<id>.
    //    Reuses the still-logged-in original `page` from the seed phase.
    await page.goto(volumeUrl);
    await expect(page.locator("body")).toContainText(borrowerName);
    const borrowerLink = page.getByRole("link", { name: borrowerName });
    await expect(borrowerLink).toBeVisible();
    const borrowerHref = await borrowerLink.getAttribute("href");
    expect(borrowerHref).toMatch(/^\/borrower\/\d+$/);
    // Click the link → should land on /borrower/<id>.
    await borrowerLink.click();
    await page.waitForURL(/\/borrower\/\d+/);
  });
});
