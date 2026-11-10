/**
 * Story 17-3 E2E (#565, FR174) — one ISBN rule for every form.
 *
 * Real journey from a blank browser: loginAs(librarian) → /catalog → New
 * title → an ISBN typed by hand with hyphens (or an `ISBN` prefix and an
 * ISBN-10) → the bare ISBN-13 scanned afterwards reaches that title instead
 * of creating a duplicate. An ISBN with a wrong check digit is refused with
 * the field hint and nothing is created.
 *
 * Data: ISBNs from `specIsbn("IR", clock-based seq)`, the title names carry the
 * spec id; assertions only on this spec's rows (Rule #23).
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn, specIsbn10 } from "../../helpers/isbn";
import { simulateScan } from "../../helpers/scanner";

// Review round 1: manual creation refuses an ISBN another title holds, so
// a rerun on a database that was not reset must not draw a used ISBN. The
// sequence follows the clock (one value per 10 ms, cycling every ~5 min,
// far longer than a run) and each test takes its own slot.
function seq(slot: number): number {
  return 1000 + (Math.floor(Date.now() / 10) % 32000) * 3 + slot;
}

function hyphenated(isbn13: string): string {
  return `${isbn13.slice(0, 3)}-${isbn13.slice(3, 5)}-${isbn13.slice(5, 8)}-${isbn13.slice(8, 12)}-${isbn13.slice(12)}`;
}

async function openTitleForm(page: Page, name: string, isbn: string) {
  await page.locator("#new-title-btn").click();
  const form = page.locator("#title-form-container form");
  await expect(form).toBeVisible({ timeout: 10000 });
  await form.locator("#title-field").fill(name);
  await form.locator("#media-type-field").selectOption("book");
  await page.waitForSelector("#page-count-field", { timeout: 10000 });
  await form.locator("#genre-field").selectOption({ index: 1 });
  await form.locator("#language-field").fill("fr");
  await form.locator("#isbn-field").fill(isbn);
  await form.locator('button[type="submit"]').click();
  return form;
}

async function scanFindsTitle(page: Page, isbn13: string, name: string) {
  await simulateScan(page, "#scan-field", isbn13);
  const exists = page
    .locator('.feedback-entry')
    .filter({ hasText: /already in your catalog|déjà dans votre catalogue/i });
  await expect(exists.first()).toBeVisible({ timeout: 10000 });
  await expect(page.locator("#context-banner")).toContainText(name, { timeout: 10000 });
  await expect(page.locator("#scan-field")).toBeFocused();
}

test.describe("Story 17-3 — one ISBN rule", () => {
  test("a hyphenated ISBN typed by hand is found by a bare scan", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto("/catalog");

    const isbn13 = specIsbn("IR", seq(0));
    const name = `ISBN typed by hand IR ${Date.now()}`;
    const form = await openTitleForm(page, name, hyphenated(isbn13));
    await expect(
      page.locator('.feedback-entry[data-feedback-variant="success"]').first(),
    ).toBeVisible({ timeout: 10000 });
    await expect(form).not.toBeVisible();

    await scanFindsTitle(page, isbn13, name);
  });

  test("an ISBN prefix and an ISBN-10 typed by hand are stored as the ISBN-13", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.goto("/catalog");

    const { isbn10, isbn13 } = specIsbn10("IR", seq(1));
    const name = `ISBN-10 by hand IR ${Date.now()}`;
    await openTitleForm(page, name, `ISBN ${isbn10.slice(0, 1)}-${isbn10.slice(1, 4)}-${isbn10.slice(4, 9)}-${isbn10.slice(9)}`);
    await expect(
      page.locator('.feedback-entry[data-feedback-variant="success"]').first(),
    ).toBeVisible({ timeout: 10000 });

    await scanFindsTitle(page, isbn13, name);
  });

  test("an ISBN with a wrong check digit is refused with the field hint", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.goto("/catalog");

    const isbn13 = specIsbn("IR", seq(2));
    const wrong = isbn13.slice(0, 12) + ((Number(isbn13[12]) + 1) % 10).toString();
    const name = `Wrong check digit IR ${Date.now()}`;
    const form = await openTitleForm(page, name, hyphenated(wrong));

    const hint = page
      .locator(".feedback-entry")
      .filter({ hasText: /check digit does not match|clé de contrôle ne correspond pas/i });
    await expect(hint.first()).toBeVisible({ timeout: 10000 });
    await expect(form).toBeVisible();

    // Nothing was created: the search finds no title of that name.
    await page.goto(`/?q=${encodeURIComponent(name)}`);
    await expect(page.locator("#browse-results")).toBeVisible({ timeout: 10000 });
    await expect(
      page.locator("#browse-results a[href^='/title/']").filter({ hasText: name }),
    ).toHaveCount(0);
  });
});
