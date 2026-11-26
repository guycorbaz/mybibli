import { test, expect, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specLcode } from "../../helpers/isbn";
import { createLocation } from "../../helpers/locations";
import { checkAccessibility } from "../../helpers/accessibility";
import { vcodeSlot } from "../../helpers/vcode";

/**
 * Story 20-4 (#716) — a storage location's optional description: set on the
 * edit form, read on `/location/{id}` (line breaks kept), cleared; over 500
 * characters it is refused next to the field with the typed text kept.
 *
 * Data: L-codes in `specLcode` bucket 9 (only L9998 / L9999 are taken
 * there, by other specs), offset by the attempt's slot so a CI retry on the
 * same stack never meets its own earlier location; names prefixed `LDS-`.
 * The location is reached through `/scan?code=` (story 9-9), not through the
 * tree markup.
 */

/** The L-code `n` (0-9) of this attempt; unique across the spec's tests. */
function lcode(n: number): string {
  return specLcode(9, 100 + vcodeSlot() * 10 + n);
}

/** Create a root location and return its id (read from `/location/{id}`). */
async function createAndOpen(page: Page, name: string, code: string): Promise<string> {
  await createLocation(page, name, code);
  await page.goto(`/scan?code=${code}`);
  await expect(page).toHaveURL(/\/location\/\d+$/);
  const id = page.url().match(/\/location\/(\d+)$/)?.[1];
  expect(id, "location id in the URL").toBeTruthy();
  await expect(page.locator("h1")).toHaveText(name);
  return id as string;
}

test.describe("Location description (story 20-4)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("set a two-line description, read it, clear it", async ({ page }) => {
    const name = `LDS-${vcodeSlot()}-Salon`;
    const id = await createAndOpen(page, name, lcode(0));
    await expect(page.locator("#location-description")).toHaveCount(0);

    await page.goto(`/locations/${id}/edit`);
    const field = page.locator("#edit-description");
    await expect(field).toHaveValue("");
    await expect(page.locator("#edit-description-help")).toContainText(
      /Where to find it|Où le trouver|Wo der Standort zu finden ist|Dove trovarla/,
    );
    await checkAccessibility(page);
    await field.fill("Left bookcase, top shelf\nBehind the door");
    await page.locator("#edit-location-submit").click();
    await expect(page).toHaveURL(/\/locations\/?$/);

    await page.goto(`/location/${id}`);
    const description = page.locator("#location-description");
    await expect(description).toBeVisible();
    await expect(description).toContainText("Left bookcase, top shelf");
    await expect(description).toContainText("Behind the door");
    // Line breaks are kept on the page (whitespace-pre-line).
    expect(await description.innerText()).toMatch(/top shelf\s*\n\s*Behind the door/);

    // The edit form shows it again, then a blank save clears it.
    await page.goto(`/locations/${id}/edit`);
    await expect(field).toHaveValue(/Left bookcase, top shelf\r?\nBehind the door/);
    await field.fill("   ");
    await page.locator("#edit-location-submit").click();
    await expect(page).toHaveURL(/\/locations\/?$/);
    await page.goto(`/location/${id}`);
    await expect(page.locator("h1")).toHaveText(name);
    await expect(page.locator("#location-description")).toHaveCount(0);
  });

  test("501 characters are refused next to the field, the text kept", async ({ page }) => {
    const name = `LDS-${vcodeSlot()}-Cave`;
    const id = await createAndOpen(page, name, lcode(1));

    await page.goto(`/locations/${id}/edit`);
    const field = page.locator("#edit-description");
    const typed = "x".repeat(501);
    await field.fill(typed);
    await page.locator("#edit-location-submit").click();

    // The edit page comes back (422) with the message under the field.
    const error = page.locator("#edit-description-error");
    await expect(error).toBeVisible();
    await expect(error).toHaveText(
      /Description must be at most 500 characters\.|La description doit compter au plus 500 caractères\.|Die Beschreibung darf höchstens 500 Zeichen lang sein\.|La descrizione può contenere al massimo 500 caratteri\./,
    );
    await expect(field).toHaveValue(typed);
    await expect(field).toHaveAttribute("aria-invalid", "true");
    await expect(field).toHaveAttribute(
      "aria-describedby",
      "edit-description-help edit-description-error",
    );
    await expect(page.locator("#edit-name")).toHaveValue(name);
    await checkAccessibility(page);

    // Nothing was saved.
    await page.goto(`/location/${id}`);
    await expect(page.locator("h1")).toHaveText(name);
    await expect(page.locator("#location-description")).toHaveCount(0);
  });
});
