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

    // The three count rows are rendered (regex tolerates EN/FR plurals).
    await expect(card).toContainText(/\d+\s+(titles?|titres?)/i);
    await expect(card).toContainText(/\d+\s+volumes?/i);
    await expect(card).toContainText(
      /\d+\s+(active loans?|prêts? en cours)/i,
    );

    // CRITICAL: anonymous render must NOT leak the /loans link anywhere.
    await expect(page.locator('a[href="/loans"]')).toHaveCount(0);

    // The aria-describedby target span exists (screen-reader sign-in hint).
    await expect(card.locator("#glance-loans-hint")).toBeAttached();
  });

  test("librarian: card shows /loans link, click navigates to /loans", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/");

    const card = page.locator("#collection-glance");
    await expect(card).toBeVisible();

    // The loan-count line is now a real link to /loans (exactly one).
    const loansLink = card.locator('a[href="/loans"]');
    await expect(loansLink).toHaveCount(1);

    await loansLink.click();
    await page.waitForURL(/\/loans/);
  });
});
