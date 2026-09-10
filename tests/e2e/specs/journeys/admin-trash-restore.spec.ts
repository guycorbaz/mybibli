import { test, expect, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { createBorrower } from "../../helpers/loans";

/**
 * Issue #478 — the Trash panel's Restore journey.
 *
 * The button existed since story 8-6 but pointed at a route that was never
 * registered: HTMX does not swap a 4xx, so a click produced nothing at all
 * — no restore, no error, no feedback entry. Nothing in the suite noticed,
 * because `TrashService::restore` was only ever called from `#[cfg(test)]`.
 *
 * The journey uses a **borrower**, not a series, on purpose: restoring
 * leaves the entity live, and `empty-states.spec.ts` asserts that the
 * /series list is empty. A live borrower collides with nothing.
 */

/** Create a borrower, soft-delete it through its modal, return the name. */
async function seedDeletedBorrower(page: Page, slug: string): Promise<string> {
  const name = `TR-${slug}-${Date.now() % 1000000}`;
  await createBorrower(page, name);

  const link = page
    .locator('tbody a[href^="/borrower/"]')
    .filter({ hasText: new RegExp(`^\\s*${name}\\s*$`) });
  await link.click();
  await expect(page.locator("h1")).toContainText(name, { timeout: 5000 });

  await page.locator("button[data-modal-trigger]").click();
  await expect(page.locator("#modal-slot dialog[open]")).toBeVisible({
    timeout: 5000,
  });
  await page.locator("[data-modal-confirm]").click();
  await page.waitForURL("**/borrowers", { timeout: 10000 });
  return name;
}

/** Open the Trash tab filtered to borrowers and return the row for `name`. */
async function openTrashRow(page: Page, name: string) {
  await page.goto("/admin?tab=trash", { waitUntil: "domcontentloaded" });
  await expect(
    page.locator('section[aria-labelledby="admin-trash-heading"]'),
  ).toBeVisible({ timeout: 10000 });
  await page.locator("#filter-entity-type").selectOption("borrowers");
  const row = page.locator("tbody tr").filter({ hasText: name });
  await expect(row).toBeVisible({ timeout: 10000 });
  return row;
}

const restoreButton = (row: ReturnType<Page["locator"]>) =>
  row.getByRole("button", { name: /^(restore|restaurer)$/i });

test.describe("Issue #478: restore from the Trash", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page, "admin");
  });

  test("delete a borrower, restore it from the Trash, find it back in the list", async ({
    page,
  }) => {
    const name = await seedDeletedBorrower(page, "SC1");
    const row = await openTrashRow(page, name);

    await restoreButton(row).click();

    // The panel re-renders and the OOB feedback entry names the item.
    await expect(page.locator(".feedback-entry").first()).toContainText(
      new RegExp(`(Restored|Restauré).*${name}`, "i"),
      { timeout: 10000 },
    );

    // The row is gone from the Trash — the restore actually committed.
    await expect(page.locator("tbody tr").filter({ hasText: name })).toHaveCount(
      0,
      { timeout: 10000 },
    );

    // And the borrower is back in its own list.
    await page.goto("/borrowers");
    await expect(
      page.locator('tbody a[href^="/borrower/"]').filter({ hasText: name }),
    ).toBeVisible({ timeout: 10000 });
  });

  test("replaying a restore from a stale panel says so instead of failing silently", async ({
    page,
  }) => {
    const name = await seedDeletedBorrower(page, "SC2");
    const row = await openTrashRow(page, name);

    // Capture the exact request the panel would re-send if the admin left
    // the tab open and clicked twice.
    const url = await restoreButton(row).getAttribute("hx-post");
    expect(url).toBeTruthy();

    await restoreButton(row).click();
    await expect(page.locator(".feedback-entry").first()).toContainText(
      new RegExp(`(Restored|Restauré).*${name}`, "i"),
      { timeout: 10000 },
    );

    // The row is no longer in the trash, so the replay gets the friendly
    // "already gone" copy — never a silent no-op, which is the #478
    // symptom this spec exists to prevent.
    const csrf = await page
      .locator('meta[name="csrf-token"]')
      .getAttribute("content");
    const replay = await page.request.post(url as string, {
      headers: { "X-CSRF-Token": csrf as string, "HX-Request": "true" },
    });
    expect(replay.status()).toBe(404);
    expect(await replay.text()).toMatch(
      /no longer in the trash|plus dans la corbeille/i,
    );
  });
});
