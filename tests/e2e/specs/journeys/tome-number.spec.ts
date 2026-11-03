import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

/**
 * Story 13-10 (#579) — the tome number. Spec ID "TN".
 *
 * The mock BnF answers specIsbn("TN", seq ≥ 1000) with the title
 * "Tome 3 : Le Mystère des tomes": the title is stored without the
 * marker, the number shows as "Tome 3 (detected)", pre-fills the series
 * position, and the position is then edited from the title page.
 * Creates its own uniquely named series (#528). Each attempt scans fresh
 * ISBNs, so a retry never meets the state an earlier attempt left.
 */
const freshSeq = (base: number, span: number) => base + (Date.now() % span);
const CLEAN_TITLE = "Le Mystère des tomes";
const TOME = (n: number) => new RegExp(`(Tome|Volume|Band|Tomo)\\s*${n}\\b`);
const DETECTED = /\((détecté|detected|erkannt|rilevato)\)/;

async function scanAndOpen(page: Page, isbn: string, title: string | RegExp) {
  await page.goto("/catalog");
  const scanField = page.locator("#scan-field");
  await scanField.fill(isbn);
  await scanField.press("Enter");
  await expect(
    page
      .locator("#feedback-list .feedback-skeleton, #feedback-list .feedback-entry")
      .first(),
  ).toBeVisible({ timeout: 5000 });

  // The metadata lands in the background: re-query until the title
  // carries its fetched name.
  const titleLink = page.locator(
    "#browse-results table.browse-table tbody tr td a[href^='/title/']",
    { hasText: title },
  );
  await expect(async () => {
    await page.goto(`/?q=${isbn}`);
    await expect(titleLink.first()).toBeVisible({ timeout: 2000 });
  }).toPass({ timeout: 30000 });
  await titleLink.first().click();
  await expect(page).toHaveURL(/\/title\/\d+/);
}

test.describe("The tome number (story 13-10, #579)", () => {
  test("a provider tome marker becomes the title's detected number, then its series position", async ({
    page,
  }) => {
    // Blank context, real login (no cookie injection).
    await loginAs(page);
    const MARKER_ISBN = specIsbn("TN", freshSeq(1000, 90000));

    const seriesName = `TN-Series-${Date.now()}`;
    await page.goto("/series/new");
    await page.locator("#series-name").fill(seriesName);
    await page.locator("#series-type").selectOption("open");
    await page.locator('main button[type="submit"]').last().click();
    await page.waitForURL(/\/series\/\d+/);

    await scanAndOpen(page, MARKER_ISBN, CLEAN_TITLE);
    const titleUrl = page.url();

    // The marker is gone from the title; the number waits as a hint.
    const h1 = page.locator("#title-metadata h1");
    await expect(h1).toHaveText(CLEAN_TITLE);
    await expect(h1).not.toContainText("3");
    const tome = page.locator("#title-tome");
    await expect(tome).toContainText(TOME(3));
    await expect(tome).toContainText(DETECTED);

    // Attaching a series: the position is pre-filled with the hint.
    await page.locator("#assign-series").selectOption({ label: seriesName });
    await expect(page.locator("#assign-position")).toHaveValue("3");
    await page.locator("#assign-series-submit").click();
    await page.waitForURL(titleUrl);

    // The hint is cleared; "Tome 3" now comes from the series position.
    await expect(tome).toContainText(TOME(3));
    await expect(tome).toContainText(seriesName);
    await expect(tome).not.toContainText(DETECTED);

    // Edit the position from the title page.
    const edit = page.locator("[data-position-edit]").first();
    await edit.locator("summary").click();
    await edit.locator("input[name='position_number']").fill("4");
    await edit.locator("button[type='submit']").click();
    await expect(tome).toContainText(TOME(4), { timeout: 10000 });
    await expect(tome).not.toContainText(TOME(3));

    // The browse row shows the number too.
    await page.goto(`/?q=${MARKER_ISBN}`);
    const row = page
      .locator("#browse-results table.browse-table tbody tr", { hasText: CLEAN_TITLE })
      .first();
    await expect(row.locator("[data-tome]")).toContainText(TOME(4));
  });

  test("a hand-typed title keeps its marker and gets a hint", async ({ page }) => {
    await loginAs(page);
    const MANUAL_ISBN = specIsbn("TN", freshSeq(1, 999));
    // The catch-all metadata resolves first ("Test Title <isbn>"), so the
    // manual edit below never races the background write.
    await scanAndOpen(page, MANUAL_ISBN, `Test Title ${MANUAL_ISBN}`);

    await page.locator("button[hx-get$='/edit']").first().click();
    const titleField = page.locator("#edit-title");
    await expect(titleField).toBeVisible({ timeout: 5000 });
    const handTitle = `Saga TN T5 : à la main ${Date.now()}`;
    await titleField.fill(handTitle);
    await page.locator("#edit-title-submit").click();

    // Saved as typed — never rewritten — with a hint on where the number goes.
    await expect(page.locator("#title-metadata")).toContainText(handTitle, {
      timeout: 5000,
    });
    await expect(page.locator("#title-feedback")).toContainText(TOME(5));
  });
});
