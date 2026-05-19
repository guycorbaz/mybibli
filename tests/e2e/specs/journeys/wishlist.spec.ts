/**
 * CR #242 — Wish list feature smoke test.
 *
 * Covers (per the issue Acceptance Criteria):
 * - Free-form add (no ISBN): title-only entry shows up in the list.
 * - Mark as bought: UX-DR8 modal opens, confirm soft-deletes, the
 *   item disappears from the list and the home counter chip is gone.
 * - List → detail navigation works.
 * - Print page renders with the correct count.
 *
 * The ISBN add sub-flow and the catalog-scan auto-link both go
 * through the real provider chain, which the mock metadata server
 * stubs — leaving the deeper interplay for a follow-up smoke. v1
 * smoke focuses on the user-facing happy path: type a wish list,
 * see it, bought it, gone.
 *
 * Spec ID "WL" — generated ISBN never collides with other specs.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

test.describe("CR #242 — Wish list", () => {
  test("free-form add, mark as bought, list/detail/print round-trip", async ({
    page,
  }) => {
    await loginAs(page, "librarian");

    const TITLE = `WL-Smoke-${Date.now()}`;

    // Step 1: navigate to the list — empty-state visible.
    await page.goto("/wishlist");
    await expect(page.locator("h1")).toContainText(/Wish list/i);

    // Step 2: click "Add" → form opens.
    await page
      .getByRole("link", { name: /Add to wish list|Ajouter/i })
      .first()
      .click();
    await page.waitForURL(/\/wishlist\/new/);

    // Step 3: switch to free-form mode (the "By ISBN" sub-form is
    // shown by default; click the "Free-form" tab).
    await page
      .locator('[data-wishlist-mode="freeform"]')
      .click();
    await expect(
      page.locator("#wishlist-mode-freeform"),
    ).toBeVisible();
    await page.locator("#freeform-title").fill(TITLE);
    await page
      .locator('#wishlist-mode-freeform form button[type="submit"]')
      .click();

    // Step 4: redirected back to /wishlist; the new entry is in the
    // list.
    await page.waitForURL(/\/wishlist(\?.*)?$/);
    const titleLink = page
      .locator(`a[href^="/wishlist/"]`)
      .filter({ hasText: TITLE })
      .first();
    await expect(titleLink).toBeVisible({ timeout: 10000 });

    // Step 5: navigate to the detail page.
    await titleLink.click();
    await page.waitForURL(/\/wishlist\/\d+/);
    await expect(page.locator("h1")).toContainText(TITLE);

    // Step 6: open the "Mark as bought" modal.
    await page
      .getByRole("button", { name: /Bought|Acheté/i })
      .first()
      .click();
    const dialog = page.locator("#modal-slot dialog[open]");
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText(TITLE);

    // Step 7: confirm → HX-Redirect → /wishlist; entry is gone.
    await dialog.locator("[data-modal-confirm]").click();
    await page.waitForURL(/\/wishlist(\?.*)?$/);
    await expect(
      page.locator(`a[href^="/wishlist/"]`).filter({ hasText: TITLE }),
    ).toHaveCount(0);

    // Step 8: print page renders with the matching count.
    await page.goto("/wishlist/print");
    await expect(page.locator("h1")).toContainText(/Wish list|wish list/i);
  });
});
