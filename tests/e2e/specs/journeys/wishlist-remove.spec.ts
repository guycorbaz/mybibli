/**
 * Story 20-1 (#714, FR183) — "Remove" a wish list entry without saying
 * it was bought.
 *
 * - Remove from the list (desktop table) and from the entry page; the
 *   spec's sibling entry stays (Rule #23: only this spec's rows).
 * - Refusal (Rule #3): the Remove modal opened on a page whose entry was
 *   removed on another page answers "no longer on the wish list" inside
 *   the modal, which stays open.
 * - "Bought" still opens the "bought" modal.
 * - axe on /wishlist.
 *
 * Spec id "WR". Free-form entries with titles unique per run (no ISBN,
 * no note: nothing story 20-2's dedup could match).
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { checkAccessibility } from "../../helpers/accessibility";
import { expectHtmxProcessed } from "../../helpers/htmx";

const DIALOG = "#modal-slot dialog[open]";

function uniqueTitle(tag: string): string {
  return `WR-${tag}-${Date.now()}-${Math.floor(Math.random() * 1e6)}`;
}

/** Adds a free-form entry through the form; ends on /wishlist. */
async function addEntry(page: Page, title: string): Promise<void> {
  await page.goto("/wishlist/new");
  await page.locator('[data-wishlist-mode="freeform"]').click();
  await expect(page.locator("#wishlist-mode-freeform")).toBeVisible();
  await page.locator("#freeform-title").fill(title);
  await page.locator('#wishlist-mode-freeform form button[type="submit"]').click();
  await page.waitForURL(/\/wishlist(\?.*)?$/);
  await expect(rowLink(page, title)).toBeVisible({ timeout: 10000 });
}

/** The entry's link in the desktop table (the mobile cards are hidden). */
function rowLink(page: Page, title: string) {
  return page.locator("table a[href^='/wishlist/']").filter({ hasText: title });
}

function removeButton(page: Page, title: string) {
  return page.getByRole("button", {
    name: new RegExp(`^(Remove|Retirer) .*${title}`),
  });
}

async function openRemoveModal(page: Page, title: string) {
  await removeButton(page, title).click();
  const dialog = page.locator(DIALOG);
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText(title);
  await expect(dialog).toContainText(/Remove|Retirer/i);
  await expectHtmxProcessed(dialog.locator("[data-modal-confirm]"));
  return dialog;
}

test.describe("Story 20-1 — remove an entry from the wish list", () => {
  test("remove from the list and from the entry page; the sibling stays", async ({ page }) => {
    await loginAs(page, "librarian");
    const fromList = uniqueTitle("list");
    const fromPage = uniqueTitle("page");
    const sibling = uniqueTitle("sibling");
    await addEntry(page, fromList);
    await addEntry(page, fromPage);
    await addEntry(page, sibling);

    await checkAccessibility(page);

    // From the list.
    let dialog = await openRemoveModal(page, fromList);
    await expect(dialog).not.toContainText(/Trash|Corbeille/i);
    await dialog.locator("[data-modal-confirm]").click();
    await page.waitForURL(/\/wishlist(\?.*)?$/);
    await expect(rowLink(page, fromList)).toHaveCount(0);
    await expect(rowLink(page, sibling)).toBeVisible();

    // From the entry page.
    await rowLink(page, fromPage).click();
    await page.waitForURL(/\/wishlist\/\d+$/);
    await expect(page.locator("h1")).toContainText(fromPage);
    dialog = await openRemoveModal(page, fromPage);
    await dialog.locator("[data-modal-confirm]").click();
    await page.waitForURL(/\/wishlist(\?.*)?$/);
    await expect(rowLink(page, fromPage)).toHaveCount(0);
    await expect(rowLink(page, sibling)).toBeVisible();
  });

  test("an entry removed on another page is refused inside the modal", async ({ page, context }) => {
    await loginAs(page, "librarian");
    const title = uniqueTitle("stale");
    await addEntry(page, title);

    // Page A: the Remove modal open on the (soon out-of-date) list.
    const dialog = await openRemoveModal(page, title);

    // Page B (same session): removes the same entry.
    const other = await context.newPage();
    await other.goto("/wishlist");
    const otherDialog = await openRemoveModal(other, title);
    await otherDialog.locator("[data-modal-confirm]").click();
    await other.waitForURL(/\/wishlist(\?.*)?$/);
    await expect(rowLink(other, title)).toHaveCount(0);
    await other.close();

    // Back on A: Confirm is refused, in the modal, which stays open.
    await dialog.locator("[data-modal-confirm]").click();
    const error = dialog.locator("[data-modal-error]");
    await expect(error).toBeVisible({ timeout: 10000 });
    await expect(error).toContainText(
      /no longer on the wish list|n’est plus dans la wish list/i,
    );
    await expect(dialog).toBeVisible();
  });

  test("Bought still opens the bought modal", async ({ page }) => {
    await loginAs(page, "librarian");
    const title = uniqueTitle("bought");
    await addEntry(page, title);

    await page
      .getByRole("button", { name: new RegExp(`(Mark|Marquer).*${title}.*(bought|acheté)`, "i") })
      .click();
    const dialog = page.locator(DIALOG);
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText(/as bought|comme acheté/i);
    await expect(dialog).not.toContainText(/Trash|Corbeille/i);
    await expect(dialog.locator("[data-modal-confirm]")).toHaveText(/Bought|Acheté/);

    // Cancel leaves the entry on the list.
    await dialog.locator("[data-modal-cancel]").click();
    await expect(page.locator(DIALOG)).toHaveCount(0);
    await expect(rowLink(page, title)).toBeVisible();
  });
});
