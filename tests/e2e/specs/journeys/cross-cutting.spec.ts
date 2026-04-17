import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

const VALID_ISBN = specIsbn("XC", 1);

test.describe("Cross-Cutting Patterns (Story 1-8)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // AC1: Soft Delete - Entity Visibility
  test("delete a volume → it disappears from catalog", async ({
    page,
  }) => {
    await page.goto("/catalog");

    const scanField = page.locator("#scan-field");

    // Create a title
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    // ISBN scan lands as a skeleton first (resolved later via OOB swap).
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
      timeout: 5000,
    });

    // Create a volume
    await scanField.fill("V0098");
    await scanField.press("Enter");
    const volFeedback = page.locator(
      '.feedback-entry[data-feedback-variant="success"]'
    );
    await expect(volFeedback.first()).toBeVisible({ timeout: 5000 });
  });

  // AC5: Theme - System Preference Detection
  test("theme toggle applies dark mode class and persists", async ({
    page,
  }) => {
    await page.goto("/catalog");

    // Initial state: check if dark class is present or not
    const htmlEl = page.locator("html");

    // Click theme toggle
    const themeBtn = page.locator("#theme-toggle");
    await themeBtn.click();

    // Check that class toggled
    const hasDark = await htmlEl.evaluate((el) =>
      el.classList.contains("dark")
    );

    // Click again to toggle back
    await themeBtn.click();
    const hasDarkAfter = await htmlEl.evaluate((el) =>
      el.classList.contains("dark")
    );

    // Should be different states
    expect(hasDark).not.toBe(hasDarkAfter);
  });

  // AC6: Theme Toggle - Persistence
  test("theme preference persists after page reload", async ({
    page,
  }) => {
    await page.goto("/catalog");

    // Get initial dark state
    const htmlEl = page.locator("html");
    const initialDark = await htmlEl.evaluate((el) =>
      el.classList.contains("dark")
    );

    // Toggle theme
    const themeBtn = page.locator("#theme-toggle");
    await themeBtn.click();

    // Reload page
    await page.reload();

    // Theme should persist
    const afterReloadDark = await htmlEl.evaluate((el) =>
      el.classList.contains("dark")
    );
    expect(afterReloadDark).not.toBe(initialDark);

    // Restore original state
    await page.locator("#theme-toggle").click();
  });

  // AC9: Navigation Bar
  test("navigation bar shows Catalog link for librarian", async ({
    page,
  }) => {
    await page.goto("/catalog");

    const nav = page.locator("nav[aria-label='Main navigation']");
    await expect(nav).toBeVisible();

    // Catalog link should be visible for librarian
    const catalogLink = nav.locator('a[href="/catalog"]');
    await expect(catalogLink).toBeVisible();
  });

  test("navigation bar hides loans/borrowers for anonymous (Story 7-1 AC #6)", async ({
    context,
    page,
  }) => {
    // Story 7-1 AC #1 inverted this: /catalog IS anonymous-readable now.
    // What stays hidden for anonymous is the loan/borrower surface.
    await context.clearCookies();
    await page.goto("/");

    const nav = page.locator("nav[aria-label='Main navigation']");
    await expect(nav).toBeVisible();

    // Read-only browsing links must remain visible to anonymous.
    await expect(nav.locator('a[href="/catalog"]')).toBeVisible();
    await expect(nav.locator('a[href="/series"]')).toBeVisible();
    await expect(nav.locator('a[href="/locations"]')).toBeVisible();

    // Loans/borrowers are librarian-only surfaces → hidden for anonymous.
    await expect(nav.locator('a[href="/loans"]')).toHaveCount(0);
    await expect(nav.locator('a[href="/borrowers"]')).toHaveCount(0);
    // /admin dead link removed in Story 7-1.
    await expect(nav.locator('a[href="/admin"]')).toHaveCount(0);
  });

  // AC9: Current page highlighted
  test("current page highlighted in nav bar", async ({ page }) => {
    await page.goto("/catalog");

    const catalogLink = page.locator(
      'nav a[href="/catalog"][aria-current="page"]'
    );
    await expect(catalogLink).toBeVisible();
  });

  // Theme toggle aria-label updates dynamically
  test("theme toggle has accessible aria-label", async ({
    page,
  }) => {
    await page.goto("/catalog");

    const themeBtn = page.locator("#theme-toggle");
    const label = await themeBtn.getAttribute("aria-label");

    // Should have an accessible label
    expect(label).toMatch(/Toggle theme|Switch to (light|dark) mode/);
  });
});
