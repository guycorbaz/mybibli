/**
 * Story 13-5 — every series in the title page's selector (#576), and the
 * home page's Recent additions following the browse mode (#577).
 *
 * Spec ID "RT" for ISBNs. Series names carry a per-run unique prefix and
 * are looked up by that name only: the series table is shared with every
 * other spec and is never assumed empty (#528).
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { checkAccessibility } from "../../helpers/accessibility";

async function createSeries(page: Page, name: string): Promise<void> {
  const csrf = await page
    .locator('meta[name="csrf-token"]')
    .getAttribute("content");
  const resp = await page.request.post("/series", {
    form: { _csrf_token: csrf ?? "", name, series_type: "open" },
    maxRedirects: 0,
  });
  expect(resp.status(), `create series ${name}`).toBe(303);
}

async function scanTitle(page: Page, isbn: string): Promise<number> {
  await page.goto("/catalog");
  const scanField = page.locator("#scan-field");
  await scanField.fill(isbn);
  await scanField.press("Enter");
  return Number(await titleIdFromSkeleton(page));
}

test.describe("Story 13-5 — series selector (#576)", () => {
  test("a title page offers and assigns a series sorting after the 25th", async ({
    page,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");

    // 27 series sharing a unique prefix: whatever else the table holds,
    // at least 26 series sort before the last one, so it sits past
    // page 1 (25 rows) of the paginated list the selector used to read.
    // The `zzz-` head sorts them after every other spec's series, so the
    // paginated /series list of a parallel spec keeps its own rows on
    // page 1 (series.spec.ts looks its series up there).
    const prefix = `zzz-RT-${Date.now()}-`;
    await page.goto("/series");
    for (let i = 1; i <= 27; i++) {
      await createSeries(page, `${prefix}${String(i).padStart(2, "0")}`);
    }
    const target = `${prefix}27`;

    const titleId = await scanTitle(page, specIsbn("RT", 1));
    expect(titleId).toBeGreaterThan(0);
    await page.goto(`/title/${titleId}`);

    const select = page.locator("#assign-series");
    const ours = select.locator("option").filter({ hasText: prefix });
    await expect(ours).toHaveCount(27);
    expect(await select.locator("option").count()).toBeGreaterThan(27);

    await select.selectOption({ label: target });
    await page.locator("#assign-position").fill("1");
    await page.locator("#assign-series-submit").click();
    await expect(
      page.locator(`a[href^="/series/"]:has-text("${target}")`),
    ).toBeVisible();
  });
});

test.describe("Story 13-5 — Recent additions follows the browse mode (#577)", () => {
  test("table in list mode, cards in grid mode, live and after reload", async ({
    page,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    // At least one recent addition exists (others may push it out of
    // the newest 10 under parallel load — assertions are on any row).
    await scanTitle(page, specIsbn("RT", 2));

    await page.goto("/");
    const section = page.locator("#recent-additions");
    const fold = section.locator("details#recent-additions-fold");
    await fold.locator(":scope > summary").click();
    await expect(fold).toHaveAttribute("open", "");

    const results = section.locator("#recent-additions-results");
    const table = results.locator("table.browse-table");
    const cards = results.locator(".browse-cards");
    const rowLinks = table.locator("tbody tr td a[href^='/title/']");

    // Default (list) mode: the browse table, with its column headers.
    await expect(results).toHaveClass(/browse-list/);
    await expect(table).toBeVisible();
    // Story 13-7 adds the "Added" column.
    await expect(table.locator("thead th[scope='col']")).toHaveCount(6);
    await expect(rowLinks.first()).toBeVisible();
    await expect(cards).toBeHidden();
    await checkAccessibility(page);

    // Toggle to grid: Recent additions switches live, like #browse-results.
    await page.locator('[data-browse-mode="grid"]').click();
    await expect(results).toHaveClass(/browse-grid/);
    await expect(page.locator("#browse-results")).toHaveClass(/browse-grid/);
    await expect(table).toBeHidden();
    await expect(cards.locator("article.title-card").first()).toBeVisible();

    // The persisted mode applies on page load.
    await page.reload();
    await expect(results).toHaveClass(/browse-grid/);
    await expect(table).toBeHidden();

    // And back to the table.
    await page.locator('[data-browse-mode="list"]').click();
    await expect(results).toHaveClass(/browse-list/);
    await expect(rowLinks.first()).toBeVisible();
    await rowLinks.first().click();
    await page.waitForURL(/\/title\/\d+/);

    await page.evaluate(() => localStorage.removeItem("mybibli_browse_mode"));
  });
});
