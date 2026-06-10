/**
 * CR #367 E2E — Saved searches.
 *
 * Real user journey from a blank browser (Foundation Rule #7): loginAs →
 * compose a browse state → save it → run it → rename it → delete it. Saved
 * searches are GLOBAL (shared across the instance), so the test uses a
 * unique name and cleans up after itself (the delete step) to avoid
 * polluting the shared E2E database. Spec ID "SS".
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

function uniqueName(prefix: string): string {
  return `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
}

test.describe("CR #367 — Saved searches", () => {
  test.beforeEach(async ({ page }) => {
    await page.context().clearCookies();
  });

  test("lifecycle: create → run → rename → delete", async ({ page }) => {
    await loginAs(page, "admin");
    // Establish a browse state to capture.
    await page.goto("/?q=tintin&sort=title&dir=asc");

    const name = uniqueName("E2E-Saved");
    const renamed = uniqueName("E2E-Renamed");

    // --- Create: open dropdown, name + save the current search ---
    await page.locator("#saved-searches-toggle").click();
    await expect(page.locator("#saved-searches-panel")).toBeVisible();
    await page.locator("#saved-search-name").fill(name);
    await page.locator('#saved-searches-panel form button[type="submit"]').click();

    await expect(
      page
        .locator("#feedback-list")
        .getByText(/Saved search|sauvegardée|gespeichert|salvata/i),
    ).toBeVisible({ timeout: 10000 });

    const runLink = page.locator("#saved-searches-list a", { hasText: name });
    await expect(runLink).toBeVisible();

    // --- Run: the link re-applies the saved browse state ---
    await runLink.click();
    await expect(page).toHaveURL(/[?&]q=tintin/);

    // --- Rename via UX-DR8 modal ---
    await page.locator("#saved-searches-toggle").click();
    const row = page.locator("#saved-searches-list li", { hasText: name });
    await row.locator('button[hx-get*="/rename-modal"]').click();

    const renameInput = page.locator("#saved-search-rename-input");
    await expect(renameInput).toBeVisible({ timeout: 10000 });
    await renameInput.fill(renamed);
    await page.locator("#modal-slot button[data-modal-confirm]").click();

    await expect(
      page.locator("#saved-searches-list a", { hasText: renamed }),
    ).toBeVisible({ timeout: 10000 });
    await expect(
      page.locator("#saved-searches-list a", { hasText: name }),
    ).toHaveCount(0);

    // --- Delete via UX-DR8 modal (cleanup) ---
    const renamedRow = page.locator("#saved-searches-list li", {
      hasText: renamed,
    });
    await renamedRow.locator('button[hx-get*="/delete-modal"]').click();
    await expect(page.locator("#modal-slot dialog")).toBeVisible({
      timeout: 10000,
    });
    await page.locator("#modal-slot button[data-modal-confirm]").click();

    await expect(
      page.locator("#saved-searches-list a", { hasText: renamed }),
    ).toHaveCount(0, { timeout: 10000 });
  });

  test("librarian sees the control; the save form is present", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/");
    await expect(page.locator("#saved-searches-toggle")).toBeVisible();
    await page.locator("#saved-searches-toggle").click();
    await expect(page.locator("#saved-search-name")).toBeVisible();
  });
});
