import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn, specLcode } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createLocation, createChildLocation } from "../../helpers/locations";
import { checkAccessibility } from "../../helpers/accessibility";

/**
 * Story 13-6 (#582) — the title page lists each volume with the FULL path
 * of its location (root → … → leaf, L-code kept), so "Shelf 3" is never
 * ambiguous. Spec ID "FP" ("LP" is taken by labels-page.spec.ts).
 *
 * Codes are derived from the clock inside bands no other spec hardcodes
 * (L6000–L6998, V0600–V0999) so a retry against an un-reset DB starts fresh.
 */
test.describe("Full location path on the title page (story 13-6)", () => {
  test("a volume shelved three levels deep shows root → room → shelf", async ({
    page,
  }) => {
    await loginAs(page);

    // Collision-free codes derived from the clock. L-codes: bucket 6 of
    // `specLcode` (no other spec uses it), three consecutive sequences.
    // V-codes: band V0600–V0999 — no hardcoded V-code of another spec, and
    // disjoint from home.spec.ts (`V${Date.now() % 9000 + 1000}` ≥ V1000).
    const now = Date.now();
    const suffix = `${now % 100000}`;
    const root = `FP-House-${suffix}`;
    const room = `FP-Living-room-${suffix}`;
    const shelf = `FP-Bookcase-by-the-window-shelf-3-${suffix}`;
    const seq = (now % 333) * 3;
    const rootCode = specLcode(6, seq);
    const roomCode = specLcode(6, seq + 1);
    const shelfCode = specLcode(6, seq + 2);
    const vcode = `V0${600 + (now % 400)}`;
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
