/**
 * #495 — the Library of Congress answers again, through SRU.
 *
 * Since 2026-10-26 `www.loc.gov/books/?fo=json` answers every server-side
 * request with a Cloudflare bot challenge (`403` + `cf-mitigated: challenge`).
 * Story 11-2 makes the SRU / MARCXML catalogue (`lx2.loc.gov`) the provider's
 * only source.
 *
 * Both ISBNs are served by the LoC SRU mock in
 * `tests/e2e/mock-metadata-server/server.py` and sit on the BnF mock's
 * blocklist, so the chain reaches LoC as a PRIMARY source (Google Books
 * misses unknown ISBNs):
 *
 * - `LOC_PRIMARY_ISBN` gets a full MARC 21 record — the title must carry the
 *   LoC title (decoded from the feed's `e&#x301;` references), author and
 *   publisher;
 * - `LOC_CHALLENGE_ISBN` gets the Cloudflare challenge — the chain must move
 *   on and resolve the title through Open Library.
 *
 * Fixed, mock-keyed ISBNs (the `catalog-marc-zones.spec.ts` convention); the
 * spec asserts on the title page, so it also passes against a stack where a
 * previous run already catalogued them.
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

const LOC_PRIMARY_ISBN = "9780449000038";
const LOC_CHALLENGE_ISBN = "9780449000045";

/** Scan on /catalog, then poll the title page until `expected` shows. */
async function scanAndOpenTitle(page: Page, isbn: string, expected: string) {
  await page.goto("/catalog");
  const scanField = page.locator("#scan-field");
  await scanField.fill(isbn);
  await scanField.press("Enter");
  await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
    timeout: 10000,
  });

  await page.goto(`/?q=${isbn}`);
  const titleLink = page
    .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
    .first();
  await expect(titleLink).toBeVisible({ timeout: 15000 });
  const href = (await titleLink.getAttribute("href"))!;

  // The metadata fetch is a background task: reload until it has landed.
  await expect(async () => {
    await page.goto(href);
    await expect(page.locator("main")).toContainText(expected);
  }).toPass({ timeout: 30000 });
}

test.describe("Library of Congress through SRU (#495)", () => {
  test("an ISBN only LoC holds is described from its MARC record", async ({
    page,
  }) => {
    await loginAs(page, "librarian");

    // Decoded and NFC-composed: the mock sends `L'e&#x301;conomie`.
    await scanAndOpenTitle(page, LOC_PRIMARY_ISBN, "L'économie de Congress");

    const main = page.locator("main");
    // 100$a "Primary, Sample," inverted and stripped of ISBD punctuation.
    await expect(main).toContainText("Sample Primary");
    // 264$b "Congress Press," without its trailing comma.
    await expect(main).toContainText("Congress Press");
    await expect(main).not.toContainText("Congress Press,");
    // No raw entity leaks to the page.
    await expect(main).not.toContainText("&#x301;");
  });

  test("a Cloudflare challenge from LoC hands over to the next provider", async ({
    page,
  }) => {
    await loginAs(page, "librarian");

    await scanAndOpenTitle(
      page,
      LOC_CHALLENGE_ISBN,
      "Open Library After Challenge",
    );
  });
});
