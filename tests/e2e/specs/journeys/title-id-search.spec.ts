/**
 * Story 17-5 E2E (#653, FR176) — find a title by its id.
 *
 * Real journey from a blank browser: loginAs(librarian) → a title scanned on
 * /catalog → its page shows "#<id>" → `#<id>` typed in the home search field
 * lists it → a scanner-speed `#<id>` + Enter opens it → the title is deleted
 * from its page → `#<id>` now says it is in the Trash; an unknown id says so
 * too; neither offers "Add this title".
 *
 * Data: ISBNs from specIsbn("TI", n); assertions only on the titles this
 * test creates (Rule #23). The unknown id is far above any id a test stack
 * reaches.
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { simulateScan, simulateTyping } from "../../helpers/scanner";
import { titleIdFromSkeleton } from "../../helpers/catalog";

/** Scan `isbn` on /catalog and return the new title's id. */
async function scanTitle(page: Page, isbn: string): Promise<string> {
  await page.goto("/catalog");
  await simulateScan(page, "#scan-field", isbn);
  return titleIdFromSkeleton(page);
}

/** Type `query` at human pace in the home search field. */
async function searchHome(page: Page, query: string): Promise<void> {
  await page.goto("/");
  await simulateTyping(page, "#search-field", query);
}

test.describe("Story 17-5 — find a title by its id", () => {
  test("an id lists, opens and explains its title", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");

    const id = await scanTitle(page, specIsbn("TI", 1));

    // The title page shows the id the search accepts.
    await page.goto(`/title/${id}`);
    await expect(page.locator("#title-id")).toContainText(`#${id}`);
    await expect(page.locator("#title-id")).toContainText(/Title ID|Identifiant du titre/);

    // Typed in the search field, the id lists that title alone.
    await searchHome(page, `#${id}`);
    const rows = page.locator("#browse-results table.browse-table tbody tr");
    await expect(rows.locator(`a[href="/title/${id}"]`).first()).toBeVisible({ timeout: 10000 });
    await expect(rows).toHaveCount(1);
    await expect(page).toHaveURL((url) => url.pathname === "/");

    // At scanner speed + Enter, the home scan opens it.
    await page.goto("/");
    await simulateScan(page, "#search-field", `#${id}`);
    await page.waitForURL(new RegExp(`/title/${id}$`), { timeout: 10000 });

    // Deleted from its page (no volume), the title goes to the Trash.
    await page.locator("#title-delete-button").click();
    const dialog = page.locator("#modal-slot dialog[open]");
    await expect(dialog).toBeVisible({ timeout: 10000 });
    await dialog.locator("[data-modal-confirm]").click();
    await page.waitForURL((url) => url.pathname === "/", { timeout: 10000 });

    await searchHome(page, `#${id}`);
    const results = page.locator("#browse-results");
    await expect(results).toContainText(
      new RegExp(`Title #${id} is in the Trash|Le titre #${id} est dans la corbeille`),
      { timeout: 10000 },
    );
    await expect(results.locator('a[href^="/catalog/title/new"]')).toHaveCount(0);

    // The scan of a deleted title's id lands on that explanation.
    await page.goto("/");
    await simulateScan(page, "#search-field", `#${id}`);
    await page.waitForURL(new RegExp(`/\\?q=%23${id}$`), { timeout: 10000 });
    await expect(page.locator("#browse-results")).toContainText(/in the Trash|dans la corbeille/);
  });

  test("an unknown id says so, a bare number stays a text search", async ({ page }) => {
    await loginAs(page, "librarian");

    await searchHome(page, "#9999999999");
    const results = page.locator("#browse-results");
    await expect(results).toContainText(
      /No title has the ID #9999999999|Aucun titre ne porte l'identifiant #9999999999/,
      { timeout: 10000 },
    );
    await expect(results.locator('a[href^="/catalog/title/new"]')).toHaveCount(0);

    // Without `#`, digits (here neither an ISBN nor a code) are an ordinary search.
    await searchHome(page, "987654321");
    await expect(results).toContainText(/No matches|Aucun résultat/, { timeout: 10000 });
    await expect(results).not.toContainText(/No title has the ID|Aucun titre ne porte/);
  });
});
