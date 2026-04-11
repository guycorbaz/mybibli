import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

test.describe("Series CRUD & Listing (Story 5-3)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // AC8: E2E smoke — create closed series → visit detail → edit → verify persistence
  test("smoke: create closed series, visit detail, edit name, verify persistence", async ({
    page,
  }) => {
    const SERIES_NAME = `SE-Test-${Date.now()}`;
    const EDITED_NAME = `${SERIES_NAME}-Edited`;

    // Navigate to series list
    await page.goto("/series");
    await expect(page.locator("h1")).toContainText(/Series|Séries/i);

    // Click "Add series" button
    const addBtn = page.getByRole("link", { name: /add|ajouter/i });
    await expect(addBtn).toBeVisible();
    await addBtn.click();
    await page.waitForURL("**/series/new");

    // Fill create form
    await page.locator("#series-name").fill(SERIES_NAME);
    await page.locator("#series-type").selectOption("closed");
    await page.locator("#series-total").fill("10");
    await page.locator('button[type="submit"]').click();

    // Should redirect to detail page
    await page.waitForURL(/\/series\/\d+/);
    await expect(page.locator("h1")).toContainText(SERIES_NAME);

    // Verify detail shows correct type and stats
    await expect(page.getByText(/closed|fermée/i)).toBeVisible();
    // Total should be 10
    await expect(
      page.getByText(/Total volumes.*10|Nombre total.*10/i),
    ).toBeVisible();

    // Click edit
    const editLink = page.getByRole("link", { name: /edit|modifier/i });
    await editLink.click();
    await page.waitForURL(/\/series\/\d+\/edit/);

    // Change name
    await page.locator("#series-name").fill(EDITED_NAME);
    await page.locator('button[type="submit"]').click();

    // Should redirect back to detail with updated name
    await page.waitForURL(/\/series\/\d+$/);
    await expect(page.locator("h1")).toContainText(EDITED_NAME);

    // Go back to list and verify updated name appears
    await page.goto("/series");
    await expect(page.getByText(EDITED_NAME)).toBeVisible();
  });

  // AC5: Anonymous access — public read
  test("anonymous user can access series list", async ({ context, page }) => {
    await context.clearCookies();
    await page.goto("/series");
    // Should NOT redirect to login
    expect(page.url()).toContain("/series");
    await expect(page.locator("h1")).toContainText(/Series|Séries/i);
  });

  // Delete test
  test("delete series removes it from list", async ({ page }) => {
    const SERIES_NAME = `SE-Delete-${Date.now()}`;

    // Create a series first
    await page.goto("/series/new");
    await page.locator("#series-name").fill(SERIES_NAME);
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(/\/series\/\d+/);

    // Set up dialog handler for hx-confirm
    page.on("dialog", (d) => d.accept());

    // Click delete button
    const deleteBtn = page.getByRole("button", {
      name: /delete|supprimer/i,
    });
    await expect(deleteBtn).toBeVisible();
    await deleteBtn.click();

    // Should redirect to series list
    await page.waitForURL("**/series", { timeout: 5000 });

    // Series should no longer appear in list
    await expect(page.getByText(SERIES_NAME)).not.toBeVisible();
  });
});

test.describe("Series Assignment & Gap Detection (Story 5-4)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // AC8: Create closed series → assign titles → verify gap grid
  test("smoke: assign titles to series and verify gap grid", async ({
    page,
  }) => {
    const SERIES_NAME = `SE-Gap-${Date.now()}`;
    // Step 1: Create a closed series with total=5
    await page.goto("/series/new");
    await page.locator("#series-name").fill(SERIES_NAME);
    await page.locator("#series-type").selectOption("closed");
    await page.locator("#series-total").fill("5");
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(/\/series\/\d+/);
    const seriesUrl = page.url();

    // Step 2: Create 2 titles via scan on catalog page
    // Scan ISBN 1
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(specIsbn("SE", 10));
    await page.locator("#scan-field").press("Enter");
    // Wait for the feedback entry (not just skeleton) to ensure title is created
    await expect(page.locator(".feedback-entry, .feedback-skeleton")).toBeVisible({ timeout: 10000 });

    // Scan ISBN 2
    await page.locator("#scan-field").fill(specIsbn("SE", 11));
    await page.locator("#scan-field").press("Enter");
    await expect(page.locator(".feedback-entry, .feedback-skeleton").last()).toBeVisible({ timeout: 10000 });

    // Step 3: Find title 1 via home search — navigate with query param
    // The title is created during scan, so it should be searchable immediately
    await page.goto(`/?q=${specIsbn("SE", 10)}`);
    const title1Link = page.locator("a[href^='/title/']").first();
    await expect(title1Link).toBeVisible({ timeout: 15000 });
    const title1Href = (await title1Link.getAttribute("href"))!;
    await page.goto(title1Href);
    await page.waitForURL(/\/title\/\d+/);
    const title1Url = page.url();

    // Assign title 1 to series at position 1
    await page.locator("#assign-series").selectOption({ label: SERIES_NAME });
    await page.locator("#assign-position").fill("1");
    await page.locator("#assign-series-submit").click();
    await page.waitForURL(/\/title\/\d+/);

    // Verify assignment appears (use link selector to avoid matching the dropdown option)
    await expect(
      page.locator(`a[href^="/series/"]:has-text("${SERIES_NAME}")`),
    ).toBeVisible();

    // Step 4: Find title 2 via home search and navigate to detail
    await page.goto(`/?q=${specIsbn("SE", 11)}`);
    const title2Link = page.locator("a[href^='/title/']").first();
    await expect(title2Link).toBeVisible({ timeout: 10000 });
    const title2Href = (await title2Link.getAttribute("href"))!;
    await page.goto(title2Href);
    await page.waitForURL(/\/title\/\d+/);

    // Assign title 2 at position 3
    await page.locator("#assign-series").selectOption({ label: SERIES_NAME });
    await page.locator("#assign-position").fill("3");
    await page.locator("#assign-series-submit").click();
    await page.waitForURL(/\/title\/\d+/);

    // Step 5: Navigate to series detail and verify gap grid
    await page.goto(seriesUrl);
    await expect(page.locator("h1")).toContainText(SERIES_NAME);

    // Gap grid should be visible
    const grid = page.locator('[role="grid"]');
    await expect(grid).toBeVisible({ timeout: 5000 });

    // Should have 5 cells (positions 1-5)
    const cells = grid.locator('[role="gridcell"]');
    await expect(cells).toHaveCount(5);

    // Positions 1 and 3 should be filled (links)
    const filledCells = grid.locator("a[role='gridcell']");
    await expect(filledCells).toHaveCount(2);

    // Positions 2, 4, 5 should be missing (divs, not links)
    const missingCells = grid.locator("div[role='gridcell']");
    await expect(missingCells).toHaveCount(3);
  });

  // AC3: Click filled square navigates to title
  test("clicking filled square navigates to title detail", async ({
    page,
  }) => {
    const SERIES_NAME = `SE-Click-${Date.now()}`;
    const ISBN = specIsbn("SE", 12);

    // Create series
    await page.goto("/series/new");
    await page.locator("#series-name").fill(SERIES_NAME);
    await page.locator("#series-type").selectOption("closed");
    await page.locator("#series-total").fill("3");
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(/\/series\/\d+/);
    const seriesUrl = page.url();

    // Create title via scan
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(ISBN);
    await page.locator("#scan-field").press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    // Find the title via home search
    await page.goto(`/?q=${ISBN}`);
    const titleLink = page.locator("a[href^='/title/']").first();
    await expect(titleLink).toBeVisible({ timeout: 10000 });
    const titleHref = (await titleLink.getAttribute("href"))!;
    await page.goto(titleHref);
    await page.waitForURL(/\/title\/\d+/);

    // Assign to series at position 2
    await page.locator("#assign-series").selectOption({ label: SERIES_NAME });
    await page.locator("#assign-position").fill("2");
    await page.locator("#assign-series-submit").click();
    await page.waitForURL(/\/title\/\d+/);

    // Go to series detail and click filled square
    await page.goto(seriesUrl);
    const filledSquare = page.locator("a[role='gridcell']").first();
    await expect(filledSquare).toBeVisible();
    await filledSquare.click();

    // Should navigate to title detail
    await page.waitForURL(/\/title\/\d+/);
  });

  // AC6: Omnibus covering 3 positions fills gap grid
  test("omnibus assignment fills multiple positions in gap grid", async ({
    page,
  }) => {
    const SERIES_NAME = `SE-Omni-${Date.now()}`;

    // Create closed series with total=8
    await page.goto("/series/new");
    await page.locator("#series-name").fill(SERIES_NAME);
    await page.locator("#series-type").selectOption("closed");
    await page.locator("#series-total").fill("8");
    await page.locator('button[type="submit"]').click();
    await page.waitForURL(/\/series\/\d+/);
    const seriesUrl = page.url();

    // Create a title via scan
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(specIsbn("SE", 20));
    await page.locator("#scan-field").press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    // Find title via home search
    await page.goto(`/?q=${specIsbn("SE", 20)}`);
    const titleLink = page.locator("a[href^='/title/']").first();
    await expect(titleLink).toBeVisible({ timeout: 10000 });
    const titleHref = (await titleLink.getAttribute("href"))!;
    await page.goto(titleHref);
    await page.waitForURL(/\/title\/\d+/);

    // Assign as omnibus positions 3-5
    await page.locator("#assign-series").selectOption({ label: SERIES_NAME });
    await page.locator("#assign-position").fill("3");
    await page.locator("#assign-omnibus").check();
    await page.locator("#assign-end-position").fill("5");
    await page.locator("#assign-series-submit").click();
    await page.waitForURL(/\/title\/\d+/);

    // Verify assignment shows as range
    await expect(
      page.locator(`a[href^="/series/"]:has-text("${SERIES_NAME}")`),
    ).toBeVisible();
    await expect(page.getByText("#3-5").first()).toBeVisible();

    // Navigate to series detail and verify gap grid
    await page.goto(seriesUrl);
    const grid = page.locator('[role="grid"]');
    await expect(grid).toBeVisible({ timeout: 5000 });

    // 8 cells total
    const cells = grid.locator('[role="gridcell"]');
    await expect(cells).toHaveCount(8);

    // 3 filled (positions 3,4,5)
    const filledCells = grid.locator("a[role='gridcell']");
    await expect(filledCells).toHaveCount(3);

    // 5 missing (positions 1,2,6,7,8)
    const missingCells = grid.locator("div[role='gridcell']");
    await expect(missingCells).toHaveCount(5);
  });
});
