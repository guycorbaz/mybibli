import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn10 } from "../../helpers/isbn";

// Story 11-1 (#497) — an ISBN-10 scanned on /catalog is recognised as an
// ISBN, converted to its ISBN-13, and handled exactly like the ISBN-13.
// specId "IX" is unique to this spec (seq 2 and 16 have an `X` check digit).

const TITLE_EXISTS = /already in your catalog|déjà dans votre catalogue/i;

async function scan(page: Page, code: string) {
  const scanField = page.locator("#scan-field");
  await scanField.fill(code);
  await scanField.press("Enter");
}

function titleExistsEntries(page: Page) {
  return page
    .locator('#feedback-list .feedback-entry[data-feedback-variant="info"]')
    .filter({ hasText: TITLE_EXISTS });
}

/**
 * Scan a code that must be handled as an ISBN under `isbn13`: either the
 * skeleton of a new title carrying the ISBN-13, or — on a rerun against a
 * non-reset stack, where an earlier run already created it — one more
 * "already in your catalog" entry. Never the media-type selector a UPC
 * classification would show.
 */
async function scanExpectingIsbn(page: Page, code: string, isbn13: string) {
  const before = await titleExistsEntries(page).count();
  await scan(page, code);
  await expect(
    page
      .locator("#feedback-list .feedback-skeleton")
      .filter({ hasText: isbn13 })
      .or(titleExistsEntries(page).nth(before)),
  ).toBeVisible({ timeout: 10000 });
  await expect(page.locator('#feedback-list [role="group"]')).toHaveCount(0);
  await page.waitForLoadState("networkidle");
}

/** Scan a code whose title is already catalogued: one more info entry. */
async function scanExpectingExisting(page: Page, code: string) {
  const before = await titleExistsEntries(page).count();
  await scan(page, code);
  await expect(titleExistsEntries(page)).toHaveCount(before + 1, {
    timeout: 10000,
  });
}

test.describe("ISBN-10 at scan (#497)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("scanning an ISBN-10 creates the title under its ISBN-13", async ({
    page,
  }) => {
    const { isbn10, isbn13 } = specIsbn10("IX", 1);
    await page.goto("/catalog");

    await scanExpectingIsbn(page, isbn10, isbn13);

    // Stored under the ISBN-13: scanning the ISBN-13 finds it.
    await scanExpectingExisting(page, isbn13);
  });

  test("an ISBN-10 ending in X gets the same treatment", async ({ page }) => {
    const { isbn10, isbn13 } = specIsbn10("IX", 2);
    expect(isbn10.endsWith("X")).toBe(true);
    await page.goto("/catalog");

    await scanExpectingIsbn(page, isbn10, isbn13);

    await scanExpectingExisting(page, isbn13);
  });

  test("scanning the ISBN-10 of a title catalogued under its ISBN-13 returns it", async ({
    page,
  }) => {
    const { isbn10, isbn13 } = specIsbn10("IX", 3);
    await page.goto("/catalog");

    await scanExpectingIsbn(page, isbn13, isbn13);

    await scanExpectingExisting(page, isbn10);
  });

  test("a 10-digit code with a bad ISBN-10 checksum is not taken for an ISBN", async ({
    page,
  }) => {
    const { isbn10 } = specIsbn10("IX", 4);
    const last = parseInt(isbn10.slice(-1), 10);
    const badChecksum = isbn10.slice(0, 9) + ((last + 1) % 10).toString();
    await page.goto("/catalog");

    await scan(page, badChecksum);

    // Current classification applies: a UPC, so the media-type selector.
    const selector = page.locator('#feedback-list [role="group"]');
    await expect(selector).toBeVisible({ timeout: 10000 });
    await expect(page.locator("#feedback-list")).toContainText(
      `UPC ${badChecksum}`,
    );
  });

  test("a home-page search by ISBN-10 finds the title", async ({ page }) => {
    const { isbn10, isbn13 } = specIsbn10("IX", 5);
    await page.goto("/catalog");
    await scanExpectingIsbn(page, isbn10, isbn13);

    await page.goto(`/?q=${isbn10}`);
    const rows = page.locator(
      "#browse-results table.browse-table tbody tr td a[href^='/title/']",
    );
    await expect(rows).toHaveCount(1, { timeout: 10000 });
  });
});
