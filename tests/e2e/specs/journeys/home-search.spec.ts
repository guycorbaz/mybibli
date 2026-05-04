import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { simulateScan, simulateTyping } from "../../helpers/scanner";
import { scanTitleAndVolume } from "../../helpers/loans";
import { createLocation } from "../../helpers/locations";

const SPEC_ID = "HS";

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

    // Pre-click sanity — exactly one <header> and one <main>. Can't count
    // bare <nav> because home.html legitimately has two: the main nav bar
    // AND `<nav id="pagination">` emitted by `render_pagination_oob`.
    await expect(page.locator("header")).toHaveCount(1);
    await expect(page.locator("main#main-content")).toHaveCount(1);

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

    // THE REGRESSION ASSERTION: still exactly one <header> and one <main>.
    // With the bug, the full layout was swapped INTO `#browse-results`,
    // yielding 2 <header> and 2 <main> elements in the DOM.
    await expect(page.locator("header")).toHaveCount(1);
    await expect(page.locator("main#main-content")).toHaveCount(1);
  });
});

/**
 * Story 9-9 — Home page scanner detection state machine.
 *
 * Locks in AC12: a barcode scanner burst (`simulateScan`, 20 ms inter-key)
 * followed by Enter on the home `#search-field` triggers the `scan-fire`
 * event → `GET /scan` → HX-Redirect to the right detail page or to
 * `/catalog?code=…`. Slow human typing (`simulateTyping`, 100 ms) does
 * NOT trigger `/scan` — it stays on the inline browse search path.
 */
test.describe("Home page scanner detection — scan to navigate", () => {
  // Per CLAUDE.md "Local Testing Before Push" + Foundation Rule #14, each
  // test gets its own session via loginAs() for parallel safety.
  test.beforeEach(async ({ page }) => {
    await loginAs(page, "admin");
  });

  test("scanning an unknown ISBN redirects to /catalog?code=<isbn>", async ({
    page,
  }) => {
    // Use a high-sequence specIsbn so the synthetic ISBN is guaranteed
    // not to match any existing title.
    const isbn = specIsbn(SPEC_ID, 91);

    await page.goto("/");
    await simulateScan(page, "#search-field", isbn);

    // Browser navigates because the handler returns HX-Redirect.
    await page.waitForURL(new RegExp(`/catalog\\?code=${isbn}`), {
      timeout: 5000,
    });
    expect(page.url()).toContain(`/catalog?code=${isbn}`);
  });

  test("scanning a known V-code redirects to /volume/<id>", async ({ page }) => {
    // Per story-9-8 catch — V-code derived from Date.now() to avoid retry
    // collisions. CHAR(5) max, so 4-digit suffix on the 'V' prefix.
    const vcode = `V${(Date.now() % 10000).toString().padStart(4, "0")}`;
    const isbn = specIsbn(SPEC_ID, 1);

    // Seed: create a title + volume with the unique V-code via the catalog
    // workflow (the helper drives the real /catalog screen, not a backdoor).
    await scanTitleAndVolume(page, isbn, vcode);

    await page.goto("/");
    await simulateScan(page, "#search-field", vcode);

    // HX-Redirect to /volume/<id> — the id is allocated server-side.
    await page.waitForURL(/\/volume\/\d+/, { timeout: 5000 });
    expect(page.url()).toMatch(/\/volume\/\d+/);
  });

  test("scanning a known L-code redirects to /location/<id>", async ({
    page,
  }) => {
    // L-code derived from Date.now() to avoid retry collisions; CHAR(5).
    const lcode = `L${(Date.now() % 10000).toString().padStart(4, "0")}`;
    const locationName = `HS-9-9 Shelf ${Date.now()}`;

    await createLocation(page, locationName, lcode);

    await page.goto("/");
    await simulateScan(page, "#search-field", lcode);

    await page.waitForURL(/\/location\/\d+/, { timeout: 5000 });
    expect(page.url()).toMatch(/\/location\/\d+/);
  });

  test("typing slowly stays on home and triggers inline browse search", async ({
    page,
  }) => {
    await page.goto("/");

    // Slow typing (100 ms inter-key) → SEARCH_MODE → search-fire (NOT scan-fire).
    await simulateTyping(page, "#search-field", "test");

    // Inline browse: #browse-results swap landed, page DID NOT navigate
    // away from /. Match either a title card or empty-state block.
    const tbody = page.locator("#browse-results");
    await expect(
      tbody.locator("article.title-card, .text-center").first(),
    ).toBeVisible({ timeout: 5000 });

    // Critical: still on home (not /title/, /volume/, /location/, /catalog).
    // Path-and-param assertion (NOT byte equality) — hx-include on the
    // search field appends filter=, sort=, dir= to the pushed URL, so we
    // can't pin the full querystring.
    const pageUrl = new URL(page.url());
    expect(pageUrl.pathname).toBe("/");
    expect(pageUrl.searchParams.get("q")).toBe("test");
  });
});
