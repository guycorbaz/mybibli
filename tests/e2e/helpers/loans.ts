import { expect, Page, request } from "@playwright/test";

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
  // CR #300: many specs share an ISBN across multiple tests, so by the 2nd
  // test the title already has ≥1 volume and the V-code scan returns the
  // phantom-volume confirmation modal. The helper documents the "attach
  // this volume to this title" intent — Confirm and proceed. No-op when no
  // modal is open (typical first-test or unique-ISBN-per-test case).
  const phantomConfirm = page
    .locator("#modal-slot dialog[open]")
    .getByRole("button", {
      name: /Add another copy|Ajouter un exemplaire|Exemplar hinzufügen|Aggiungi una copia/i,
    });
  // Volume scan feedback locator: filter first, then assert visibility —
  // do not use `.first()` on the unfiltered list, which can lock onto a
  // stale ISBN scan entry rendered earlier on the page.
  const volumeFeedback = page
    .locator(".feedback-entry")
    .filter({ hasText: new RegExp(`\\b${escapeRegex(volumeLabel)}\\b`, "i") });
  // #458: `isVisible()` does NOT wait (its `timeout` option is deprecated
  // and ignored since Playwright 1.15), so polling it here raced the HTMX
  // round-trip — whether the modal was caught depended on server latency,
  // and a missed modal left the scan stuck behind an open dialog. Wait
  // deterministically for whichever surface the scan actually produces:
  // the confirmation modal, or the volume feedback entry directly.
  await expect(phantomConfirm.or(volumeFeedback.first()).first()).toBeVisible({
    timeout: 10000,
  });
  if (await phantomConfirm.isVisible()) {
    await phantomConfirm.click();
  }
  await expect(volumeFeedback.first()).toBeVisible({ timeout: 10000 });
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
  await page.locator('main button[type="submit"]').last().click();
  // Assertion-as-wait: the borrower anchor appearing in the list confirms
  // the server committed the INSERT. Scoped to `tbody` so the assertion
  // works correctly across viewports — story 10-3 added a parallel
  // mobile-card surface (`md:hidden`) that also carries the borrower
  // anchor. The `tbody` scope picks only the desktop-table copy, which
  // is the surface visible at default Playwright viewport (1280x720).
  const exactName = new RegExp(`^\\s*${escapeRegex(name)}\\s*$`);
  await expect(
    page.locator('tbody a[href^="/borrower/"]').filter({ hasText: exactName }),
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
  // Scoped to `tbody` (story 10-3): the borrowers list now renders the
  // same row in two surfaces (desktop table + mobile card list). Counting
  // matches across both would inflate the count by 2× and break the
  // ambiguity-detection assertion. Only the table has a `<tbody>`.
  const link = page
    .locator('tbody a[href^="/borrower/"]')
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
  // Story 8-2: the direct `page.request.post(...)` path does NOT fire
  // HTMX's `htmx:configRequest`, so `csrf.js` cannot inject the token.
  // Read the meta tag off the current /loans page and echo it via the
  // X-CSRF-Token header ourselves.
  const csrfToken = await page
    .locator('meta[name="csrf-token"]')
    .getAttribute("content");
  // maxRedirects: 0 so we observe the handler's 303 directly. Following the
  // 303 would re-GET /loans, and a 500 there would be reported as
  // "Failed to create loan …" even though the POST actually committed —
  // hiding the real failure mode.
  const response = await page.request.post("/loans", {
    form: {
      _csrf_token: csrfToken ?? "",
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
 * Story 9-11 — the Return button now opens a UX-DR8 confirmation modal
 * (`<dialog open aria-modal="true">` mounted into `#modal-slot`) instead
 * of firing a browser `confirm()` dialog. The flow is:
 *   1. Click the row's Return button → HTMX fetches the modal fragment
 *   2. Wait for `#modal-slot dialog[open]` to appear
 *   3. Click `[data-modal-confirm]` → POST `/loans/:id/return`
 *   4. Wait for the row to disappear from `#loans-table-body`
 *
 * Replaces the pre-9-11 `page.once("dialog", ...)` handler — the browser
 * confirm path is gone, so dialog interception is dead code now.
 */
export async function returnLoanFromLoansPage(
  page: Page,
  volumeLabel: string,
): Promise<void> {
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
  // Wait for the modal to mount — the `<dialog open>` selector matches the
  // scanner-guard 7-5 contract, so this assertion doubles as proof that the
  // modal entered the slot before we click Confirm.
  const modalDialog = page.locator("#modal-slot dialog[open]");
  await expect(modalDialog).toBeVisible({ timeout: 5000 });
  await modalDialog.locator("[data-modal-confirm]").click();
  // Wait for the row to disappear — POST /loans/:id/return commits, then
  // the modal closes and the feedback HTML lands in `#loan-feedback`.
  await expect(page.locator("#loans-table-body")).not.toContainText(
    volumeLabel,
    { timeout: 10000 },
  );
}

/**
 * Return a loan from the borrower detail page (the active-loans table).
 *
 * Story 9-11 — same modal flow as `returnLoanFromLoansPage`, but the row
 * lives in `#active-loans-section` and the feedback target after Confirm
 * is `#borrower-feedback`. Relocated from the inline spec helper into
 * this shared module per Foundation Rule #1 (DRY) — both helpers share
 * 90% of their body modulo the row container selector.
 */
export async function returnLoanFromBorrowerDetail(
  page: Page,
  volumeLabel: string,
): Promise<void> {
  const activeLoans = page.locator("#active-loans-section");
  // Story 10-3: the active-loans section now renders the Return button
  // in both surfaces (mobile card + desktop table). Scope to `tbody` so
  // the helper picks the visible desktop-table copy at the default
  // Playwright viewport (1280x720).
  const returnBtn = activeLoans
    .locator('tbody button:has-text("Return"), tbody button:has-text("Retourner")')
    .first();
  await expect(returnBtn).toBeVisible({ timeout: 3000 });
  await returnBtn.click();
  const modalDialog = page.locator("#modal-slot dialog[open]");
  await expect(modalDialog).toBeVisible({ timeout: 5000 });
  await modalDialog.locator("[data-modal-confirm]").click();
  await expect(activeLoans).not.toContainText(volumeLabel, {
    timeout: 10000,
  });
}

/**
 * Seed an overdue loan via the TEST_MODE `/debug/seed-overdue-loan` endpoint.
 *
 * Mirror of the `setSessionTimeoutSecs` helper in session-inactivity-timeout.spec.ts:
 * opens a private `APIRequestContext`, logs in as admin (the route is admin-only
 * defense-in-depth), grabs the post-login CSRF token off `/catalog`'s meta tag
 * (login rotates the token per story 8-2), and posts the form. The caller's
 * `Page` and its cookie/role state are untouched — useful when the test wants
 * to assert what `librarian` sees after `admin` seeded backdated history.
 *
 * Resolves `volumeLabel` and `borrowerName` to ids server-side (the endpoint
 * does the lookup) so callers can reuse the names they already passed to
 * `scanTitleAndVolume` + `createBorrower`, no extra UI scraping for ids.
 *
 * Throws on auth failure, missing entity, or non-2xx response — failing loud
 * here is better than producing a silent green when the seed didn't land.
 *
 * @param baseURL  passed via `testInfo.project.use.baseURL` or `request`'s baseURL
 * @param opts.volumeLabel   V-code that was passed to `scanTitleAndVolume`
 * @param opts.borrowerName  exact name that was passed to `createBorrower`
 * @param opts.daysOverdue   `loaned_at = NOW() - INTERVAL ? DAY`; pick > the
 *                           default 30-day overdue threshold (e.g. 60)
 */
export async function seedOverdueLoan(
  baseURL: string,
  opts: {
    volumeLabel: string;
    borrowerName: string;
    daysOverdue: number;
  },
): Promise<void> {
  const ctx = await request.newContext({ baseURL });
  try {
    const loginHtml = await (await ctx.get("/login")).text();
    const csrfMatch = /name="_csrf_token"\s+value="([^"]+)"/.exec(loginHtml);
    if (!csrfMatch) {
      throw new Error("seedOverdueLoan: _csrf_token not found on /login");
    }
    const csrf = csrfMatch[1]!;
    const username = process.env.TEST_ADMIN_USERNAME ?? "admin";
    const password = process.env.TEST_ADMIN_PASSWORD ?? "admin";
    const loginResp = await ctx.post("/login", {
      form: { _csrf_token: csrf, username, password },
      maxRedirects: 0,
      failOnStatusCode: false,
    });
    if (loginResp.status() !== 303) {
      throw new Error(
        `seedOverdueLoan: admin login failed (status ${loginResp.status()})`,
      );
    }
    // Story 8-2 — login rotates the CSRF token. Read the new one off any
    // authenticated page's <meta name="csrf-token">.
    const pageHtml = await (await ctx.get("/catalog")).text();
    const csrf2Match = /<meta name="csrf-token" content="([^"]+)"/.exec(pageHtml);
    if (!csrf2Match) {
      throw new Error("seedOverdueLoan: post-login csrf-token meta not found");
    }
    const csrf2 = csrf2Match[1]!;
    const res = await ctx.post("/debug/seed-overdue-loan", {
      form: {
        _csrf_token: csrf2,
        volume_label: opts.volumeLabel,
        borrower_name: opts.borrowerName,
        days_overdue: String(opts.daysOverdue),
      },
    });
    if (!res.ok()) {
      const body = await res.text();
      throw new Error(
        `seedOverdueLoan failed (status ${res.status()}): ${body.slice(0, 300)}. ` +
          `Ensure the app is started with TEST_MODE=1.`,
      );
    }
  } finally {
    await ctx.dispose();
  }
}
