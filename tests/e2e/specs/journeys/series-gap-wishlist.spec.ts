import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { checkAccessibility } from "../../helpers/accessibility";

// Story 20-2 (FR184, #715; D4–D6) — a series gap sent to the wish list
// from the series grid: the expected tome and the wish list entry in one
// form, the success entry on the series page, the existing-entry answer
// and a refusal under its field. specId "GW" (no ISBN or V-code needed:
// every row hangs on a series this spec creates). Series names carry a
// run stamp so a rerun on a non-reset stack never meets an earlier run's
// wish list entry (the title + note duplicate rule, Rule #23).

const RUN = Math.floor(Date.now() / 1000) % 20000;

async function createClosedSeries(page: Page, name: string, total: number) {
  await page.goto("/series/new");
  await page.locator("#series-name").fill(name);
  await page.locator("#series-type").selectOption("closed");
  await page.locator("#series-total").fill(String(total));
  await page.locator('main button[type="submit"]').last().click();
  await page.waitForURL(/\/series\/\d+$/);
  return new URL(page.url()).pathname;
}

async function openGapWishlist(page: Page, position: number) {
  const action = page.locator(`[data-gap-wishlist="${position}"]`);
  await expect(action).toHaveAccessibleName(
    new RegExp(
      `Add to wish list: tome ${position}|Ajouter à la wish list : tome ${position}`,
      "i",
    ),
  );
  await action.click();
  await page.waitForURL(/\/expected-tomes\/new\?position=\d+&wishlist=1$/);
}

test.describe("Series gap to the wish list (story 20-2)", () => {
  test("a gap goes to the wish list, then a second send finds the entry", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    const name = `GW-Gap-${RUN}`;
    const seriesPath = await createClosedSeries(page, name, 3);
    const prefill = `${name}, tome 2`;

    // The gap's action opens the form in wish list mode, title pre-filled.
    await openGapWishlist(page, 2);
    await expect(page.locator("#expected-tome-position")).toHaveValue("2");
    await expect(page.locator("#expected-tome-title")).toHaveValue(prefill);
    await expect(page.locator("#expected-tome-wishlist-help")).toBeVisible();
    await checkAccessibility(page);
    // A double click posts once (review pass 1): the success entry, never
    // the "already on your wish list" answer of a second post.
    await page.locator("#expected-tome-save").dblclick();

    // Back on the series page: the success entry, its link, the named gap.
    await page.waitForURL(
      (url) =>
        url.pathname === seriesPath && url.searchParams.has("wishlist_item"),
    );
    const feedback = page.locator("#series-feedback");
    await expect(feedback).toContainText(
      /added to your wish list|ajouté à votre wish list/i,
    );
    const entryLink = feedback.getByRole("link", {
      name: /open the wish list entry|ouvrir l'entrée de la wish list/i,
    });
    const href = await entryLink.getAttribute("href");
    expect(href).toMatch(/^\/wishlist\/\d+$/);
    const card = page
      .locator("[data-expected-tome]")
      .filter({ hasText: prefill });
    await expect(card).toHaveAttribute("data-position", "2");
    await expect(page.locator('[data-gap-wishlist="2"]')).toHaveCount(0);

    // Back to the form: Save is usable again (review pass 2, `pageshow`).
    await page.goBack();
    await expect(page.locator("#expected-tome-save")).toBeEnabled();

    // The entry is on the wish list.
    await page.goto("/wishlist");
    await expect(page.locator(`a[href="${href}"]`).first()).toContainText(
      prefill,
    );

    // Delete the expected tome (the entry stays), send gap 2 again: the
    // same title + note find the first entry, nothing is created.
    await page.goto(seriesPath);
    await card.getByRole("button", { name: /^(delete|supprimer) —/i }).click();
    const dialog = page.locator("dialog[open]");
    await expect(dialog).toBeVisible();
    await dialog.locator("[data-modal-confirm]").click();
    await page.waitForURL((url) => url.pathname === seriesPath);
    await expect(page.locator("[data-expected-tome]")).toHaveCount(0);

    await openGapWishlist(page, 2);
    await expect(page.locator("#expected-tome-title")).toHaveValue(prefill);
    await page.locator("#expected-tome-save").click();
    const notice = page.locator("#expected-tome-notice");
    await expect(notice).toContainText(
      /already on your wish list|déjà dans votre wish list/i,
    );
    await expect(notice.locator(`a[href="${href}"]`)).toBeVisible();

    // Nothing was created: the gap is still plain.
    await page.goto(seriesPath);
    await expect(page.locator("[data-expected-tome]")).toHaveCount(0);
    await expect(page.locator('[data-gap-wishlist="2"]')).toBeVisible();
  });

  test("a blank title is refused under its field, other values kept", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    const name = `GW-Refuse-${RUN}`;
    const seriesPath = await createClosedSeries(page, name, 3);

    await openGapWishlist(page, 3);
    const title = page.locator("#expected-tome-title");
    // The browser's own `required` check would stop an empty field; a
    // blank of spaces reaches the server, which trims and refuses it.
    await title.fill("   ");
    await page.locator("#expected-tome-save").click();
    await expect(page.locator("#expected-tome-title-error")).toContainText(
      /title is required|le titre est obligatoire/i,
    );
    await expect(title).toHaveAttribute("aria-invalid", "true");
    // The refused field takes the focus on the re-rendered page.
    await expect(title).toBeFocused();
    await expect(page.locator("#expected-tome-position")).toHaveValue("3");
    await expect(page.locator("#expected-tome-wishlist-help")).toBeVisible();

    await page.goto(seriesPath);
    await expect(page.locator("[data-expected-tome]")).toHaveCount(0);
  });

  test("a failed send gives Save back", async ({ page }) => {
    await loginAs(page, "librarian");
    await createClosedSeries(page, `GW-Fail-${RUN}`, 3);
    await openGapWishlist(page, 1);
    // The post never reaches the server and the browser stays on the form
    // (a 204 answer navigates nowhere): nothing is created.
    await page.route("**/expected-tomes", (route) =>
      route.request().method() === "POST"
        ? route.fulfill({ status: 204, body: "" })
        : route.continue(),
    );
    const save = page.locator("#expected-tome-save");
    await save.click();
    await expect(save).toBeDisabled();
    // Review pass 2: re-enabled after the submit-once timeout (10 s).
    await expect(save).toBeEnabled({ timeout: 15000 });
    await expect(page).toHaveURL(/wishlist=1$/);
  });
});
