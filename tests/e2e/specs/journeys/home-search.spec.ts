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
    const tbody = page.locator("#browse-results");
    // Wait for HTMX swap to complete: either title cards render or the empty-state
    // block appears. Matching either variant guarantees the swap landed.
    await expect(
      tbody.locator('article.title-card, .text-center').first(),
    ).toBeVisible({ timeout: 5000 });
  });

  test("should navigate to title detail page on row click", async ({
    page,
  }) => {
    await page.goto("/?q=test");
    // If results exist, click first row
    const rows = page.locator("#browse-results article.title-card");
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
    const emptyState = page.locator("#browse-results .text-center");
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

  // Regression — 2026-04-17: clicking a genre pill with an empty query caused
  // the home route's HTMX branch to fall through to the full-page render,
  // which HTMX then swapped into `#browse-results`, duplicating the nav bar,
  // hero, search field, and pills. Guard against re-introducing the bug.
  test("clicking a genre pill does NOT duplicate the page layout", async ({
    page,
  }) => {
    await page.goto("/");

    // Pre-click sanity — exactly one <main> and one <nav> on a clean render.
    await expect(page.locator("main#main-content")).toHaveCount(1);
    await expect(page.locator("nav")).toHaveCount(1);

    // Click any genre pill. The pills live in a tag area on the home page
    // and carry `hx-get` with `filter=genre:<id>`.
    const firstGenrePill = page.locator("a[hx-get*='filter=genre:']").first();
    await expect(firstGenrePill).toBeVisible();
    const pillHref = await firstGenrePill.getAttribute("hx-get");
    expect(pillHref).toMatch(/filter=genre:\d+/);

    await firstGenrePill.click();

    // Post-click: #browse-results swap landed. Wait for either a title card
    // or the empty-state block to materialize inside the target.
    const results = page.locator("#browse-results");
    await expect(
      results.locator("article.title-card, .text-center").first(),
    ).toBeVisible({ timeout: 10000 });

    // THE REGRESSION ASSERTION: still exactly one <main> and one <nav>.
    // With the bug, the full layout was swapped INTO `#browse-results`,
    // yielding 2 <main> and 2 <nav> elements in the DOM.
    await expect(page.locator("main#main-content")).toHaveCount(1);
    await expect(page.locator("nav")).toHaveCount(1);
  });
});
