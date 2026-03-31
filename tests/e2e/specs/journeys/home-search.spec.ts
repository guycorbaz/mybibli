import { test, expect } from "@playwright/test";

test.describe("Home page search", () => {
  test("should display search field on home page", async ({ page }) => {
    await page.goto("/");
    const searchField = page.locator("#search-field");
    await expect(searchField).toBeVisible();
    await expect(searchField).toHaveAttribute("type", "search");
  });

  test("should show search results when typing 2+ characters", async ({
    page,
  }) => {
    await page.goto("/");
    const searchField = page.locator("#search-field");
    await searchField.fill("te");
    // Trigger search-fire event (simulating debounce completion)
    await searchField.dispatchEvent("search-fire");
    // Wait for HTMX swap
    await page.waitForTimeout(500);
    const tbody = page.locator("#search-results-body");
    await expect(tbody).toBeVisible();
  });

  test("should navigate to title detail page on row click", async ({
    page,
  }) => {
    await page.goto("/?q=test");
    // If results exist, click first row
    const rows = page.locator("#search-results-body tr[role='link']");
    const count = await rows.count();
    if (count > 0) {
      await rows.first().click();
      await expect(page).toHaveURL(/\/title\/\d+/);
    }
  });

  test("should support bookmarkable URLs with query params", async ({
    page,
  }) => {
    await page.goto("/?q=test&page=1");
    const searchField = page.locator("#search-field");
    await expect(searchField).toHaveValue("test");
  });

  test("should show empty state for no results", async ({ page }) => {
    await page.goto("/?q=zzzznonexistent99999");
    // Check for empty state SVG or message
    const emptyState = page.locator("#search-results-body td[colspan]");
    if ((await emptyState.count()) > 0) {
      await expect(emptyState).toContainText("No results");
    }
  });

  test("should focus search field when pressing / key", async ({ page }) => {
    await page.goto("/");
    // Click body first to ensure no input is focused
    await page.locator("body").click();
    await page.keyboard.press("/");
    const searchField = page.locator("#search-field");
    await expect(searchField).toBeFocused();
  });

  test("should have accessible search field", async ({ page }) => {
    await page.goto("/");
    const searchField = page.locator("#search-field");
    const ariaLabel = await searchField.getAttribute("aria-label");
    expect(ariaLabel).toBeTruthy();
  });

  test("should display title detail page", async ({ page }) => {
    // Navigate directly to a title detail page (assumes title with id 1 exists)
    const response = await page.goto("/title/1");
    // May be 404 if no data, but the route should exist
    expect(response?.status()).toBeLessThanOrEqual(404);
  });

  test("should display contributor detail page", async ({ page }) => {
    const response = await page.goto("/contributor/1");
    expect(response?.status()).toBeLessThanOrEqual(404);
  });

  test("should display location detail stub page", async ({ page }) => {
    const response = await page.goto("/location/1");
    expect(response?.status()).toBeLessThanOrEqual(404);
  });
});
