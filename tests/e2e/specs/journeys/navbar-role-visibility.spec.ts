/**
 * Story 9-18 — NavBar role-based visibility polish E2E.
 *
 * Covers AC10 — 3 scenarios, each verifying BOTH the sidebar (≥ lg) AND
 * the small-screen drawer (story 14-2: the same `<nav>`, opened by the
 * menu button below lg):
 *   1. Anonymous on /login — nav-link set per AC1
 *   2. Librarian on /loans — nav-link set per AC2
 *   3. Admin on /admin?tab=health — nav-link set per AC3
 *
 * Spec ID "NV" — no ISBNs generated.
 *
 * Login/logout on small screens (issue #150): the drawer's foot carries
 * the role-gated login link / logout POST form. The 3 scenarios assert
 * this positively.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { openDrawer } from "../../helpers/drawer";

const TABLET = { width: 600, height: 800 };

test.describe("Story 9-18 — NavBar role-based visibility", () => {
  test("anonymous user — nav link set on /login (sidebar + drawer)", async ({
    page,
  }) => {
    await page.goto("/login");

    const desktopNav = page.getByRole("navigation", { name: "Main navigation" });

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
    // Story 11-21 — no Proposals entry for anonymous visitors.
    await expect(page.locator("a[href='/proposals']")).toHaveCount(0);

    // Drawer — switch to a small viewport, open it from the menu button
    await page.setViewportSize(TABLET);
    const panel = await openDrawer(page);

    // Mobile — visible
    await expect(panel.locator("a[href='/catalog']")).toBeVisible();
    await expect(panel.locator("a[href='/locations']")).toBeVisible();
    await expect(panel.locator("a[href='/series']")).toBeVisible();

    // Mobile — hidden
    await expect(panel.locator("a[href='/borrowers']")).toHaveCount(0);
    await expect(panel.locator("a[href='/loans']")).toHaveCount(0);
    await expect(panel.locator("a[href='/admin']")).toHaveCount(0);
    await expect(panel.locator("form[action='/logout']")).toHaveCount(0);

    // Mobile — visible (issue #150 — Sign in in the drawer's foot); story
    // 14-2 — the slim bar has no Sign-in link of its own.
    await expect(panel.locator("a[href='/login']")).toBeVisible();
    await expect(page.locator("header a[href='/login']")).toHaveCount(0);
  });

  test("librarian — nav link set on /loans (sidebar + drawer)", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/loans");

    const desktopNav = page.getByRole("navigation", { name: "Main navigation" });

    // Desktop — visible
    await expect(desktopNav.locator("a[href='/']").first()).toBeVisible();
    await expect(desktopNav.locator("a[href='/catalog']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/locations']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/series']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/borrowers']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/loans']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/proposals']")).toBeVisible();
    await expect(desktopNav.locator("form[action='/logout']")).toBeVisible();

    // Desktop — hidden
    await expect(desktopNav.locator("a[href='/admin']")).toHaveCount(0);
    await expect(desktopNav.locator("a[href='/login']")).toHaveCount(0);

    // Drawer — switch to a small viewport, open it from the menu button
    await page.setViewportSize(TABLET);
    const panel = await openDrawer(page);

    // Mobile — visible (issue #150 — logout in panel for parity)
    await expect(panel.locator("a[href='/catalog']")).toBeVisible();
    await expect(panel.locator("a[href='/locations']")).toBeVisible();
    await expect(panel.locator("a[href='/series']")).toBeVisible();
    await expect(panel.locator("a[href='/borrowers']")).toBeVisible();
    await expect(panel.locator("a[href='/loans']")).toBeVisible();
    await expect(panel.locator("a[href='/proposals']")).toBeVisible();
    await expect(panel.locator("form[action='/logout']")).toBeVisible();
    // Story 11-21 (#545) / 14-2 — the slim bar has no logout of its own;
    // the drawer's (above) is the way out.
    await expect(page.locator("header form[action='/logout']")).toHaveCount(0);

    // Mobile — hidden
    await expect(panel.locator("a[href='/admin']")).toHaveCount(0);
    await expect(panel.locator("a[href='/login']")).toHaveCount(0);
  });

  test("admin — nav link set on /admin?tab=health (sidebar + drawer)", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    // Use canonical URL — /admin server-side redirects to /admin?tab=health (story 8-1)
    await page.goto("/admin?tab=health");

    const desktopNav = page.getByRole("navigation", { name: "Main navigation" });

    // Desktop — visible (full set)
    await expect(desktopNav.locator("a[href='/']").first()).toBeVisible();
    await expect(desktopNav.locator("a[href='/catalog']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/locations']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/series']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/borrowers']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/loans']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/admin']")).toBeVisible();
    await expect(desktopNav.locator("a[href='/proposals']")).toBeVisible();
    await expect(desktopNav.locator("form[action='/logout']")).toBeVisible();

    // Desktop — hidden
    await expect(desktopNav.locator("a[href='/login']")).toHaveCount(0);

    // Drawer — switch to a small viewport, open it from the menu button
    await page.setViewportSize(TABLET);
    const panel = await openDrawer(page);

    // Mobile — visible (admin set + logout per issue #150)
    await expect(panel.locator("a[href='/catalog']")).toBeVisible();
    await expect(panel.locator("a[href='/locations']")).toBeVisible();
    await expect(panel.locator("a[href='/series']")).toBeVisible();
    await expect(panel.locator("a[href='/borrowers']")).toBeVisible();
    await expect(panel.locator("a[href='/loans']")).toBeVisible();
    await expect(panel.locator("a[href='/admin']")).toBeVisible();
    await expect(panel.locator("a[href='/proposals']")).toBeVisible();
    await expect(panel.locator("form[action='/logout']")).toBeVisible();
    await expect(page.locator("header form[action='/logout']")).toHaveCount(0);

    // Mobile — hidden
    await expect(panel.locator("a[href='/login']")).toHaveCount(0);
  });
});
