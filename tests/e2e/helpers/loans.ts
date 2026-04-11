import { expect, Page } from "@playwright/test";

/**
 * Loan-related E2E helpers — canonical patterns for Epic 4 specs.
 *
 * CRITICAL: Never use `page.waitForTimeout` in these helpers. Every wait must
 * be tied to an observable DOM state (row appears, row disappears, feedback
 * text matches). See CLAUDE.md → E2E Test Patterns → HTMX wait strategies.
 *
 * Root cause (story 5-1c, 2026-04-10):
 *   Previous loan helpers ended with `await page.waitForURL(/\/loans/)` after
 *   submitting the new-loan form. Because the form is rendered inline on the
 *   `/loans` page (HTMX), the URL already matched, so `waitForURL` resolved
 *   IMMEDIATELY — before the server committed the loan. Subsequent assertions
 *   then flaked under parallel load. The fix is to wait for the loan row to
 *   appear in `#loans-table-body` (assertion-as-wait), which only happens
 *   after a full page render post-commit.
 */

/**
 * Escape regex metacharacters in a user-supplied string so it can be
 * interpolated into a `new RegExp(...)` pattern as a literal. Used for
 * volume labels and borrower names passed to Playwright `filter({ hasText })`.
 */
function escapeRegex(input: string): string {
  return input.replace(/[-/\\^$*+?.()|[\]{}]/g, "\\$&");
}

/**
 * Create a title + volume via catalog scan.
 *
 * Requires the page to be logged in. Navigates to /catalog, scans the ISBN,
 * scans the volume label, and waits for both feedback entries to confirm the
 * operations succeeded.
 */
export async function scanTitleAndVolume(
  page: Page,
  isbn: string,
  volumeLabel: string,
): Promise<void> {
  await page.goto("/catalog");
  const scanField = page.locator("#scan-field");
  await scanField.fill(isbn);
  await scanField.press("Enter");
  // ISBN scan creates the title; we only need the feedback skeleton to appear,
  // not the metadata resolution to complete.
  await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
    timeout: 10000,
  });
  await scanField.fill(volumeLabel);
  await scanField.press("Enter");
  // Volume scan is synchronous; wait for a feedback entry containing the
  // exact volume label. Filter first, then assert visibility — do not use
  // `.first()` on the unfiltered list, which can lock onto a stale ISBN
  // scan entry rendered earlier on the page.
  const volumeFeedback = page
    .locator(".feedback-entry")
    .filter({ hasText: new RegExp(`\\b${escapeRegex(volumeLabel)}\\b`, "i") });
  await expect(volumeFeedback.first()).toBeVisible({ timeout: 5000 });
}

/**
 * Create a borrower via /borrowers form.
 *
 * Navigates to /borrowers, clicks the Add button, fills the form, submits, and
 * waits for the new borrower row to appear in the table. Waiting for the row
 * (not just the URL) proves the server committed the insert.
 */
export async function createBorrower(page: Page, name: string): Promise<void> {
  await page.goto("/borrowers");
  await page.getByText(/Add borrower|Ajouter/i).click();
  await page.locator("#new-name").fill(name);
  await page.locator('button[type="submit"]').last().click();
  // Assertion-as-wait: the borrower anchor appearing in the list confirms
  // the server committed the INSERT. Scoped to the actual `/borrower/:id`
  // anchor so stale DOM (form input echo, nav, breadcrumbs) cannot trip
  // the wait before the row is visible.
  const exactName = new RegExp(`^\\s*${escapeRegex(name)}\\s*$`);
  await expect(
    page.locator('a[href^="/borrower/"]').filter({ hasText: exactName }),
  ).toBeVisible({ timeout: 5000 });
}

/**
 * Look up a borrower id by exact name from the /borrowers page.
 *
 * Uses the borrower list on /borrowers which renders an `<a href="/borrower/{id}">`
 * per active borrower. Finds the anchor whose text matches the name exactly.
 */
async function getBorrowerIdByName(page: Page, name: string): Promise<string> {
  await page.goto("/borrowers");
  const link = page
    .locator('a[href^="/borrower/"]')
    .filter({ hasText: new RegExp(`^\\s*${escapeRegex(name)}\\s*$`) });
  // Fail loud on ambiguity: if two borrowers share the exact name, .first()
  // would silently pick the wrong id and the loan would be created against
  // an unrelated borrower, producing misleading assertion failures later.
  await expect(link).toHaveCount(1, { timeout: 5000 });
  const href = await link.getAttribute("href");
  if (!href) {
    throw new Error(`Borrower not found by name: ${name}`);
  }
  const id = href.split("/").pop();
  if (!id) {
    throw new Error(`Could not parse borrower id from href: ${href}`);
  }
  return id;
}

/**
 * Register a loan for an existing volume + borrower.
 *
 * The caller must have already created the title, volume, and borrower. This
 * helper submits the loan via a direct `POST /loans` request instead of the
 * inline HTMX form, because the HTMX form flow proved racy under parallel
 * worker load (see story 5-1c root-cause analysis): the `#loan-feedback`
 * innerHTML swap sometimes did not arrive within the test's polling window,
 * causing `toContainText` to time out on an empty feedback div.
 *
 * Direct POST gives:
 *   - Deterministic commit-before-return semantics (Playwright awaits the
 *     response, which only arrives after the server finishes the insert)
 *   - No dependency on HTMX client-side interception or DOM mutations
 *   - No dependency on the form UI visibility / dropdown population
 *
 * After the POST, navigate to /loans to verify the row is visible via a fresh
 * server-side render (not HTMX). This guarantees read-your-writes across the
 * create → list round-trip, which is the whole point of story 5-1c.
 */
export async function createLoan(
  page: Page,
  volumeLabel: string,
  borrowerName: string,
): Promise<void> {
  const borrowerId = await getBorrowerIdByName(page, borrowerName);
  // maxRedirects: 0 so we observe the handler's 303 directly. Following the
  // 303 would re-GET /loans, and a 500 there would be reported as
  // "Failed to create loan …" even though the POST actually committed —
  // hiding the real failure mode.
  const response = await page.request.post("/loans", {
    form: {
      volume_label: volumeLabel,
      borrower_id: borrowerId,
    },
    maxRedirects: 0,
  });
  if (response.status() !== 303) {
    const body = await response.text();
    throw new Error(
      `Failed to create loan for ${volumeLabel} / ${borrowerName}: expected 303 redirect, got ${response.status()} — ${body.slice(0, 300)}`,
    );
  }
  // Re-fetch /loans to confirm the row is visible in the table. The GET runs
  // a fresh DB query, guaranteeing the INSERT committed before assertion.
  await page.goto("/loans");
  await expect(page.locator("#loans-table-body")).toContainText(volumeLabel, {
    timeout: 5000,
  });
}

/**
 * Return a loan from the /loans page.
 *
 * Registers the dialog handler BEFORE clicking (dialog handler registration is
 * async in Playwright), clicks the Return button in the row matching the
 * volume label, and waits for the row to disappear from `#loans-table-body`.
 */
export async function returnLoanFromLoansPage(
  page: Page,
  volumeLabel: string,
): Promise<void> {
  // Must register BEFORE the click — registration is async in Playwright.
  page.once("dialog", (dialog) => {
    dialog.accept().catch(() => {});
  });
  // Word-boundary match so `V0070` does not match rows containing `V00701`,
  // `V00702`, etc. Substring semantics in Playwright's `hasText: string`
  // option have bitten per-spec V-code conventions in the past. Case-
  // insensitive for parity with `scanTitleAndVolume`.
  const loanRow = page
    .locator("#loans-table-body tr")
    .filter({
      hasText: new RegExp(`\\b${escapeRegex(volumeLabel)}\\b`, "i"),
    });
  const returnBtn = loanRow
    .locator('button:has-text("Return"), button:has-text("Retourner")')
    .first();
  await expect(returnBtn).toBeVisible({ timeout: 3000 });
  await returnBtn.click();
  // Wait for the row to disappear — HTMX swaps #loans-table-body on success
  await expect(page.locator("#loans-table-body")).not.toContainText(
    volumeLabel,
    { timeout: 10000 },
  );
}
