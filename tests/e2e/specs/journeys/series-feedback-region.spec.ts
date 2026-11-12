/**
 * Story 18-5 (#526) — an error on the series page is shown, not dropped.
 *
 * Every AppError retargets to `#feedback-list`; the series page had none,
 * so a failed HTMX action did nothing visible. Since 18-5 the base layout
 * carries a default region. The reported case: a series deleted in
 * another tab, then "Delete" clicked on the stale page — the delete modal
 * request answers "not found", which must land next to the page content,
 * with no dialog opened and focus left on the button (Rule #21). And it
 * must be the server's message: HTMX drops 4xx bodies, so mybibli.js's
 * responseError handler inserts an AppError answer (HX-Retarget
 * #feedback-list + one feedback entry); before that, only the generic
 * "Server error (404)" line showed.
 *
 * From a blank browser; the spec uses its own series (Rule #23). It creates
 * a series, so its file name sorts after the specs that assume none exists
 * (#528).
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { checkAccessibility } from "../../helpers/accessibility";

const NOT_FOUND = /couldn't find what you were looking for|Introuvable|nicht finden|non .*trovat/i;
const DELETE = /^(Delete|Supprimer|Löschen|Elimina)$/i;
// mybibli.js's fallback for an error it could not show (#526: until 18-5
// every AppError body was dropped by HTMX and this line stood in for it).
const GENERIC = /Server error|Erreur serveur|Serverfehler|Errore del server/i;

test.describe("Story 18-5 — errors visible on the series page (#526)", () => {
  test("a series deleted in another tab: Delete shows not found, focus stays", async ({
    page,
    context,
  }) => {
    await context.clearCookies();
    await loginAs(page, "librarian");

    const name = `SF-Gone-${Date.now()}`;
    await page.goto("/series/new");
    await page.locator("#series-name").fill(name);
    await page.locator('main button[type="submit"]').last().click();
    await page.waitForURL(/\/series\/\d+$/);
    const seriesUrl = page.url();

    // The region exists and is empty before anything fails.
    const region = page.locator("#feedback-list");
    await expect(region).toHaveCount(1);
    await expect(region).toHaveAttribute("aria-live", "polite");
    await expect(region.locator(".feedback-entry")).toHaveCount(0);

    // Another tab deletes the series.
    const other = await context.newPage();
    await other.goto(seriesUrl);
    await other.getByRole("button", { name: DELETE }).click();
    await expect(other.locator("#modal-slot dialog[open]")).toBeVisible();
    await other.locator("[data-modal-confirm]").click();
    await other.waitForURL(/\/series$/);
    await other.close();

    // Back on the stale page: Delete asks for a modal of a series that is gone.
    const deleteBtn = page.getByRole("button", { name: DELETE });
    await deleteBtn.click();
    await expect(region.locator(".feedback-entry")).toContainText(NOT_FOUND, {
      timeout: 10000,
    });
    // The server's localized message, not the generic "Server error (404)".
    await expect(region).not.toContainText(GENERIC);
    await expect(region.locator(".feedback-entry")).toHaveCount(1);
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);
    await expect(deleteBtn).toBeFocused();

    await checkAccessibility(page);
  });
});
