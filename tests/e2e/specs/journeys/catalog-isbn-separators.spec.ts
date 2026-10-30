import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn, specIsbn10 } from "../../helpers/isbn";

// Story 12-1 (#502) — an ISBN typed as printed on the book, with hyphens or
// spaces, is recognised on /catalog and in the home search, and handled
// under its bare ISBN-13. specId "IH" is unique to this spec.
// Same rerunnable idiom as catalog-isbn10.spec.ts (story 11-1).

const TITLE_EXISTS = /already in your catalog|déjà dans votre catalogue/i;

/** `9782205001211` → `978-2-205-00121-1`. */
function hyphenated13(isbn13: string): string {
  return [
    isbn13.slice(0, 3),
    isbn13.slice(3, 4),
    isbn13.slice(4, 7),
    isbn13.slice(7, 12),
    isbn13.slice(12),
  ].join("-");
}

/** `2205001213` → `2-205-00121-3`. */
function hyphenated10(isbn10: string): string {
  return [isbn10.slice(0, 1), isbn10.slice(1, 4), isbn10.slice(4, 9), isbn10.slice(9)].join("-");
}

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
 * Scan a code that must be handled as the ISBN `isbn13`: the skeleton of a
 * new title carrying the bare ISBN-13, or — on a rerun against a non-reset
 * stack — one more "already in your catalog" entry. Never the media-type
 * selector, never the unsupported-code warning.
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
  await expect(
    page.locator('#feedback-list .feedback-entry[data-feedback-variant="warning"]'),
  ).toHaveCount(0);
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

test.describe("ISBN typed with separators (#502)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("a hyphenated ISBN-13 creates the title under its bare ISBN-13", async ({
    page,
  }) => {
    const isbn13 = specIsbn("IH", 1);
    await page.goto("/catalog");

    await scanExpectingIsbn(page, hyphenated13(isbn13), isbn13);

    // Stored bare: scanning the bare ISBN-13 finds it.
    await scanExpectingExisting(page, isbn13);
  });

  test("an ISBN-13 typed with spaces returns the existing title", async ({
    page,
  }) => {
    const isbn13 = specIsbn("IH", 2);
    await page.goto("/catalog");
    await scanExpectingIsbn(page, isbn13, isbn13);

    await scanExpectingExisting(page, hyphenated13(isbn13).replaceAll("-", " "));
  });

  test("a hyphenated ISBN-10 is recognised and carried as its ISBN-13", async ({
    page,
  }) => {
    const { isbn10, isbn13 } = specIsbn10("IH", 3);
    await page.goto("/catalog");

    await scanExpectingIsbn(page, hyphenated10(isbn10), isbn13);

    await scanExpectingExisting(page, isbn13);
  });

  test("a home search by the hyphenated ISBN finds the title", async ({ page }) => {
    const isbn13 = specIsbn("IH", 4);
    await page.goto("/catalog");
    await scanExpectingIsbn(page, isbn13, isbn13);

    await page.goto(`/?q=${encodeURIComponent(hyphenated13(isbn13))}`);
    const rows = page.locator(
      "#browse-results table.browse-table tbody tr td a[href^='/title/']",
    );
    await expect(rows).toHaveCount(1, { timeout: 10000 });
  });
});
