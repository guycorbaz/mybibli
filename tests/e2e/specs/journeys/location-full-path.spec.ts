import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createLocation, createChildLocation } from "../../helpers/locations";
import { checkAccessibility } from "../../helpers/accessibility";

/**
 * Story 13-6 (#582) — the title page lists each volume with the FULL path
 * of its location (root → … → leaf, L-code kept), so "Shelf 3" is never
 * ambiguous. Spec ID "FP" ("LP" is taken by labels-page.spec.ts).
 *
 * Codes are randomised inside bands no other spec uses (L6000–L6999,
 * V7000–V7999) so a retry against an un-reset DB starts fresh.
 */
test.describe("Full location path on the title page (story 13-6)", () => {
  test("a volume shelved three levels deep shows root → room → shelf", async ({
    page,
  }) => {
    await loginAs(page);

    const n = Math.floor(Math.random() * 330);
    const suffix = `${Date.now() % 100000}`;
    const root = `FP-House-${suffix}`;
    const room = `FP-Living-room-${suffix}`;
    const shelf = `FP-Bookcase-by-the-window-shelf-3-${suffix}`;
    const rootCode = `L${6000 + n * 3}`;
    const roomCode = `L${6001 + n * 3}`;
    const shelfCode = `L${6002 + n * 3}`;
    const vcode = `V${7000 + Math.floor(Math.random() * 999)}`;
    const isbn = specIsbn("FP", 1000 + Math.floor(Math.random() * 8999));

    await createLocation(page, root, rootCode);
    await createChildLocation(page, root, room, roomCode);
    await createChildLocation(page, room, shelf, shelfCode);

    // Catalog a title, a volume, then shelve it on the leaf.
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(isbn);
    await scanField.press("Enter");
    const titleId = await titleIdFromSkeleton(page);
    await scanField.fill(vcode);
    await scanField.press("Enter");
    await expect(
      page.locator(".feedback-entry").filter({ hasText: vcode }),
    ).toBeVisible({ timeout: 10000 });
    await scanField.fill(shelfCode);
    await scanField.press("Enter");
    await expect(
      page.locator(".feedback-entry").filter({ hasText: shelf }).first(),
    ).toBeVisible({ timeout: 10000 });

    const fullPath = `${root} → ${room} → ${shelf}`;

    // Desktop table.
    await page.goto(`/title/${titleId}`);
    const row = page
      .locator("#title-volumes table tbody tr")
      .filter({ hasText: vcode });
    await expect(row).toContainText(fullPath);
    await expect(row).toContainText(`(${shelfCode})`);
    await checkAccessibility(page);

    // Mobile cards: the long path wraps, no horizontal scroll.
    await page.setViewportSize({ width: 375, height: 800 });
    await page.goto(`/title/${titleId}`);
    const card = page.locator("#title-volumes ul li").filter({ hasText: vcode });
    await expect(card.locator("[data-volume-location]")).toContainText(fullPath);
    await expect(card).toBeVisible();
    const overflows = await page.evaluate(
      () =>
        document.documentElement.scrollWidth >
        document.documentElement.clientWidth,
    );
    expect(overflows, "a long location path must wrap, not scroll").toBe(false);
    await checkAccessibility(page);
  });
});
