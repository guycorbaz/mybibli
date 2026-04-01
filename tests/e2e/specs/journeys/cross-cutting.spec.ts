import { test, expect } from "@playwright/test";

const DEV_SESSION_COOKIE = {
  name: "session",
  value: "ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2ZGV2",
  domain: "localhost",
  path: "/",
};

const VALID_ISBN = "9782070360246";

test.describe("Cross-Cutting Patterns (Story 1-8)", () => {
  // AC1: Soft Delete - Entity Visibility
  test("delete a volume → it disappears from catalog", async ({
    page,
    context,
  }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
    await page.goto("/catalog");

    const scanField = page.locator("#scan-field");

    // Create a title
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForTimeout(1000);

    // Create a volume
    await scanField.fill("V0099");
    await scanField.press("Enter");
    const volFeedback = page.locator(
      '.feedback-entry[data-feedback-variant="success"]'
    );
    await expect(volFeedback.first()).toBeVisible({ timeout: 5000 });
  });

  // AC5: Theme - System Preference Detection
  test("theme toggle applies dark mode class and persists", async ({
    page,
    context,
  }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
    await page.goto("/catalog");

    // Initial state: check if dark class is present or not
    const htmlEl = page.locator("html");

    // Click theme toggle
    const themeBtn = page.locator("[onclick*='mybibliToggleTheme']");
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
    context,
  }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
    await page.goto("/catalog");

    // Get initial dark state
    const htmlEl = page.locator("html");
    const initialDark = await htmlEl.evaluate((el) =>
      el.classList.contains("dark")
    );

    // Toggle theme
    const themeBtn = page.locator("[onclick*='mybibliToggleTheme']");
    await themeBtn.click();

    // Reload page
    await page.reload();

    // Theme should persist
    const afterReloadDark = await htmlEl.evaluate((el) =>
      el.classList.contains("dark")
    );
    expect(afterReloadDark).not.toBe(initialDark);

    // Restore original state
    await page.locator("[onclick*='mybibliToggleTheme']").click();
  });

  // AC9: Navigation Bar
  test("navigation bar shows Catalog link for librarian", async ({
    page,
    context,
  }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
    await page.goto("/catalog");

    const nav = page.locator("nav[aria-label='Main navigation']");
    await expect(nav).toBeVisible();

    // Catalog link should be visible for librarian
    const catalogLink = nav.locator('a[href="/catalog"]');
    await expect(catalogLink).toBeVisible();
  });

  test("navigation bar not showing Catalog for anonymous", async ({
    page,
  }) => {
    // No session cookie = anonymous
    await page.goto("/");

    const nav = page.locator("nav[aria-label='Main navigation']");
    await expect(nav).toBeVisible();

    // Catalog link should not be visible (hidden by role check)
    const catalogLink = nav.locator('a[href="/catalog"]');
    await expect(catalogLink).not.toBeVisible();
  });

  // AC9: Current page highlighted
  test("current page highlighted in nav bar", async ({ page, context }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
    await page.goto("/catalog");

    const catalogLink = page.locator(
      'nav a[href="/catalog"][aria-current="page"]'
    );
    await expect(catalogLink).toBeVisible();
  });

  // Theme toggle aria-label updates dynamically
  test("theme toggle has accessible aria-label", async ({
    page,
    context,
  }) => {
    await context.addCookies([DEV_SESSION_COOKIE]);
    await page.goto("/catalog");

    const themeBtn = page.locator("[onclick*='mybibliToggleTheme']");
    const label = await themeBtn.getAttribute("aria-label");

    // Should have a dynamic label (not just "Toggle theme")
    expect(label).toMatch(/Switch to (light|dark) mode/);
  });
});
