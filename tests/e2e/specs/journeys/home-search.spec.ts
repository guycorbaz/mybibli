import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { simulateScan, simulateTyping } from "../../helpers/scanner";
import { scanTitleAndVolume } from "../../helpers/loans";
import { createLocation } from "../../helpers/locations";

const SPEC_ID = "HS";

/**
 * Per-test unique CHAR(5) label generator (V0001 … V0999 …).
 * Combines a process-local counter with `Date.now() % 1000` so two
 * parallel tests in the same wall-clock millisecond can't collide on
 * the UNIQUE constraint of `volumes.label` / `storage_locations.label`.
 * Story-9-9 review fix — the prior `Date.now() % 10000` formula was
 * collision-prone under `fullyParallel: true`.
 */
let labelCounter = 0;
function uniqueLabel(prefix: "V" | "L"): string {
  labelCounter = (labelCounter + 1) % 100;
  const counterPart = labelCounter.toString().padStart(2, "0");
  const timePart = (Date.now() % 100).toString().padStart(2, "0");
  return `${prefix}${counterPart}${timePart}`;
}

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
    // Wait for HTMX swap to complete: either a result row (CR #250
    // list-mode <table> or the legacy `.browse-cards` grid markup)
    // renders, or the empty-state block appears. Matching either
    // variant guarantees the swap landed.
    await expect(
      tbody
        .locator(
          'table.browse-table tbody tr, article.title-card, .text-center',
        )
        .first(),
    ).toBeVisible({ timeout: 5000 });
  });

  test("should navigate to title detail page on row click", async ({
    page,
  }) => {
    await page.goto("/?q=test");
    // CR #250 — default browse mode is list = sortable table. Click
    // the first table-row link; sibling .title-card markup exists in
    // the DOM for grid mode but `display: none` in list mode, so
    // `.first()` on a card would auto-wait on a hidden element.
    const rows = page.locator(
      "#browse-results table.browse-table tbody tr td a[href^='/title/']",
    );
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
    // Story 9-15 — empty state now uses the StatusMessage component with
    // EN copy "No matches" / FR "Aucun résultat" (encouraging-tone rewrite).
    const emptyState = page.locator(
      "#browse-results [data-status-message][data-variant='empty']",
    );
    await expect(emptyState).toBeVisible();
    await expect(emptyState).toContainText(/No matches|Aucun résultat/i);
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

    // Post-click: #browse-results swap landed. Wait for a result row
    // (CR #250 list-mode table OR legacy grid-mode card) or the
    // empty-state block to materialize inside the target.
    const results = page.locator("#browse-results");
    await expect(
      results
        .locator(
          "table.browse-table tbody tr, article.title-card, .text-center",
        )
        .first(),
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
    // Per story-9-8 catch + 9-9 review fix — V-code derived from
    // `uniqueLabel("V")` which combines a per-test counter with
    // `Date.now() % 1000` so two parallel tests in the same wall-clock
    // ms can't collide on the UNIQUE constraint of `volumes.label`.
    const vcode = uniqueLabel("V");
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
    // L-code via `uniqueLabel("L")` for the same parallel-collision
    // resistance as V-codes. CHAR(5) constrained.
    const lcode = uniqueLabel("L");
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
    // away from /. Match a result row (CR #250 table OR legacy card)
    // or the empty-state block.
    const tbody = page.locator("#browse-results");
    await expect(
      tbody
        .locator(
          "table.browse-table tbody tr, article.title-card, .text-center",
        )
        .first(),
    ).toBeVisible({ timeout: 5000 });

    // Critical: still on home (not /title/, /volume/, /location/, /catalog).
    // Path-and-param assertion (NOT byte equality) — hx-include on the
    // search field appends filter=, sort=, dir= to the pushed URL, so we
    // can't pin the full querystring.
    //
    // Use `expect.poll` here instead of a one-shot read: under the
    // larger #315 dual-wrapper search-fragment payload (v1.7.2+), HTMX's
    // hx-push-url update for the LAST debounced search-fire occasionally
    // lands a few ms AFTER the visible-row assertion settled, so a
    // single `expect(...).toBe(...)` could see the URL still at an
    // intermediate `q=tes` state. Polling lets the URL converge to the
    // final `q=test` without changing semantics.
    await expect
      .poll(() => new URL(page.url()).pathname, { timeout: 5000 })
      .toBe("/");
    // Fix #196 (v1.7.9): bumped 2 s → 5 s alongside the simulateTyping
    // helper switch (pressSequentially → keyboard.type) to give the
    // debounced search + HTMX swap + hx-push-url chain headroom under
    // default-worker parallelism. The previous 2 s ceiling occasionally
    // observed `q=tes` (last keystroke dropped by pressSequentially) and
    // gave up before the URL converged.
    await expect
      .poll(() => new URL(page.url()).searchParams.get("q"), {
        timeout: 5000,
      })
      .toBe("test");
  });

  /**
   * AC12 Test 4 — Escape clears the input AND resets the state machine.
   * Per `static/js/search.js:34-39`, the Escape branch zeroes the field
   * value, transitions to IDLE, cancels any pending debounce, and clears
   * the polite aria-live region. Locks the contract end-to-end so a
   * future regression that drops Escape handling fails the build.
   */
  test("pressing Escape clears the field and resets the state machine", async ({
    page,
  }) => {
    await page.goto("/");

    // Type a couple of chars (slow) to drive SEARCH_MODE.
    await simulateTyping(page, "#search-field", "abc");
    await expect(page.locator("#search-field")).toHaveValue("abc");

    // Press Escape and re-assert: field empty + announcement region empty.
    await page.locator("#search-field").press("Escape");
    await expect(page.locator("#search-field")).toHaveValue("");
    await expect(
      page.locator("#search-state-announcement"),
    ).toHaveText("");

    // State-machine reset proof: a subsequent fast scanner burst on the
    // same field must still classify as SCAN_PENDING and trigger /scan.
    // We use an unknown ISBN so the redirect target is /catalog?code=…
    // (no DB seeding needed).
    const isbn = specIsbn(SPEC_ID, 92);
    await simulateScan(page, "#search-field", isbn);
    await page.waitForURL(new RegExp(`/catalog\\?code=${isbn}`), {
      timeout: 5000,
    });
  });
});
