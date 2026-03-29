import { test, expect } from "@playwright/test";

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
    const color = await h1.evaluate(
      (el) => getComputedStyle(el).color,
    );
    // Indigo color should be applied (not default black)
    expect(color).not.toBe("rgb(0, 0, 0)");
  });
});
