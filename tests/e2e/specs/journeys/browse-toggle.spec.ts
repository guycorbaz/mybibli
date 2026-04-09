import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

test.describe("Browse List/Grid Toggle (Story 5-6)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
    // Create a title so search has results
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(specIsbn("BT", 1));
    await page.locator("#scan-field").press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");
  });

  test("smoke: toggle visible, switch to grid, reload persists", async ({
    page,
  }) => {
    // Navigate to home with search
    await page.goto(`/?q=${specIsbn("BT", 1)}`);

    // BrowseToggle should be visible
    const toggle = page.locator('[role="radiogroup"]');
    await expect(toggle).toBeVisible({ timeout: 5000 });

    // List mode should be active by default
    const listBtn = page.locator('[data-browse-mode="list"]');
    await expect(listBtn).toHaveAttribute("aria-checked", "true");

    // Results container should have browse-list class
    const container = page.locator("#browse-results");
    await expect(container).toHaveClass(/browse-list/);

    // Click grid button
    const gridBtn = page.locator('[data-browse-mode="grid"]');
    await gridBtn.click();

    // Container should switch to browse-grid
    await expect(container).toHaveClass(/browse-grid/);
    await expect(gridBtn).toHaveAttribute("aria-checked", "true");
    await expect(listBtn).toHaveAttribute("aria-checked", "false");

    // Reload page — preference should persist via localStorage
    await page.goto(`/?q=${specIsbn("BT", 1)}`);
    const containerAfterReload = page.locator("#browse-results");
    // browse-mode.js applies the saved preference on DOMContentLoaded
    await expect(containerAfterReload).toHaveClass(/browse-grid/, {
      timeout: 3000,
    });

    // Clean up localStorage for other tests
    await page.evaluate(() =>
      localStorage.removeItem("mybibli_browse_mode"),
    );
  });

  test("title cards are rendered as articles", async ({ page }) => {
    await page.goto(`/?q=${specIsbn("BT", 1)}`);

    // Should have at least one title card
    const cards = page.locator("article.title-card");
    await expect(cards.first()).toBeVisible({ timeout: 5000 });

    // Card should have a link to title detail
    const link = cards.first().locator("a.title-card-link");
    await expect(link).toBeVisible();
  });

  test("ARIA: radiogroup with proper roles", async ({ page }) => {
    await page.goto(`/?q=${specIsbn("BT", 1)}`);

    const radiogroup = page.locator('[role="radiogroup"]');
    await expect(radiogroup).toBeVisible();
    await expect(radiogroup).toHaveAttribute(
      "aria-label",
      /Display mode|Mode d'affichage/i,
    );

    const radios = radiogroup.locator('[role="radio"]');
    await expect(radios).toHaveCount(2);
  });
});
