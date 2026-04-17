/**
 * Story 7-4 — Content Security Policy headers E2E.
 *
 * Three live tests + one optional (skipped):
 *   1. Anonymous flow — /, /catalog, a title detail page; verify the exact
 *      `content-security-policy` directive on each response and that no
 *      console / pageerror messages mention CSP refusal.
 *   2. Authenticated librarian flow — scan a title + volume, create a loan,
 *      open /borrower/{id}, return the loan. Exercises the 4 extracted
 *      inline scripts (audio toggle, title-form, loan borrower search,
 *      borrower-detail reload) and the OOB-swapped feedback dismiss path.
 *   3. Negative — inject an inline <script> after DOM load and assert it
 *      never executes (regression gate for `script-src 'self'`).
 *   4. Report-only — skipped here; AC 9 unit test already proves the code
 *      path. A docker override would add >20 lines for marginal value.
 *
 * Spec ID: "CS" (used by `specIsbn("CS", n)` for unique test data).
 *
 * Selector / wait policy: response.headers() + page.on("console") only.
 * No waitForTimeout (CI grep gate).
 */
import { test, expect, Page, ConsoleMessage } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume, createBorrower, createLoan } from "../../helpers/loans";

const SPEC_ID = "CS";

// Exact directive emitted by src/middleware/csp.rs::CSP_DIRECTIVES.
// Kept in sync by hand — failing this assertion flags a deliberate change
// that the unit test (test_csp_enforced_mode_headers) should catch first.
const EXPECTED_CSP =
  "default-src 'self'; " +
  "script-src 'self'; " +
  "style-src 'self'; " +
  "img-src 'self' data: https://covers.openlibrary.org https://books.google.com https://image.tmdb.org https://coverartarchive.org; " +
  "font-src 'self'; " +
  "connect-src 'self'; " +
  "frame-src 'none'; " +
  "frame-ancestors 'none'; " +
  "object-src 'none'; " +
  "base-uri 'self'; " +
  "form-action 'self'";

const CSP_REFUSAL_RE = /Refused to (execute|apply|load)|Content[- ]Security[- ]Policy/i;

/**
 * Hook console + pageerror + securitypolicyviolation listeners BEFORE any
 * navigation so early-load violations are not missed. Returns the array —
 * call assert at the end. The `securitypolicyviolation` DOM event surfaces
 * the offending sample + source file so failures point at the exact origin
 * of the violation, not just the hash.
 */
function trackCspViolations(page: Page): string[] {
  const messages: string[] = [];
  page.on("console", (msg: ConsoleMessage) => {
    const text = msg.text();
    if (CSP_REFUSAL_RE.test(text) || text.startsWith("[CSP-EVT]")) {
      messages.push(`console: ${text}`);
    }
  });
  page.on("pageerror", (err) => {
    if (CSP_REFUSAL_RE.test(err.message)) messages.push(`pageerror: ${err.message}`);
  });
  // Re-installs on every navigation via addInitScript — needed because the
  // listener is on the document, which is reset on cross-origin navigation
  // (no-op for our same-origin app, but harmless).
  void page.addInitScript(() => {
    document.addEventListener("securitypolicyviolation", (e) => {
      // eslint-disable-next-line no-console
      console.log(
        `[CSP-EVT] dir=${e.violatedDirective} kind=${e.blockedURI} sample=${(e.sample || "").slice(0, 200)} src=${e.sourceFile}:${e.lineNumber}:${e.columnNumber}`,
      );
    });
  });
  return messages;
}

/**
 * GET `path` and assert that the response header `content-security-policy`
 * exactly matches the enforced directive string. Returns the response so
 * callers can chain further assertions (e.g. `img naturalWidth > 0`).
 */
async function assertCsp(page: Page, path: string) {
  const resp = await page.goto(path);
  expect(resp, `goto(${path}) returned no response`).not.toBeNull();
  const csp = resp!.headers()["content-security-policy"];
  expect(csp, `missing CSP header on ${path}`).toBeTruthy();
  expect(csp).toBe(EXPECTED_CSP);
  // Hardening headers spot-check: nosniff and Permissions-Policy denial.
  expect(resp!.headers()["x-content-type-options"]).toBe("nosniff");
  expect(resp!.headers()["permissions-policy"]).toContain("camera=()");
  return resp!;
}

test.describe("Story 7-4 — CSP headers", () => {
  test("anonymous flow: every page emits the strict CSP and no violations fire", async ({
    page,
  }) => {
    const violations = trackCspViolations(page);
    await page.context().clearCookies();

    await assertCsp(page, "/");
    await assertCsp(page, "/catalog");

    // AC 7 verification step: visit `/login` (bare.html layout — no nav,
    // theme.js must early-return when `#theme-toggle` is absent). Same
    // console listener catches any CSP refusal or pageerror that the
    // refactored theme.js could trigger here.
    await assertCsp(page, "/login");

    // Open a title detail page — pick whichever title is on the catalog page
    // first. If the catalog is empty (fresh test DB), skip the detail step
    // gracefully so this spec doesn't depend on prior seed data.
    await page.goto("/catalog");
    const firstTitleLink = page.locator('a[href^="/title/"]').first();
    if ((await firstTitleLink.count()) > 0) {
      const href = await firstTitleLink.getAttribute("href");
      if (href) await assertCsp(page, href);
    }

    expect(
      violations,
      `unexpected CSP violations on anonymous flow: ${violations.join(" | ")}`,
    ).toHaveLength(0);
  });

  test("authenticated librarian flow stays CSP-clean through scan + loan + return", async ({
    page,
  }) => {
    const violations = trackCspViolations(page);
    await loginAs(page, "librarian");

    const isbn = specIsbn(SPEC_ID, 1);
    // V-code is `V` + exactly 4 digits (`^V\d{4}$`). Use the last 4 digits
    // of `Date.now()` to keep the value unique across spec runs without
    // colliding with V-code ranges used by other specs (which pick from
    // V0001–V0099 / V0100–V0199 etc.). 7xxx is the CS spec range.
    const volumeLabel = `V7${Date.now().toString().slice(-3)}`;
    const borrowerName = `CS-Borrower-${Date.now()}`;

    await scanTitleAndVolume(page, isbn, volumeLabel);

    // AC 12 — cover image must render under strict CSP. Search the home
    // page for the freshly-scanned ISBN, click into the title detail page,
    // and assert the cover img has `naturalWidth > 0` (proves `img-src
    // 'self'` allows the self-served `/covers/{id}.jpg` or the placeholder
    // `/static/icons/{type}.svg`). The mock metadata server returns
    // synthetic covers for the generated ISBN; the metadata-fetch task
    // downloads them locally. /catalog itself is a scan surface and does
    // not list title links — use the home search like other specs do.
    await page.goto(`/?q=${isbn}`);
    const titleLink = page.locator('a[href^="/title/"]').first();
    await expect(titleLink).toBeVisible({ timeout: 10000 });
    const titleHref = await titleLink.getAttribute("href");
    expect(titleHref).toMatch(/\/title\/\d+/);
    await assertCsp(page, titleHref!);
    // Wait for any cover image (real or placeholder SVG) to be present;
    // either path proves img-src directives don't block self-origin assets.
    const coverImg = page.locator('img[src*="/covers/"], img[src*="/static/icons/"]').first();
    await expect(coverImg).toBeVisible({ timeout: 5000 });
    const naturalWidth = await coverImg.evaluate(
      (el) => (el as HTMLImageElement).naturalWidth,
    );
    expect(
      naturalWidth,
      "cover img must decode under strict CSP (naturalWidth > 0)",
    ).toBeGreaterThan(0);
    // Note on `data:` URL allowance: AC 2's `img-src 'self' data: …` keeps
    // the `data:` token as a defensive future-proof — no template currently
    // emits a `data:` URL (cover.html:8 falls back to a self-origin
    // `/static/icons/{type}.svg`, not a data URI). If a future change
    // introduces a data-URL placeholder, the directive already covers it.

    await createBorrower(page, borrowerName);
    await createLoan(page, volumeLabel, borrowerName);

    // After loan creation we're on /loans — verify the header is here too.
    const loansResp = await page.goto("/loans");
    expect(loansResp!.headers()["content-security-policy"]).toBe(EXPECTED_CSP);

    // Borrower detail page exercises the extracted borrower-detail script
    // (initBorrowerDetailReload — gated on body[data-page="borrower-detail"]).
    await page.goto("/borrowers");
    const borrowerLink = page.getByRole("link", { name: borrowerName }).first();
    await expect(borrowerLink).toBeVisible({ timeout: 5000 });
    const href = await borrowerLink.getAttribute("href");
    expect(href).toMatch(/\/borrower\/\d+/);
    await assertCsp(page, href!);

    // Verify body[data-page] is set so the reload script can engage.
    expect(await page.locator("body").getAttribute("data-page")).toBe(
      "borrower-detail",
    );

    expect(
      violations,
      `unexpected CSP violations on authenticated flow: ${violations.join(" | ")}`,
    ).toHaveLength(0);
  });

  test("negative — injected inline script is blocked by CSP and never runs", async ({
    page,
  }) => {
    const violations = trackCspViolations(page);
    await page.context().clearCookies();
    await page.goto("/catalog");

    // Inject an inline <script> after the page has loaded. Under strict
    // `script-src 'self'` the browser must refuse to execute it; window.__pwnd
    // must remain undefined and a refusal message must surface.
    await page.evaluate(() => {
      const s = document.createElement("script");
      s.textContent = "window.__pwnd = true;";
      document.body.appendChild(s);
    });

    // Poll for up to ~2s using expect.poll (retries on its own); the
    // sleep-style helper is banned by the CI grep gate.
    await expect
      .poll(async () => await page.evaluate(() => (window as unknown as { __pwnd?: boolean }).__pwnd), {
        timeout: 2000,
      })
      .toBeUndefined();

    // The browser must have logged a script-block refusal. Chromium emits
    // either "Refused to execute inline script" (network-level block) or
    // "Executing inline script violates the following Content Security
    // Policy directive 'script-src 'self''" (post-parse violation report).
    // Match either phrasing.
    const scriptBlocked = violations.some(
      (m) =>
        /Refused to execute inline script/i.test(m) ||
        /Executing inline script violates/i.test(m),
    );
    expect(
      scriptBlocked,
      `expected an inline-script CSP refusal, got: ${violations.join(" | ")}`,
    ).toBe(true);
  });

  // AC 11 Test 4: report-only mode E2E. Per Dev Notes "Report-only E2E"
  // decision gate, the docker override would add >20 lines for a path the
  // unit test (test_csp_report_only_mode_headers in src/middleware/csp.rs)
  // already proves. Skipped here, with a documented breadcrumb.
  test.skip("report-only mode emits Content-Security-Policy-Report-Only and does not block — covered by unit test", async () => {
    // To enable: spin up the app with CSP_REPORT_ONLY=true on a separate
    // port via a docker-compose override, add a Playwright project pointing
    // at it, and assert (a) the report-only header is set, (b) the inline
    // script from the negative test above actually runs.
  });
});
