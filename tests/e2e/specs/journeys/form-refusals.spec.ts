/**
 * Issue #676 E2E — a form refusal shows its own message next to the field.
 *
 * Real journeys from a blank browser (loginAs(librarian)):
 * - the title page → Edit metadata → an ISBN with a wrong check digit, an
 *   ISBN another title holds, a 12-digit UPC with a wrong check digit: each
 *   refusal shows its terse hint under the field, what was typed stays, focus
 *   stays where the librarian left it (Rule #21), nothing is saved.
 * - /wishlist/new → By ISBN → an ISBN with a wrong check digit, then one that
 *   is not an ISBN at all: the hint shows under the ISBN field; a valid ISBN
 *   then previews as before and the hint is gone.
 *
 * Data: ISBNs from specIsbn("FR", n), title names carrying the spec id `FR`;
 * assertions only on this spec's rows (Rule #23).
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

const CHECK_DIGIT = /check digit does not match|clé de contrôle ne correspond pas/i;
const ISBN_TAKEN = /Another title already uses this ISBN|Un autre titre utilise déjà cet ISBN/i;
const GENERIC = /Server error|Erreur serveur/i;

/** `isbn` with its check digit made wrong. */
function wrongCheckDigit(isbn: string): string {
  const last = Number(isbn.at(-1));
  return isbn.slice(0, -1) + String((last + 1) % 10);
}

/** A 12-digit UPC-A whose check digit is wrong. */
function badUpcA(): string {
  let body = "5";
  while (body.length < 11) body += Math.floor(Math.random() * 10);
  const sum = [...body].reduce((acc, c, i) => acc + Number(c) * (i % 2 === 0 ? 3 : 1), 0);
  const valid = (10 - (sum % 10)) % 10;
  return body + String((valid + 1) % 10);
}

/** The id of this spec's title named `name`, or null. */
async function titleId(page: Page, name: string): Promise<string | null> {
  const response = await page.request.get(`/?q=${encodeURIComponent(name)}`, {
    headers: { "HX-Request": "true" },
  });
  const html = await response.text();
  const at = html.indexOf(name);
  if (at < 0) return null;
  const links = [...html.slice(0, at).matchAll(/href="\/title\/(\d+)"/g)];
  return links.at(-1)?.[1] ?? null;
}

/** Creates a book through the manual form and opens its edit form. */
async function editNewTitle(page: Page, name: string) {
  await page.goto("/catalog");
  await page.locator("#new-title-btn").click();
  const form = page.locator("#title-form-container form");
  await expect(form).toBeVisible({ timeout: 10000 });
  await form.locator("#title-field").fill(name);
  await form.locator("#media-type-field").selectOption("book");
  await page.waitForSelector("#page-count-field", { timeout: 10000 });
  await form.locator("#genre-field").selectOption({ index: 1 });
  await form.locator("#language-field").fill("fr");
  await form.locator('button[type="submit"]').click();
  await expect(form).not.toBeVisible({ timeout: 10000 });
  const id = await titleId(page, name);
  expect(id).not.toBeNull();

  await page.goto(`/title/${id}`);
  await page.getByRole("button", { name: /Edit metadata|Modifier les métadonnées/i }).click();
  await expect(page.locator("#title-edit-form")).toBeVisible({ timeout: 5000 });
  return id as string;
}

test.describe("Issue #676 — form refusals show their message", () => {
  test("the title edit form shows each ISBN and UPC refusal under its field", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");

    // Another title holds this ISBN (a re-run finds it already there).
    const taken = specIsbn("FR", 1);
    await page.goto("/catalog");
    const scan = page.locator("#scan-field");
    await scan.fill(taken);
    await scan.press("Enter");
    await expect(page.locator(".feedback-entry, .feedback-skeleton").first()).toBeVisible({
      timeout: 10000,
    });

    const name = `Refusals FR ${Date.now()}`;
    const id = await editNewTitle(page, name);
    const isbn = page.locator("#edit-isbn");
    const upc = page.locator("#edit-upc");
    const subtitle = page.locator("#edit-subtitle");
    await subtitle.fill("typed before the refusal");

    // 1. A wrong check digit, submitted with Enter from the ISBN field.
    const typo = wrongCheckDigit(specIsbn("FR", 2));
    await isbn.fill(typo);
    await isbn.press("Enter");
    const isbnError = page.locator("#edit-isbn-error");
    await expect(isbnError).toContainText(CHECK_DIGIT, { timeout: 10000 });
    await expect(isbn).toBeFocused();
    await expect(isbn).toHaveValue(typo);
    await expect(subtitle).toHaveValue("typed before the refusal");
    await expect(page.locator("#feedback-list")).not.toContainText(GENERIC);

    // The hint goes as soon as the field changes.
    await isbn.fill(taken);
    await expect(isbnError).toBeEmpty();

    // 2. An ISBN another title holds, submitted with the Save button.
    await page.locator("#edit-title-submit").click();
    await expect(isbnError).toContainText(ISBN_TAKEN, { timeout: 10000 });
    await expect(page.locator("#edit-title-submit")).toBeFocused();
    await expect(isbn).toHaveValue(taken);
    await expect(page.locator("#feedback-list")).not.toContainText(GENERIC);

    // 3. A refused UPC, submitted with Enter from the UPC field — another
    // input than the ISBN one holds focus (Rule #21).
    await isbn.fill("");
    const code = badUpcA();
    await upc.fill(code);
    await upc.press("Enter");
    await expect(page.locator("#edit-upc-warning")).toContainText(CHECK_DIGIT, { timeout: 10000 });
    await expect(upc).toBeFocused();
    await expect(upc).toHaveValue(code);
    await expect(page.locator("#feedback-list")).not.toContainText(GENERIC);

    // Nothing was saved.
    const saved = await page.request.get(`/title/${id}/metadata`, { headers: { "HX-Request": "true" } });
    const html = await saved.text();
    expect(html).not.toContain(typo);
    expect(html).not.toContain(code);
    expect(html).not.toContain("typed before the refusal");
  });

  test("the wish list's By ISBN form shows its refusal under the field", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.goto("/wishlist/new");

    const field = page.locator("#isbn-input");
    const error = page.locator("#isbn-input-error");
    const typo = wrongCheckDigit(specIsbn("FR", 3));
    await field.fill(typo);
    await field.press("Enter");
    await expect(error).toContainText(CHECK_DIGIT, { timeout: 10000 });
    await expect(field).toBeFocused();
    await expect(field).toHaveValue(typo);

    await field.fill("12345");
    await field.press("Enter");
    await expect(error).toContainText(/ISBN/, { timeout: 10000 });
    await expect(error).not.toContainText(CHECK_DIGIT);
    await expect(field).toHaveValue("12345");

    // A valid ISBN previews as before; the hint is gone.
    await field.fill(specIsbn("FR", 3));
    await expect(error).toBeEmpty();
    await field.press("Enter");
    await expect(page.locator("#wishlist-preview")).toContainText(specIsbn("FR", 3), { timeout: 15000 });
    await expect(error).toBeEmpty();
  });
});
