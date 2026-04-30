import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

test.describe("Home page", () => {
  test("should display mybibli title", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("h1")).toContainText("mybibli");
  });

  test("should have correct page title", async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveTitle("mybibli");
  });

  test("should load Tailwind CSS styles", async ({ page }) => {
    await page.goto("/");
    const h1 = page.locator("h1");
    const color = await h1.evaluate((el) => getComputedStyle(el).color);
    // Indigo color should be applied (not default black)
    expect(color).not.toBe("rgb(0, 0, 0)");
  });
});

// Story 9-1 — "Collection at a glance" card.
test.describe("Home page — Collection at a glance card", () => {
  test("anonymous: card visible, three counts, loan count is NOT a link", async ({
    page,
  }) => {
    await page.goto("/");

    // Card section is present and labelled by its heading.
    const card = page.locator("#collection-glance");
    await expect(card).toBeVisible();
    await expect(
      card.getByRole("heading", { level: 2 }),
    ).toContainText(/Collection at a glance|Aperçu de la collection/i);

    // The three count rows are rendered IN ORDER (titles, volumes, loans) —
    // scope each assertion to its own <li> so a label/order swap regression
    // would fail (a global toContainText would silently match the wrong row).
    const rows = card.locator("ul > li");
    await expect(rows).toHaveCount(3);
    await expect(rows.nth(0)).toContainText(/\d+\s+(titles?|titres?)/i);
    await expect(rows.nth(1)).toContainText(/\d+\s+volumes?/i);
    await expect(rows.nth(2)).toContainText(
      /\d+\s+(active loans?|prêts? en cours)/i,
    );

    // CRITICAL: anonymous render must NOT leak the /loans link anywhere on the page.
    await expect(page.locator('a[href="/loans"]')).toHaveCount(0);

    // The aria-describedby target span exists (screen-reader sign-in hint),
    // and its anchor span carries the matching reference inside the card.
    await expect(card.locator("#glance-loans-hint")).toBeAttached();
    await expect(
      card.locator('[aria-describedby="glance-loans-hint"]'),
    ).toHaveCount(1);
  });

  test("librarian: card shows /loans link, click navigates to /loans", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/");

    const card = page.locator("#collection-glance");
    await expect(card).toBeVisible();

    // The loan-count line is the third row and a real link to /loans
    // (exactly one inside the card; the nav bar also has one but that's
    // outside the card scope).
    const rows = card.locator("ul > li");
    await expect(rows).toHaveCount(3);
    await expect(rows.nth(2)).toContainText(
      /\d+\s+(active loans?|prêts? en cours)/i,
    );

    const loansLink = card.locator('a[href="/loans"]');
    await expect(loansLink).toHaveCount(1);
    await expect(loansLink).toContainText(
      /active loans?|prêts? en cours/i,
    );

    await loansLink.click();
    await page.waitForURL(/\/loans/);
  });
});
