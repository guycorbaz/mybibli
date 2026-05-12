/**
 * Story 9-18 — NavBar role-based visibility polish E2E.
 *
 * Covers AC10 — 3 scenarios, each verifying BOTH the desktop nav strip
 * AND the mobile hamburger panel:
 *   1. Anonymous on /login — nav-link set per AC1
 *   2. Librarian on /loans — nav-link set per AC2
 *   3. Admin on /admin?tab=health — nav-link set per AC3
 *
 * Spec ID "NV" — no ISBNs generated.
 *
 * Mobile-panel login/logout parity (issue #150 — shipped): the panel
 * now carries the same role-gated login link / logout POST form as the
 * desktop strip. The 3 scenarios assert this positively.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

const TABLET = { width: 600, height: 800 };

test.describe("Story 9-18 — NavBar role-based visibility", () => {
  test("anonymous user — nav link set on /login (desktop + mobile panel)", async ({
    page,
  }) => {
    await page.goto("/login");

    const desktopNav = page.locator("nav[aria-label='Main navigation']");

    // Desktop — visible
    await expect(desktopNav.locator("a[href='/']").first()).toBeVisible();
    await expect(desktopNav.locator("a[href='/catalog']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/locations']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/series']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/login']")).toBeVisible();
    await expect(desktopNav.locator("#theme-toggle")).toBeVisible();

    // Desktop — hidden for anonymous
    await expect(desktopNav.locator("a[href='/borrowers']")).toHaveCount(0);
    await expect(desktopNav.locator("a[href='/loans']")).toHaveCount(0);
    await expect(desktopNav.locator("a[href='/admin']")).toHaveCount(0);
    await expect(desktopNav.locator("form[action='/logout']")).toHaveCount(0);

    // Mobile panel — switch to tablet viewport, open the hamburger
    await page.setViewportSize(TABLET);
    await page.locator("#mobile-menu-toggle").click();
    const panel = page.locator("#mobile-nav");
    await expect(panel).toBeVisible();

    // Mobile — visible
    await expect(panel.locator("a[href='/catalog']")).toBeVisible();
    await expect(panel.locator("a[href='/locations']")).toBeVisible();
    await expect(panel.locator("a[href='/series']")).toBeVisible();

    // Mobile — hidden
    await expect(panel.locator("a[href='/borrowers']")).toHaveCount(0);
    await expect(panel.locator("a[href='/loans']")).toHaveCount(0);
    await expect(panel.locator("a[href='/admin']")).toHaveCount(0);
    await expect(panel.locator("form[action='/logout']")).toHaveCount(0);

    // Mobile — visible (issue #150 — Sign in now in panel for parity)
    await expect(panel.locator("a[href='/login']")).toBeVisible();
  });

  test("librarian — nav link set on /loans (desktop + mobile panel)", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/loans");

    const desktopNav = page.locator("nav[aria-label='Main navigation']");

    // Desktop — visible
    await expect(desktopNav.locator("a[href='/']").first()).toBeVisible();
    await expect(desktopNav.locator("a[href='/catalog']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/locations']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/series']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/borrowers']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/loans']")).toBeVisible();
    await expect(desktopNav.locator("form[action='/logout']")).toBeVisible();

    // Desktop — hidden
    await expect(desktopNav.locator("a[href='/admin']")).toHaveCount(0);
    await expect(desktopNav.locator("a[href='/login']")).toHaveCount(0);

    // Mobile panel
    await page.setViewportSize(TABLET);
    await page.locator("#mobile-menu-toggle").click();
    const panel = page.locator("#mobile-nav");
    await expect(panel).toBeVisible();

    // Mobile — visible (issue #150 — logout in panel for parity)
    await expect(panel.locator("a[href='/catalog']")).toBeVisible();
    await expect(panel.locator("a[href='/locations']")).toBeVisible();
    await expect(panel.locator("a[href='/series']")).toBeVisible();
    await expect(panel.locator("a[href='/borrowers']")).toBeVisible();
    await expect(panel.locator("a[href='/loans']")).toBeVisible();
    await expect(panel.locator("form[action='/logout']")).toBeVisible();

    // Mobile — hidden
    await expect(panel.locator("a[href='/admin']")).toHaveCount(0);
    await expect(panel.locator("a[href='/login']")).toHaveCount(0);
  });

  test("admin — nav link set on /admin?tab=health (desktop + mobile panel)", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    // Use canonical URL — /admin server-side redirects to /admin?tab=health (story 8-1)
    await page.goto("/admin?tab=health");

    const desktopNav = page.locator("nav[aria-label='Main navigation']");

    // Desktop — visible (full set)
    await expect(desktopNav.locator("a[href='/']").first()).toBeVisible();
    await expect(desktopNav.locator("a[href='/catalog']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/locations']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/series']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/borrowers']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/loans']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/admin']")).toBeVisible();
    await expect(desktopNav.locator("form[action='/logout']")).toBeVisible();

    // Desktop — hidden
    await expect(desktopNav.locator("a[href='/login']")).toHaveCount(0);

    // Mobile panel
    await page.setViewportSize(TABLET);
    await page.locator("#mobile-menu-toggle").click();
    const panel = page.locator("#mobile-nav");
    await expect(panel).toBeVisible();

    // Mobile — visible (admin set + logout per issue #150)
    await expect(panel.locator("a[href='/catalog']")).toBeVisible();
    await expect(panel.locator("a[href='/locations']")).toBeVisible();
    await expect(panel.locator("a[href='/series']")).toBeVisible();
    await expect(panel.locator("a[href='/borrowers']")).toBeVisible();
    await expect(panel.locator("a[href='/loans']")).toBeVisible();
    await expect(panel.locator("a[href='/admin']")).toBeVisible();
    await expect(panel.locator("form[action='/logout']")).toBeVisible();

    // Mobile — hidden
    await expect(panel.locator("a[href='/login']")).toHaveCount(0);
  });
});
