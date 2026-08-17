import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

/**
 * CR #206 — the title-edit form groups genre and Dewey into a Classification
 * fieldset, apart from the provider-fed bibliographic fields.
 *
 * What is worth asserting here is the CONTAINMENT, not the mere presence of
 * the two controls: they were both on the form before this change too. A test
 * that only checked `#edit-genre` and `#edit-dewey` are visible would have
 * passed against the old layout, and would keep passing if someone pulled one
 * of them back out of the fieldset. So every locator below is scoped THROUGH
 * the fieldset.
 */
test.describe("Classification fieldset (CR #206)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  /** Scan an ISBN, open its title page, and enter the metadata-edit form. */
  async function openEditForm(page: import("@playwright/test").Page, isbn: string) {
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(isbn);
    await page.locator("#scan-field").press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
      timeout: 10000,
    });

    await page.goto(`/?q=${isbn}`);
    await page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first()
      .click();
    await expect(page.locator("h1")).toBeVisible({ timeout: 5000 });

    await page
      .getByRole("button", { name: /Edit metadata|Modifier les métadonnées/i })
      .click();
    await expect(page.locator("#title-edit-form")).toBeVisible({ timeout: 5000 });
  }

  test("genre and Dewey sit inside the Classification fieldset, publication date does not", async ({
    page,
  }) => {
    await openEditForm(page, specIsbn("CL", 1));

    // The fieldset is identified the way a user identifies it: by its legend.
    const classification = page
      .locator("#title-edit-form fieldset")
      .filter({ has: page.locator("legend", { hasText: /Classification|Classement|Systematik|Classificazione/i }) });
    await expect(classification).toHaveCount(1);

    // Containment — the actual subject of this change.
    await expect(classification.locator("#edit-genre")).toBeVisible();
    await expect(classification.locator("#edit-dewey")).toBeVisible();

    // Publication date is bibliographic and must stay OUT of it. Asserting the
    // negative here is what keeps the grouping meaningful: a fieldset that
    // eventually swallows every field classifies nothing.
    await expect(classification.locator("#edit-pub-date")).toHaveCount(0);
    await expect(page.locator("#edit-pub-date")).toBeVisible();

    // The help line carries the asymmetry the form previously left implicit.
    await expect(classification.locator("p")).toContainText(
      /no metadata fetch ever changes it|aucune récupération de métadonnées ne le modifie|kein Metadatenabruf ändert es|nessun recupero di metadati lo modifica/i,
    );
  });

  test("both classification fields still save from inside the fieldset", async ({
    page,
  }) => {
    await openEditForm(page, specIsbn("CL", 2));

    const classification = page
      .locator("#title-edit-form fieldset")
      .filter({ has: page.locator("legend", { hasText: /Classification|Classement|Systematik|Classificazione/i }) });

    // Dewey: a value distinctive enough not to collide with seeded data.
    await classification.locator("#edit-dewey").fill("621.381");

    // Genre: pick whatever second option exists, so the assertion does not
    // depend on the reference-data seed naming.
    const genre = classification.locator("#edit-genre");
    const options = genre.locator("option");
    const count = await options.count();
    const targetLabel =
      count > 1 ? await options.nth(1).innerText() : await options.nth(0).innerText();
    await genre.selectOption({ index: count > 1 ? 1 : 0 });

    // Empty numeric inputs would 422 on an empty-string → i32 parse.
    const pageCount = page.locator("#edit-page-count");
    if (await pageCount.isVisible({ timeout: 500 }).catch(() => false)) {
      const val = await pageCount.inputValue();
      if (!val) await pageCount.fill("0");
    }

    await page.locator('main button[type="submit"]').last().click();

    // Both values survive the HTMX swap back into the metadata panel.
    await expect(page.locator("#title-metadata")).toContainText("621.381", {
      timeout: 10000,
    });
    await expect(page.locator("#title-metadata")).toContainText(targetLabel.trim(), {
      timeout: 10000,
    });
  });
});
