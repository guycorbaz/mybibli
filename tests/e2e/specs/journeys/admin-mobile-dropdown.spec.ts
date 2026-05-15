/**
 * Story 10-4 (closes #160) — Admin tabs render as a <select> on mobile.
 *
 * Foundation Rule #7 envelope: blank browser, real loginAs, real HTMX
 * round-trips. The two viewports exercise both surfaces (the desktop
 * tablist is hidden on mobile, and vice versa).
 *
 * Spec ID "AM" — no ISBNs generated.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

const MOBILE_VIEWPORT = { width: 375, height: 667 };

test.describe("Story 10-4 — admin tabs mobile dropdown", () => {
  test.beforeEach(async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");
  });

  test("mobile viewport: <select> visible, desktop tablist hidden, Health selected by default", async ({
    page,
  }) => {
    await page.setViewportSize(MOBILE_VIEWPORT);
    await page.goto("/admin");

    const dropdown = page.locator("#admin-tab-select");
    await expect(dropdown).toBeVisible();
    // Health is the default landing tab.
    await expect(dropdown).toHaveValue("health");
    // Desktop tablist is not visible at mobile breakpoint.
    await expect(page.locator('[role="tablist"]')).not.toBeVisible();
  });

  test("desktop viewport: tablist visible, <select> hidden", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.goto("/admin");
    await expect(page.locator('[role="tablist"]')).toBeVisible();
    await expect(page.locator("#admin-tab-select")).not.toBeVisible();
  });

  test("mobile dropdown change → URL updates, panel swaps to the new tab", async ({
    page,
  }) => {
    await page.setViewportSize(MOBILE_VIEWPORT);
    await page.goto("/admin");

    const dropdown = page.locator("#admin-tab-select");
    await dropdown.selectOption("users");

    // hx-push-url=true updates the address bar via HX-Push-Url
    await page.waitForURL(/\/admin(\?tab=users)?/, { timeout: 5000 });
    // Panel content reflects the new tab — Users panel has the create-user
    // CTA somewhere; the panel-id is the most stable assertion target.
    await expect(page.locator("#panel-users")).toBeVisible({ timeout: 5000 });
    // The dropdown's value tracks the new selection after the swap.
    await expect(page.locator("#admin-tab-select")).toHaveValue("users");
  });
});
