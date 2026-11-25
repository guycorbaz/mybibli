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
import { badUpcA, titleIdByName as titleId } from "../../helpers/catalog";
import { expectHtmxProcessed } from "../../helpers/htmx";

const CHECK_DIGIT = /check digit does not match|clé de contrôle ne correspond pas/i;
// Story 19-4 (#679 item 7) — the holder is named, as a link.
const ISBN_TAKEN = /already has this ISBN\. Check the number, or open that title|porte déjà cet ISBN\. Vérifiez le numéro, ou ouvrez ce titre/i;
const TITLE_REQUIRED = /Title is required|Le titre est obligatoire/i;
const MEDIA_INVALID = /Invalid media type|Type de média invalide/i;
const GENERIC = /Server error|Erreur serveur/i;

/** `isbn` with its check digit made wrong. */
function wrongCheckDigit(isbn: string): string {
  const last = Number(isbn.at(-1));
  return isbn.slice(0, -1) + String((last + 1) % 10);
}

/** Creates a book through the manual form and opens its edit form. */
async function editNewTitle(page: Page, name: string) {
  await page.goto("/catalog");
  await page.locator("#new-title-btn").click();
  const form = page.locator("#title-form-container form");
  await expectHtmxProcessed(form);
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
  // #685 — the edit form has only `hx-post`: an Enter before HTMX wires it
  // is a native GET submit that reloads the page read-only.
  await expectHtmxProcessed(page.locator("#title-edit-form"));
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
    const holder = isbnError.locator("a");
    await expect(holder).toHaveCount(1);
    await expect(holder).toHaveAttribute("href", /^\/title\/\d+$/);
    await expect(holder).not.toHaveAttribute("href", `/title/${id}`);
    await expect(holder).not.toBeEmpty();
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
    expect(saved.status()).toBe(200);
    const html = await saved.text();
    expect(html).toContain(name);
    expect(html).not.toContain(typo);
    expect(html).not.toContain(code);
    expect(html).not.toContain("typed before the refusal");
  });

  // Story 19-4 (#679 item 6) — an empty title and an unknown media type
  // were answered in the page's feedback list; they show under their field.
  test("the title edit form shows an empty title and an unknown media type under their fields", async ({
    page,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    const name = `Fields FR ${Date.now()}`;
    const id = await editNewTitle(page, name);
    const title = page.locator("#edit-title");
    const subtitle = page.locator("#edit-subtitle");
    await subtitle.fill("typed before the refusal");

    // 1. An empty title: the browser's `required` is removed in the page,
    // so the server's refusal is what the librarian meets.
    await title.evaluate((el) => el.removeAttribute("required"));
    await title.fill("");
    await title.press("Enter");
    const titleError = page.locator("#edit-title-error");
    await expect(titleError).toContainText(TITLE_REQUIRED, { timeout: 10000 });
    await expect(title).toBeFocused();
    await expect(title).toHaveValue("");
    await expect(subtitle).toHaveValue("typed before the refusal");
    await expect(page.locator("#feedback-list")).not.toContainText(TITLE_REQUIRED);
    await title.fill(name);
    await expect(titleError).toBeEmpty();

    // 2. A media type outside the <select>, posted with the Save button.
    const media = page.locator("#edit-media-type");
    await media.evaluate((el) => {
      const option = document.createElement("option");
      option.value = "vinyl";
      option.textContent = "Vinyl";
      el.appendChild(option);
    });
    await media.selectOption("vinyl");
    await page.locator("#edit-title-submit").click();
    const mediaError = page.locator("#edit-media-type-error");
    await expect(mediaError).toContainText(MEDIA_INVALID, { timeout: 10000 });
    await expect(page.locator("#edit-title-submit")).toBeFocused();
    await expect(media).toHaveValue("vinyl");
    await expect(page.locator("#feedback-list")).not.toContainText(MEDIA_INVALID);

    // Nothing was saved.
    const saved = await page.request.get(`/title/${id}/metadata`, { headers: { "HX-Request": "true" } });
    const html = await saved.text();
    expect(html).toContain(name);
    expect(html).not.toContain("typed before the refusal");
  });

  // Story 19-4 (#679 item 7) — the manual form's ISBN refusal names and
  // links the title that holds the ISBN, in the feedback entry.
  test("the manual title form names the title that holds the ISBN", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    const holderName = `Holder FR ${Date.now()}`;
    const isbn = specIsbn("FR", 4);
    const create = async (name: string) => {
      await page.goto("/catalog");
      await page.locator("#new-title-btn").click();
      const form = page.locator("#title-form-container form");
      await expectHtmxProcessed(form);
      await form.locator("#title-field").fill(name);
      await form.locator("#media-type-field").selectOption("book");
      await page.waitForSelector("#page-count-field", { timeout: 10000 });
      await form.locator("#genre-field").selectOption({ index: 1 });
      await form.locator("#language-field").fill("fr");
      await form.locator("#isbn-field").fill(isbn);
      await form.locator('button[type="submit"]').click();
    };
    await create(holderName);
    await expect(page.locator("#title-form-container form")).not.toBeVisible({ timeout: 10000 });
    const holderId = await titleId(page, holderName);
    expect(holderId).not.toBeNull();

    const second = `Second FR ${Date.now()}`;
    await create(second);
    const entry = page.locator("#feedback-list .feedback-entry").filter({ hasText: ISBN_TAKEN });
    await expect(entry).toBeVisible({ timeout: 10000 });
    const link = entry.getByRole("link", { name: holderName });
    await expect(link).toHaveAttribute("href", `/title/${holderId}`);
    expect(await titleId(page, second)).toBeNull();
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
    await expect(error).toContainText(/10 or 13 digits|10 ou 13 chiffres/i, { timeout: 10000 });
    await expect(error).not.toContainText(CHECK_DIGIT);
    await expect(field).toHaveValue("12345");

    // A valid ISBN previews as before; the hint is gone.
    await field.fill(specIsbn("FR", 3));
    await expect(error).toBeEmpty();
    await field.press("Enter");
    const preview = page.locator("#wishlist-preview");
    await expect(preview).toContainText(specIsbn("FR", 3), { timeout: 15000 });
    await expect(error).toBeEmpty();

    // Typed over and refused: the earlier card (and its Confirm) is gone.
    await field.fill(typo);
    await expect(preview).toBeEmpty();
    await field.press("Enter");
    await expect(error).toContainText(CHECK_DIGIT, { timeout: 10000 });
    await expect(preview).toBeEmpty();
  });

  test("the manual title form shows a refused UPC under the field", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.goto("/catalog");
    await page.locator("#new-title-btn").click();
    const form = page.locator("#title-form-container form");
    await expectHtmxProcessed(form);
    const name = `Manual refusal FR ${Date.now()}`;
    await form.locator("#title-field").fill(name);
    await form.locator("#media-type-field").selectOption("cd");
    await form.locator("#genre-field").selectOption({ index: 1 });
    await form.locator("#language-field").fill("fr");

    const upc = form.locator("#upc-field");
    const code = badUpcA();
    await upc.fill(code);
    await upc.press("Enter");
    await expect(page.locator("#upc-field-warning")).toContainText(CHECK_DIGIT, { timeout: 10000 });
    await expect(upc).toBeFocused();
    await expect(upc).toHaveValue(code);
    await expect(form.locator("#title-field")).toHaveValue(name);
    expect(await titleId(page, name)).toBeNull();
  });
});
