/**
 * Story 17-2 E2E (#652, FR173, UX-DR43) — add a volume from the title page.
 *
 * Real journey from a blank browser: loginAs(librarian) → /catalog → scan an
 * ISBN → the title's page → a scanner burst in the volumes section's V-code
 * field (lowercase) adds the volume to THIS title; the list and its count
 * update without a reload and focus stays in the field (Rule #21). A second
 * label asks the CR #300 confirmation; Confirm adds it and focus returns to
 * the field. A label of another title is answered with a link to its volume,
 * never created again. An Anonymous visitor sees no field. axe on the page.
 *
 * Data: ISBNs from specIsbn("TP", n); V-codes this spec's own (V9900–V9989
 * base, random); assertions only on the rows this test creates (Rule #23).
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { simulateScan } from "../../helpers/scanner";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { checkAccessibility } from "../../helpers/accessibility";

const FIELD = "#title-volume-vcode";
const FEEDBACK = "#title-volume-add-feedback";

function labelBase(): number {
  return 9900 + Math.floor(Math.random() * 80);
}

/** Scan `isbn` on /catalog and return the new title's id. */
async function scanTitle(page: Page, isbn: string): Promise<string> {
  await page.goto("/catalog");
  await simulateScan(page, "#scan-field", isbn);
  return titleIdFromSkeleton(page);
}

test.describe("Story 17-2 — add a volume from the title page", () => {
  test("a scanner burst adds volumes to this title, focus stays in the field", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    const titleId = await scanTitle(page, specIsbn("TP", 1));

    await page.goto(`/title/${titleId}`);
    const field = page.locator(FIELD);
    await expect(field).toBeVisible();
    await expect(page.locator("#title-volumes-count")).toHaveText("(0)");

    const base = labelBase();
    const first = `V${base}`;
    await simulateScan(page, FIELD, first.toLowerCase());
    const created = page.locator(`${FEEDBACK} .feedback-entry[data-feedback-variant="success"]`);
    await expect(created).toContainText(first, { timeout: 10000 });
    await expect(page.locator("#title-volumes-list")).toContainText(first);
    await expect(page.locator("#title-volumes-count")).toHaveText("(1)");
    await expect(field).toBeFocused();
    await expect(field).toHaveValue("");

    // The title now has a volume: a new label asks the CR #300 confirmation.
    const second = `V${base + 1}`;
    await simulateScan(page, FIELD, second);
    const dialog = page.locator("#modal-slot dialog[open]");
    await expect(dialog).toBeVisible({ timeout: 10000 });
    await dialog.locator("[data-modal-confirm]").click();
    await expect(dialog).toHaveCount(0, { timeout: 10000 });
    await expect(created).toContainText(second, { timeout: 10000 });
    await expect(page.locator("#title-volumes-count")).toHaveText("(2)");
    await expect(page.locator("#title-volumes-list")).toContainText(second);
    await expect(field).toBeFocused();

    // The same label again: already recorded for this title, nothing added.
    await simulateScan(page, FIELD, first);
    await expect(
      page.locator(`${FEEDBACK} .feedback-entry[data-feedback-variant="info"]`),
    ).toContainText(first, { timeout: 10000 });
    await expect(page.locator("#title-volumes-count")).toHaveText("(2)");
    await expect(field).toBeFocused();

    // Not a V-code: the expected shape is named.
    await simulateScan(page, FIELD, "9782070360246");
    await expect(page.locator(FEEDBACK)).toContainText(/V0042/, { timeout: 10000 });
    await expect(page.locator("#title-volumes-count")).toHaveText("(2)");

    await checkAccessibility(page);

    // Survives a reload: both volumes belong to this title.
    await page.reload();
    await expect(page.locator("#title-volumes-list")).toContainText(first);
    await expect(page.locator("#title-volumes-list")).toContainText(second);
  });

  test("a label of another title links to its volume and adds nothing", async ({ page }) => {
    await loginAs(page, "librarian");
    const owner = await scanTitle(page, specIsbn("TP", 2));
    const label = `V${labelBase() + 5}`;
    await simulateScan(page, "#scan-field", label);
    await expect(
      page.locator('#feedback-list .feedback-entry[data-feedback-variant="success"]').filter({ hasText: label }),
    ).toBeVisible({ timeout: 10000 });

    const other = await scanTitle(page, specIsbn("TP", 3));
    expect(other).not.toEqual(owner);
    await page.goto(`/title/${other}`);
    await simulateScan(page, FIELD, label);
    const warning = page.locator(`${FEEDBACK} .feedback-entry[data-feedback-variant="warning"]`);
    await expect(warning).toContainText(label, { timeout: 10000 });
    await expect(warning.locator('a[href^="/volume/"]')).toBeVisible();
    await expect(page.locator("#title-volumes-count")).toHaveText("(0)");
    await expect(page.locator(FIELD)).toBeFocused();
  });

  test("an Anonymous visitor sees no V-code field", async ({ page }) => {
    await loginAs(page, "librarian");
    const titleId = await scanTitle(page, specIsbn("TP", 4));
    await page.context().clearCookies();

    await page.goto(`/title/${titleId}`);
    await expect(page.locator("#title-volumes")).toBeVisible();
    await expect(page.locator(FIELD)).toHaveCount(0);
  });
});
