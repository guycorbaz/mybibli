import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

/**
 * CR #202 tier 2 — when a metadata re-download finds nothing, the failure
 * message now says WHY rather than only THAT it failed.
 *
 * The ISBN below is on the mock server's NO_METADATA_ISBNS blocklist, so every
 * provider in the chain answers empty and the handler reaches its failure
 * branch. That is the whole premise: without the blocklist entry the catch-all
 * would answer and this spec would be asserting on a success page.
 */
const NO_PROVIDER_ISBN = "9780000000033";

test.describe("Metadata failure diagnosis (CR #202 tier 2)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("a failed re-download names how many sources were searched", async ({
    page,
  }) => {
    // Create the title by scanning; the async fetch will find nothing.
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(NO_PROVIDER_ISBN);
    await page.locator("#scan-field").press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
      timeout: 10000,
    });

    await page.goto(`/?q=${NO_PROVIDER_ISBN}`);
    await page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first()
      .click();
    await expect(page.locator("h1")).toBeVisible({ timeout: 5000 });

    // Re-download runs the chain synchronously and fails.
    await page
      .getByRole("button", {
        name: /Re-download metadata|Re-télécharger les métadonnées|Metadaten erneut|Riscarica metadati/i,
      })
      .click();

    const feedback = page.locator("#title-feedback");
    await expect(feedback).toBeVisible({ timeout: 20000 });

    // The pre-#202 message: states that it failed, not why. Still expected —
    // the diagnosis is an addition, not a replacement.
    await expect(feedback).toContainText(
      /no metadata found|aucune métadonnée/i,
      { timeout: 20000 },
    );

    // The addition: a second line saying what was actually done. Asserting on
    // the count phrasing rather than an exact sentence keeps the test from
    // breaking on copy edits while still proving a diagnosis was rendered.
    await expect(feedback).toContainText(
      /sources were searched|sources ont été interrogées|Quellen wurden|fonti sono state/i,
      { timeout: 20000 },
    );
  });

  test("the diagnosis never leaks an HTTP status code to the user", async ({
    page,
  }) => {
    // docs/error-message-style.md forbids HTTP codes in user-facing copy. The
    // chain distinguishes 429 from 503 internally, so this is a live risk
    // every time the copy is touched, not a hypothetical one.
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(NO_PROVIDER_ISBN);
    await page.locator("#scan-field").press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
      timeout: 10000,
    });

    await page.goto(`/?q=${NO_PROVIDER_ISBN}`);
    await page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first()
      .click();
    await page
      .getByRole("button", {
        name: /Re-download metadata|Re-télécharger les métadonnées|Metadaten erneut|Riscarica metadati/i,
      })
      .click();

    const feedback = page.locator("#title-feedback");
    await expect(feedback).toBeVisible({ timeout: 20000 });
    const text = (await feedback.innerText()).trim();

    expect(text).not.toMatch(/\b(429|503|404|500)\b/);
    // Nor the internal enum names that the outcome type is built from.
    expect(text).not.toMatch(/NotConfigured|RateLimited|Unavailable|TimedOut/);
  });
});
