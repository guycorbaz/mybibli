import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

// Story 13-1 — scan duplicates and an existing label (#574, #585).
// (a) a scanner double read of a label is ignored client-side: one volume,
//     no error;
// (b) the ISBN + label pair scanned again → info "already recorded";
// (c) a label scanned under another title → warning naming its owner, with
//     a link to the volume page. Nothing is written in (b) or (c).

const INFO = '.feedback-entry[data-feedback-variant="info"]';
const WARNING = '.feedback-entry[data-feedback-variant="warning"]';
const ERROR = '.feedback-entry[data-feedback-variant="error"]';
const SUCCESS = '.feedback-entry[data-feedback-variant="success"]';
const TITLE_EXISTS = /already in your catalog|déjà dans votre catalogue/i;

async function scan(page: Page, code: string) {
  const field = page.locator("#scan-field");
  await field.fill(code);
  await field.press("Enter");
}

/** Scan an ISBN and wait until its title is the one in context. */
async function scanIsbn(page: Page, isbn: string) {
  await scan(page, isbn);
  await expect(page.locator("#context-banner")).toContainText(isbn, {
    timeout: 10000,
  });
}

/** Scan a new label and wait for its "volume created" entry. */
async function scanNewLabel(page: Page, label: string) {
  await scan(page, label);
  await expect(
    page.locator(SUCCESS).filter({ hasText: new RegExp(`\\b${label}\\b`) }),
  ).toBeVisible({ timeout: 10000 });
}

test.describe("Scan duplicates and an existing label (#574, #585)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page, "librarian");
    await page.goto("/catalog");
  });

  test("a label read twice by the scanner creates one volume and no error", async ({
    page,
  }) => {
    const isbn = specIsbn("DL", 1);
    await scanIsbn(page, isbn);

    // Two submits of the same label, back to back (well under 200 ms).
    const field = page.locator("#scan-field");
    await field.fill("V0590");
    await field.press("Enter");
    await field.fill("V0590");
    await field.press("Enter");

    await expect(
      page.locator(SUCCESS).filter({ hasText: /\bV0590\b/ }),
    ).toBeVisible({ timeout: 10000 });

    // A later, different scan: once its answer is in, any second V0590
    // post would have been answered too.
    await scan(page, isbn);
    await expect(page.locator(INFO).filter({ hasText: TITLE_EXISTS })).toBeVisible({
      timeout: 10000,
    });

    await expect(
      page.locator(".feedback-entry").filter({ hasText: /\bV0590\b/ }),
    ).toHaveCount(1);
    await expect(page.locator(ERROR)).toHaveCount(0);
    await expect(page.locator("#context-banner")).toContainText(/\b1 vol/i);
  });

  test("the ISBN and label pair scanned again says the label is already recorded", async ({
    page,
  }) => {
    const isbn = specIsbn("DL", 2);
    await scanIsbn(page, isbn);
    await scanNewLabel(page, "V0591");

    // The pair again, as a librarian re-scanning a book of the pile.
    await scan(page, isbn);
    await expect(page.locator(INFO).filter({ hasText: TITLE_EXISTS })).toBeVisible({
      timeout: 10000,
    });
    await scan(page, "V0591");

    await expect(
      page.locator(INFO).filter({ hasText: /\bV0591\b/ }),
    ).toContainText(/already recorded|déjà enregistré/i, { timeout: 10000 });
    await expect(page.locator(ERROR)).toHaveCount(0);
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);
  });

  test("a label scanned under another title warns and links to the volume", async ({
    page,
  }) => {
    const ownerIsbn = specIsbn("DL", 3);
    await scanIsbn(page, ownerIsbn);
    await scanNewLabel(page, "V0592");

    // Another book in context, then the first book's label.
    await scanIsbn(page, specIsbn("DL", 4));
    await scan(page, "V0592");

    const warning = page.locator(WARNING).filter({ hasText: /\bV0592\b/ });
    await expect(warning).toBeVisible({ timeout: 10000 });
    await expect(warning).toContainText(ownerIsbn);
    await expect(page.locator(ERROR)).toHaveCount(0);
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);

    const link = warning.locator('a[href^="/volume/"]');
    await expect(link).toHaveCount(1);
    await link.click();
    await expect(page).toHaveURL(/\/volume\/\d+$/);
    await expect(page.locator("main")).toContainText("V0592");
  });
});
