/**
 * Story 17-4 E2E (#643 items 1 and 2, FR175) — a barcode typed by hand gets
 * the scan's warnings.
 *
 * Real journeys from a blank browser (loginAs(librarian)):
 * - /catalog → New title → a 9-digit UPC → the scan's warning under the field,
 *   focus stays in the UPC field (Rule #21), nothing created → "keep as
 *   entered" ticked → created with the code as entered.
 * - the title page → Edit metadata → an 11-digit UPC → warning, nothing saved
 *   → corrected to a valid EAN-13 → saved with no further question.
 * - a 12-digit code with a wrong check digit scanned, a type picked from the
 *   choice → the #384 message, not the generic creation failure.
 *
 * Data: random codes of this spec's own shapes, title names carrying the spec
 * id `TU`; assertions only on this spec's rows (Rule #23).
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

/** `length` random digits, never starting like an ISBN or an ISSN prefix. */
function digits(length: number): string {
  let out = "5";
  while (out.length < length) out += Math.floor(Math.random() * 10);
  return out;
}

/** A valid EAN-13 of this spec's own. */
function validEan13(): string {
  const body = digits(12);
  const sum = [...body].reduce((acc, c, i) => acc + Number(c) * (i % 2 === 0 ? 1 : 3), 0);
  return body + String((10 - (sum % 10)) % 10);
}

/** A 12-digit UPC-A whose check digit is wrong. */
function badUpcA(): string {
  const body = digits(11);
  const sum = [...body].reduce((acc, c, i) => acc + Number(c) * (i % 2 === 0 ? 3 : 1), 0);
  const valid = (10 - (sum % 10)) % 10;
  return body + String((valid + 1) % 10);
}

/** The id of this spec's title named `name`, or null. */
async function titleId(page: Page, name: string): Promise<string | null> {
  const response = await page.request.get(`/?q=${encodeURIComponent(name)}`, {
    headers: { "HX-Request": "true" },
  });
  const match = (await response.text()).match(/href="\/title\/(\d+)"/);
  return match ? match[1] : null;
}

async function openTitleForm(page: Page, name: string) {
  await page.locator("#new-title-btn").click();
  const form = page.locator("#title-form-container form");
  await expect(form).toBeVisible({ timeout: 10000 });
  await form.locator("#title-field").fill(name);
  await form.locator("#media-type-field").selectOption("book");
  await page.waitForSelector("#page-count-field", { timeout: 10000 });
  await form.locator("#genre-field").selectOption({ index: 1 });
  await form.locator("#language-field").fill("fr");
  return form;
}

test.describe("Story 17-4 — typed codes warned like scanned ones", () => {
  test("the manual title form warns on an irregular UPC and keeps it when confirmed", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto("/catalog");

    const name = `Typed UPC TU ${Date.now()}`;
    const nine = digits(9);
    const form = await openTitleForm(page, name);
    const upc = form.locator("#upc-field");
    await upc.fill(nine);
    await upc.press("Enter");

    const slot = page.locator("#upc-field-warning");
    await expect(slot.locator("[data-code-warning]")).toContainText(/9/, { timeout: 10000 });
    await expect(upc).toBeFocused();
    expect(await titleId(page, name)).toBeNull();

    await slot.getByRole("checkbox", { name: new RegExp(nine) }).check();
    await form.locator('button[type="submit"]').click();
    await expect(
      page.locator('.feedback-entry[data-feedback-variant="success"]').first(),
    ).toBeVisible({ timeout: 10000 });
    await expect(form).not.toBeVisible();

    const id = await titleId(page, name);
    expect(id).not.toBeNull();
    await page.goto(`/title/${id}`);
    await expect(page.locator("#title-metadata")).toContainText(nine);
  });

  test("the title edit form warns on an irregular UPC, and a correction saves", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.goto("/catalog");

    const name = `Edited UPC TU ${Date.now()}`;
    const form = await openTitleForm(page, name);
    await form.locator('button[type="submit"]').click();
    await expect(form).not.toBeVisible({ timeout: 10000 });
    const id = await titleId(page, name);
    expect(id).not.toBeNull();

    await page.goto(`/title/${id}`);
    await page.getByRole("button", { name: /Edit metadata|Modifier les métadonnées/i }).click();
    const upc = page.locator("#edit-upc");
    await expect(upc).toBeVisible({ timeout: 5000 });
    const pc = page.locator("#edit-page-count");
    if (await pc.isVisible({ timeout: 500 }).catch(() => false)) {
      if (!(await pc.inputValue())) await pc.fill("0");
    }

    const eleven = digits(11);
    await upc.fill(eleven);
    await page.locator("#edit-title-submit").click();
    await expect(page.locator("#edit-upc-warning [data-code-warning]")).toContainText(/11/, {
      timeout: 10000,
    });
    await expect(page.locator("#edit-title-submit")).toBeFocused();

    // Corrected: a valid barcode saves with no further question.
    const valid = validEan13();
    await upc.fill(valid);
    await page.locator("#edit-title-submit").click();
    await expect(page.locator("#edit-upc")).toHaveCount(0, { timeout: 10000 });
    await page.reload();
    await expect(page.locator("#title-metadata")).toContainText(`UPC: ${valid}`);
  });

  test("a wrong 12-digit check digit picked from the choice gets the UPC message", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.goto("/catalog");

    const code = badUpcA();
    const field = page.locator("#scan-field");
    await field.fill(code);
    await field.press("Enter");
    await page.locator(`button[role="radio"][hx-vals*="${code}"]`, { hasText: /CD/ }).click();

    const entry = page.locator(`.feedback-entry[data-scan-code="${code}"]`);
    await expect(entry).toContainText(/check digit does not match|clé de contrôle ne correspond pas/i, {
      timeout: 10000,
    });
    await expect(entry).not.toContainText(/couldn't create the title|pas pu créer/i);
  });
});
