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
    await page.locator('main button[type="submit"]').last().click();

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
    await page.locator('main button[type="submit"]').last().click();

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

  // Delete test (story 9-13: migrated from hx-confirm to UX-DR8 Modal)
  test("delete series removes it from list", async ({ page }) => {
    const SERIES_NAME = `SE-Delete-${Date.now()}`;

    // Create a series first
    await page.goto("/series/new");
    await page.locator("#series-name").fill(SERIES_NAME);
    await page.locator('main button[type="submit"]').last().click();
    await page.waitForURL(/\/series\/\d+/);

    // Click delete button → modal opens (no native confirm dialog any more)
    const deleteBtn = page.getByRole("button", {
      name: /delete|supprimer/i,
    });
    await expect(deleteBtn).toBeVisible();

    // Paranoid lock — trigger button must NOT carry hx-confirm
    // (story 9-13 migration; covers regression beyond the audit's count check)
    await expect(deleteBtn).not.toHaveAttribute("hx-confirm", /./);

    // First click: open modal, verify default focus, press Escape, verify close
    await deleteBtn.click();
    await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
    await expect(page.locator("[data-modal-default-focus]")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible();

    // Re-open and confirm the actual delete
    await deleteBtn.click();
    await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
    await page.locator("[data-modal-confirm]").click();

    // Should redirect to series list
    await page.waitForURL("**/series", { timeout: 5000 });

    // Series should no longer appear in list
    await expect(page.getByText(SERIES_NAME)).not.toBeVisible();
  });

  // Story 9-13 conflict path — verifies the meaningful series.delete_has_titles
  // copy is surfaced (not the generic error.internal copy). 1.2.1 fix for #139.
  test("delete series with assigned titles shows block message", async ({
    page,
  }) => {
    const SERIES_NAME = `SE-DeleteConflict-${Date.now()}`;
    // Picked seq=30 to avoid collision with existing assignments at 10/11/12/20.
    const ISBN = specIsbn("SE", 30);

    // Step 1: create series
    await page.goto("/series/new");
    await page.locator("#series-name").fill(SERIES_NAME);
    await page.locator('main button[type="submit"]').last().click();
    await page.waitForURL(/\/series\/\d+/);
    const seriesUrl = page.url();

    // Step 2: create a title via scan
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(ISBN);
    await page.locator("#scan-field").press("Enter");
    await expect(
      page.locator(".feedback-entry, .feedback-skeleton"),
    ).toBeVisible({ timeout: 10000 });

    // Step 3: navigate to the title via home search
    await page.goto(`/?q=${ISBN}`);
    // CR #250 — scope to the visible list-mode table row link;
    // `.browse-cards` markup is in the DOM but hidden in list mode.
    const titleLink = page
      .locator(
        "#browse-results table.browse-table tbody tr td a[href^='/title/']",
      )
      .first();
    await expect(titleLink).toBeVisible({ timeout: 15000 });
    const titleHref = (await titleLink.getAttribute("href"))!;
    await page.goto(titleHref);
    await page.waitForURL(/\/title\/\d+/);

    // Step 4: assign the title to the series at position 1
    await page.locator("#assign-series").selectOption({ label: SERIES_NAME });
    await page.locator("#assign-position").fill("1");
    await page.locator("#assign-series-submit").click();
    await page.waitForURL(/\/title\/\d+/);

    // Step 5: navigate back to the series detail page
    await page.goto(seriesUrl);

    // Open the delete modal
    const deleteBtn = page.getByRole("button", {
      name: /delete|supprimer/i,
    });
    await deleteBtn.click();
    await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();

    // Confirm — server returns 200 + inline feedback HTML; modal closes via
    // modal.js's `htmx:afterRequest` listener on 2xx.
    await page.locator("[data-modal-confirm]").click();

    // Modal closes after the 200 response
    await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible();

    // #series-feedback contains the meaningful series.delete_has_titles copy
    await expect(page.locator("#series-feedback")).toContainText(
      /title\(s\) assigned|titre\(s\) assigné/i,
    );

    // No redirect — the series was NOT deleted; URL stays on /series/:id
    await expect(page).toHaveURL(seriesUrl);
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
    await page.locator('main button[type="submit"]').last().click();
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
    const title1Link = page
      .locator(
        "#browse-results table.browse-table tbody tr td a[href^='/title/']",
      )
      .first();
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
    const title2Link = page
      .locator(
        "#browse-results table.browse-table tbody tr td a[href^='/title/']",
      )
      .first();
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
    await page.locator('main button[type="submit"]').last().click();
    await page.waitForURL(/\/series\/\d+/);
    const seriesUrl = page.url();

    // Create title via scan
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(ISBN);
    await page.locator("#scan-field").press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    // Find the title via home search
    await page.goto(`/?q=${ISBN}`);
    // CR #250 — scope to the visible list-mode table row link;
    // `.browse-cards` markup is in the DOM but hidden in list mode.
    const titleLink = page
      .locator(
        "#browse-results table.browse-table tbody tr td a[href^='/title/']",
      )
      .first();
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
    await page.locator('main button[type="submit"]').last().click();
    await page.waitForURL(/\/series\/\d+/);
    const seriesUrl = page.url();

    // Create a title via scan
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(specIsbn("SE", 20));
    await page.locator("#scan-field").press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    // Find title via home search
    await page.goto(`/?q=${specIsbn("SE", 20)}`);
    // CR #250 — scope to the visible list-mode table row link;
    // `.browse-cards` markup is in the DOM but hidden in list mode.
    const titleLink = page
      .locator(
        "#browse-results table.browse-table tbody tr td a[href^='/title/']",
      )
      .first();
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
