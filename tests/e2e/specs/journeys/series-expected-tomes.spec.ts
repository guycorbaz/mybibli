import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

// Story 11-11 (FR142, FR147, UX-DR34) — expected tomes on the series page,
// "Add to wishlist", and the scan that settles a tome (D-11-10-7 carry-over).
// specId "XT" is unique to this spec. ISBN sequences and series names carry
// a run stamp so a rerun on a non-reset stack never meets an earlier run's
// wishlist item or title.
//
// File name: this spec creates series, and several earlier specs assume
// no series exists yet (empty-states.spec.ts expects an empty /series;
// dewey-code / classification-fieldset click the LAST submit button of a
// title page, which becomes "Add to series" once a series exists). Like
// series.spec.ts, it is named to run after them in the alphabetical
// schedule (D-11-11-6).

const RUN = Math.floor(Date.now() / 1000) % 20000;
const isbnFor = (k: number) => specIsbn("XT", RUN * 4 + k);

async function createSeries(
  page: Page,
  name: string,
  total: number | null,
): Promise<string> {
  await page.goto("/series/new");
  await page.locator("#series-name").fill(name);
  if (total !== null) {
    await page.locator("#series-type").selectOption("closed");
    await page.locator("#series-total").fill(String(total));
  }
  await page.locator('main button[type="submit"]').last().click();
  await page.waitForURL(/\/series\/\d+$/);
  return new URL(page.url()).pathname;
}

async function fillTome(
  page: Page,
  position: number | null,
  title: string,
  isbn: string,
) {
  if (position !== null) {
    await page.locator("#expected-tome-position").fill(String(position));
  }
  await page.locator("#expected-tome-title").fill(title);
  await page.locator("#expected-tome-isbn").fill(isbn);
  await page.locator('main button[type="submit"]').last().click();
  await page.waitForURL(/\/series\/\d+$/);
}

function tomeCard(page: Page, title: string) {
  return page.locator("[data-expected-tome]").filter({ hasText: title });
}

test.describe("Expected tomes on the series page (story 11-11)", () => {
  test("smoke: add a tome, wish it, scan it — it settles at its position", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    const seriesPath = await createSeries(page, `XT-Smoke-${RUN}`, 3);
    const isbn = isbnFor(0);
    const title = `Tome attendu ${RUN}`;

    // Add through the series action.
    await page.locator("#add-expected-tome").click();
    await page.waitForURL(/\/expected-tomes\/new$/);
    await fillTome(page, 2, title, isbn);

    // Shown in the missing square of position 2.
    const card = tomeCard(page, title);
    await expect(card).toBeVisible();
    await expect(card).toHaveAttribute("data-position", "2");
    await expect(card).toContainText(isbn);

    // Add to wishlist, then a second click says it is already there.
    const wish = card.getByRole("button", {
      name: /add to wish list|ajouter à la liste d'envies/i,
    });
    await wish.click();
    await expect(page.locator("#series-feedback")).toContainText(
      /added to your wish list|ajouté à votre liste d'envies/i,
    );
    await wish.click();
    await expect(page.locator("#series-feedback")).toContainText(
      /already on your wish list|déjà dans votre liste d'envies/i,
    );
    await page.goto("/wishlist");
    await expect(page.getByText(title).first()).toBeVisible();

    // Scan the ISBN: the tome settles and the wishlist item is linked.
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(isbn);
    await scanField.press("Enter");
    await expect(page.locator("#feedback-list")).toContainText(
      /Expected tome found|Tome attendu trouvé/i,
      { timeout: 10000 },
    );
    await expect(
      page.locator("#feedback-list [data-wishlist-autolink]").first(),
    ).toBeVisible();

    // The series page shows the title owned at position 2, no tome left.
    await page.goto(seriesPath);
    await expect(page.locator("[data-expected-tome]")).toHaveCount(0);
    const owned = page.locator('a[role="gridcell"][href^="/title/"]');
    await expect(owned).toHaveCount(1);
    await expect(owned).toHaveAttribute("aria-label", / 2 — /);
  });

  test("edit a tome, then delete it through the modal", async ({ page }) => {
    await loginAs(page, "librarian");
    const seriesPath = await createSeries(page, `XT-Edit-${RUN}`, null);
    await page.locator("#add-expected-tome").click();
    await fillTome(page, 4, `Brouillon ${RUN}`, "");
    await expect(tomeCard(page, `Brouillon ${RUN}`)).toBeVisible();

    await tomeCard(page, `Brouillon ${RUN}`)
      .getByRole("link", { name: /^(edit|modifier)$/i })
      .click();
    await page.waitForURL(/\/expected-tomes\/\d+\/edit$/);
    await expect(page.locator("#expected-tome-position")).toHaveValue("4");
    await fillTome(page, 5, `Final ${RUN}`, "");
    const card = tomeCard(page, `Final ${RUN}`);
    await expect(card).toHaveAttribute("data-position", "5");

    const del = card.getByRole("button", { name: /^(delete|supprimer)$/i });
    await expect(del).not.toHaveAttribute("hx-confirm", /./);
    await del.click();
    const dialog = page.locator("dialog[open]");
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText(`Final ${RUN}`);
    await dialog.locator("[data-modal-confirm]").click();
    await page.waitForURL((url) => url.pathname === seriesPath);
    await expect(page.locator("[data-expected-tome]")).toHaveCount(0);
  });

  test("a refused tome keeps the form with a localized message", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await createSeries(page, `XT-Refuse-${RUN}`, 3);
    await page.locator("#add-expected-tome").click();
    await page.locator("#expected-tome-position").fill("2");
    await page.locator("#expected-tome-title").fill(`Mauvais ${RUN}`);
    await page.locator("#expected-tome-isbn").fill("12345");
    await page.locator('main button[type="submit"]').last().click();
    await expect(page.locator("#expected-tome-error")).toContainText(
      /Invalid ISBN|ISBN invalide/i,
    );
    await expect(page.locator("#expected-tome-title")).toHaveValue(
      `Mauvais ${RUN}`,
    );
  });

  test("lowering the total lists the tome under review", async ({ page }) => {
    await loginAs(page, "librarian");
    const seriesPath = await createSeries(page, `XT-Lower-${RUN}`, 5);
    // "Name this tome" on the gap pre-fills its position.
    await page.locator('[data-name-gap="5"]').click();
    await expect(page.locator("#expected-tome-position")).toHaveValue("5");
    await fillTome(page, null, `Cinquième ${RUN}`, "");
    await expect(tomeCard(page, `Cinquième ${RUN}`)).toBeVisible();

    await page.goto(`${seriesPath}/edit`);
    await page.locator("#series-total").fill("3");
    await page.locator('main button[type="submit"]').last().click();
    await page.waitForURL((url) => url.pathname === seriesPath);

    const review = page.locator("#expected-tomes-review");
    await expect(review).toContainText(`Cinquième ${RUN}`);
    await expect(review).toContainText(
      /above the series total of 3|dépasse le total de la série \(3\)/i,
    );
    await expect(page.locator("[data-expected-tome]")).toHaveCount(0);
  });

  test("an anonymous visitor sees the tome without actions", async ({
    page,
    browser,
  }) => {
    await loginAs(page, "librarian");
    const seriesPath = await createSeries(page, `XT-Anon-${RUN}`, 3);
    await page.locator("#add-expected-tome").click();
    await fillTome(page, 1, `Public ${RUN}`, isbnFor(1));

    const context = await browser.newContext();
    const anon = await context.newPage();
    await anon.goto(seriesPath);
    await expect(tomeCard(anon, `Public ${RUN}`)).toBeVisible();
    await expect(anon.locator("[data-expected-tome-actions]")).toHaveCount(0);
    await expect(anon.locator("#add-expected-tome")).toHaveCount(0);
    await context.close();
  });
});
