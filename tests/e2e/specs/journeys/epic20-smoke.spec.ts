/**
 * Story 20-6 — Epic 20 smoke test (Foundation Rule #7): the wish list, the
 * series and the locations, end to end, from a blank browser. If this test
 * fails, epic 20 is not done.
 *
 * Journey:
 *   blank context → loginAs(librarian) → a closed series of total 4 and a
 *   title scanned on /catalog, assigned at position 1 → gap 3 sent to the
 *   wish list from the series grid (20-2): the form opens in wish list mode,
 *   Save lands back on the series page with the success entry and its link,
 *   the gap shows the expected tome → the entry is removed from /wishlist
 *   through "Remove" and its modal (20-1); the expected tome stays → the
 *   total lowered to 2 is refused next to the total field, naming position
 *   3 (20-3); the stored total is still 4 → a root location and a child;
 *   the root's description is set and read on /location/{id} (20-4) → on
 *   /locations the root's disclosure button is reached by keyboard, named
 *   "Locations inside {name}", and Enter collapses then expands it (20-5).
 *
 * Data: spec id "E0". ISBN `specIsbn("E0", …)` derived from the run stamp
 * and the attempt's slot (a retry or a re-run never scans an ISBN it
 * already catalogued); series and location names run-stamped; the two
 * L-codes drawn in bucket 9, L9800–L9997, among the codes /locations does
 * not list yet. No V-code is created. Assertions only on this spec's rows
 * (Rule #23).
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { vcodeSlot } from "../../helpers/vcode";
import { simulateScan } from "../../helpers/scanner";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { expectHtmxProcessed } from "../../helpers/htmx";
import {
  createLocation,
  createChildLocation,
  freeLcodePair,
} from "../../helpers/locations";

const RUN = Math.floor(Date.now() / 1000) % 20000;
const DIALOG = "#modal-slot dialog[open]";

/** Escape a run-stamped string for use inside a RegExp. */
function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

test.describe("Epic 20 smoke — wish list, series and locations", () => {
  test("a gap to the wish list and back, the total rule, a described location, the tree by keyboard", async ({
    page,
    context,
  }) => {
    test.setTimeout(120000);
    const slot = vcodeSlot();
    const isbn = specIsbn("E0", ((RUN * 4 + slot) % 9000) * 10 + 1);
    const seriesName = `E0-Series-${RUN}-${slot}`;
    const rootName = `E0-Root-${RUN}-${slot}`;
    const childName = `E0-Child-${RUN}-${slot}`;

    // AC1 — a blank browser and a real login.
    await context.clearCookies();
    await loginAs(page, "librarian");

    // AC2 — a closed series of total 4 and one title at position 1.
    await page.goto("/series/new");
    await page.locator("#series-name").fill(seriesName);
    await page.locator("#series-type").selectOption("closed");
    await page.locator("#series-total").fill("4");
    await page.locator("#series-form-submit").click();
    await page.waitForURL(/\/series\/\d+$/);
    const seriesPath = new URL(page.url()).pathname;

    await page.goto("/catalog");
    await simulateScan(page, "#scan-field", isbn);
    const titleId = await titleIdFromSkeleton(page);
    await page.goto(`/title/${titleId}`);
    await page.locator("#assign-series").selectOption({ label: seriesName });
    await page.locator("#assign-position").fill("1");
    await page.locator("#assign-series-submit").click();
    await page.waitForURL(/\/title\/\d+/);
    await page.goto(seriesPath);
    await expect(
      page.locator(`a[role="gridcell"][href="/title/${titleId}"]`),
    ).toBeVisible();

    // Gap 3 → the wish list (20-2).
    const gap = page.locator('[data-gap-wishlist="3"]');
    await expect(gap).toHaveAccessibleName(
      /Add to wish list: tome 3|Ajouter à la wish list : tome 3/i,
    );
    await gap.click();
    await page.waitForURL(/\/expected-tomes\/new\?position=3&wishlist=1$/);
    await expect(page.locator("#expected-tome-position")).toHaveValue("3");
    const wishTitle = await page.locator("#expected-tome-title").inputValue();
    expect(wishTitle).toContain(seriesName);
    expect(wishTitle).toContain("3");
    await expect(page.locator("#expected-tome-wishlist-help")).toBeVisible();
    await page.locator("#expected-tome-save").click();

    await page.waitForURL(
      (url) => url.pathname === seriesPath && url.searchParams.has("wishlist_item"),
    );
    const feedback = page.locator("#series-feedback");
    await expect(feedback).toContainText(/added to your wish list|ajouté à votre wish list/i);
    const entryLink = feedback.getByRole("link", {
      name: /open the wish list entry|ouvrir l'entrée de la wish list/i,
    });
    const href = await entryLink.getAttribute("href");
    expect(href).toMatch(/^\/wishlist\/\d+$/);
    const tome3 = page.locator('[data-expected-tome][data-position="3"]');
    await expect(tome3).toBeVisible();
    await expect(tome3).toContainText(wishTitle);
    await expect(page.locator('[data-gap-wishlist="3"]')).toHaveCount(0);

    // Remove the entry from /wishlist (20-1).
    await page.goto("/wishlist");
    const row = page.locator(`table a[href="${href}"]`);
    await expect(row).toContainText(wishTitle);
    await page
      .getByRole("button", {
        name: new RegExp(`^(Remove|Retirer) ?: .*${escapeRegExp(wishTitle)}`),
      })
      .click();
    const dialog = page.locator(DIALOG);
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText(wishTitle);
    await expect(dialog).not.toContainText(/Trash|Corbeille/i);
    const confirm = dialog.locator("[data-modal-confirm]");
    await expectHtmxProcessed(confirm);
    await confirm.click();
    await page.waitForURL(/\/wishlist(\?.*)?$/);
    // /wishlist → /wishlist: the URL wait resolves at once (V1).
    await expect(page.locator(DIALOG)).toHaveCount(0);
    await expect(page.locator(`table a[href="${href}"]`)).toHaveCount(0);

    // Removing the entry leaves the expected tome.
    await page.goto(seriesPath);
    await expect(page.locator('[data-expected-tome][data-position="3"]')).toBeVisible();

    // AC3 — the total rule (20-3).
    await page.goto(`${seriesPath}/edit`);
    const total = page.locator("#series-total");
    await total.fill("2");
    await page.locator("#series-form-submit").click();
    await expect(page.locator("#series-total-error")).toContainText(
      /Total must be at least 3|Le total doit être d’au moins 3/,
    );
    await expect(total).toHaveValue("2");
    await expect(total).toHaveAttribute("aria-invalid", "true");
    await page.goto(`${seriesPath}/edit`);
    await expect(page.locator("#series-total")).toHaveValue("4");

    // AC4 — a described location (20-4).
    const [rootCode, childCode] = await freeLcodePair(page, 9, 800, 998);
    await createLocation(page, rootName, rootCode);
    await createChildLocation(page, rootName, childName, childCode);
    await page.goto(`/scan?code=${rootCode}`);
    await expect(page).toHaveURL(/\/location\/\d+$/);
    const pid = page.url().match(/\/location\/(\d+)$/)?.[1];
    expect(pid, "location id in the URL").toBeTruthy();
    await expect(page.locator("#location-description")).toHaveCount(0);

    await page.goto(`/locations/${pid}/edit`);
    await page
      .locator("#edit-description")
      .fill(`Smoke ${RUN} hallway\nSecond shelf ${RUN}`);
    await page.locator("#edit-location-submit").click();
    await expect(page).toHaveURL(/\/locations\/?$/);
    await page.goto(`/location/${pid}`);
    const description = page.locator("#location-description");
    await expect(description).toBeVisible();
    await expect(description).toContainText(`Smoke ${RUN} hallway`);
    await expect(description).toContainText(`Second shelf ${RUN}`);

    // AC5 — the tree by keyboard (20-5).
    await page.goto("/locations");
    const toggle = page.locator(`#tree-toggle-${pid}`);
    const children = page.locator(`#tree-children-${pid}`);
    await expect(toggle).toHaveAccessibleName(
      new RegExp(`^(Locations inside|Emplacements dans) ${escapeRegExp(rootName)}$`),
    );
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(children).toBeVisible();

    await page.locator(`#tree-node-${pid} > div > a[href="/location/${pid}"]`).focus();
    await page.keyboard.press("Shift+Tab");
    await expect(toggle).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await expect(children).toBeHidden();
    await expect(toggle).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(children.filter({ hasText: childName })).toBeVisible();
    await expect(toggle).toBeFocused();
  });
});
