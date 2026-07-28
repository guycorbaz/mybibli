/**
 * #439 — MARC 21 zones from the Library of Congress complete what the BnF
 * could not supply.
 *
 * The provider chain stops at the first provider that answers. In the test
 * stack the BnF mock answers for any ISBN and supplies a statement of
 * responsibility (UNIMARC 200$f) but no edition statement and no general note.
 * Before #439 those two zones stayed empty forever, which is exactly the
 * production symptom: the v1.13.0 backfill left anglophone titles at zero
 * zones because the providers that answer for them carry no structured
 * bibliographic data.
 *
 * The zone-completion pass must therefore reach past the winning provider to
 * the Library of Congress and fill only the empty zones — without disturbing
 * the ones already set.
 *
 * The ISBN is served by the LoC mocks in
 * `tests/e2e/mock-metadata-server/server.py` (`LOC_MARC_ISBN`), which stub the
 * flat JSON search and the SRU catalogue on separate paths.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

const ISBN = "9780449000014";

test.describe("MARC 21 zone completion (#439)", () => {
  test("a title resolved by another provider still gains LoC edition and note", async ({
    page,
  }) => {
    await loginAs(page, "librarian");

    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
      timeout: 10000,
    });

    // The metadata fetch is a background task, so poll the title page rather
    // than the scan feedback: the zones land after the chain completes.
    await page.goto(`/?q=${ISBN}`);
    const titleLink = page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first();
    await expect(titleLink).toBeVisible({ timeout: 15000 });
    const href = (await titleLink.getAttribute("href"))!;

    // Zone completion runs inside the async fetch; reload until the page
    // shows it rather than sleeping on a guessed duration.
    await expect(async () => {
      await page.goto(href);
      await expect(page.locator("main")).toContainText(
        "Congress Third edition.",
      );
    }).toPass({ timeout: 30000 });

    const main = page.locator("main");
    await expect(main).toContainText("Congress general note.");

    // And the zone the winning provider DID supply must survive untouched —
    // first source wins, the completion pass never overwrites.
    await expect(main).toContainText(/Statement of responsibility|Mention de responsabilité/i);
  });
});
