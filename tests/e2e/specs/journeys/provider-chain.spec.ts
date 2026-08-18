import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

// ISBN known to BnF mock (primary provider)
const BNF_ISBN = "9782070360246";
// ISBN known only to Google Books mock (fallback provider — not in BnF)
const GOOGLE_BOOKS_ISBN = "9780134685991";
// ISBN unknown to all providers (tests all-fail scenario)
const UNKNOWN_ISBN = "9780000000002";

test.describe("Provider Chain & Fallback (Story 3-1)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // AC2, AC3: Fallback to Google Books when BnF returns no result
  test("scan ISBN unknown to BnF resolves metadata from Google Books fallback", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Scan ISBN that only Google Books knows
    await scanField.fill(GOOGLE_BOOKS_ISBN);
    await scanField.press("Enter");

    // Should see skeleton feedback (async fetch in progress)
    const anyFeedback = page.locator(
      "#feedback-list .feedback-skeleton, #feedback-list .feedback-entry"
    );
    await expect(anyFeedback.first()).toBeVisible({ timeout: 5000 });

    // Scanning again proves the title was created: the response says so
    // directly, so this assertion does not depend on the background fetch.
    await scanField.fill(GOOGLE_BOOKS_ISBN);
    await scanField.press("Enter");

    const infoEntry = page.locator(
      '.feedback-entry[data-feedback-variant="info"]'
    );
    await expect(infoEntry).toBeVisible({ timeout: 15000 });

    // The metadata itself is written by a background task, and nothing on
    // this page will fetch it once the second scan's response has landed:
    // the PendingUpdates middleware delivers a resolved row at most once, on
    // whatever HTMX request happens to follow, and there is no polling
    // anywhere in the templates. Waiting on `body` here would therefore be
    // waiting on a DOM that is finished changing — see #467.
    //
    // Re-query instead, until the fetch has landed. `toPass` retries the
    // whole navigation, so this stays deterministic without waitForTimeout
    // (which the CI grep gate rightly forbids).
    await expect(async () => {
      await page.goto(`/?q=${GOOGLE_BOOKS_ISBN}`);
      await expect(page.locator("#browse-results")).toContainText(
        /Effective Java|Bloch/i,
        { timeout: 2000 },
      );
    }).toPass({ timeout: 30000 });
  });

  // AC8: All providers fail — title exists with no metadata, no blocking error
  test("scan unknown ISBN creates title even when all providers fail", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Scan ISBN that no provider knows
    await scanField.fill(UNKNOWN_ISBN);
    await scanField.press("Enter");

    // Should see skeleton feedback (async fetch attempted)
    const anyFeedback = page.locator(
      "#feedback-list .feedback-skeleton, #feedback-list .feedback-entry"
    );
    await expect(anyFeedback.first()).toBeVisible({ timeout: 5000 });

    // Scan again to confirm title was created despite no metadata.
    // Bounded to 15s for all-providers-fail scenario under CI load.
    await scanField.fill(UNKNOWN_ISBN);
    await scanField.press("Enter");

    // Should see "already exists" info — title was created even without metadata
    const infoEntry = page.locator(
      '.feedback-entry[data-feedback-variant="info"]'
    );
    await expect(infoEntry).toBeVisible({ timeout: 15000 });

    // No error feedback should be present — chain failure is silent to user
    const errorEntries = page.locator(
      '.feedback-entry[data-feedback-variant="error"]'
    );
    await expect(errorEntries).toHaveCount(0);
  });

  // AC1, AC2: Primary provider (BnF) still works
  test("scan ISBN known to BnF resolves metadata from primary provider", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill(BNF_ISBN);
    await scanField.press("Enter");

    // Should see feedback
    const anyFeedback = page.locator(
      "#feedback-list .feedback-skeleton, #feedback-list .feedback-entry"
    );
    await expect(anyFeedback.first()).toBeVisible({ timeout: 5000 });

    // Context banner should appear
    const banner = page.locator("#context-banner");
    await expect(banner).not.toHaveClass(/hidden/, { timeout: 5000 });

    await scanField.fill(BNF_ISBN);
    await scanField.press("Enter");

    // #467 — this used to assert on `body` right here, and flaked. The
    // background fetch for this ISBN is longer than it looks: the BnF mock
    // answers with no UNIMARC zones, which triggers the #439 zone-completion
    // pass, which consults K10plus — gated in for the 978-2 prefix and paced
    // by a 1 req/s limiter shared across the whole process. Under parallel
    // load the fetch can finish AFTER the second scan's response, and since
    // a resolved row is delivered at most once, on some later HTMX request,
    // nothing would ever swap it in.
    //
    // Assert on the catalog instead, retrying the navigation until the fetch
    // has landed.
    await expect(async () => {
      await page.goto(`/?q=${BNF_ISBN}`);
      await expect(page.locator("#browse-results")).toContainText(
        /tranger|Camus/i,
        { timeout: 2000 },
      );
    }).toPass({ timeout: 30000 });
  });
});
