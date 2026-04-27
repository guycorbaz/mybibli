/**
 * Epic 8 smoke test — Admin page shell + Health tab (Story 8-1).
 *
 * Foundation Rule #7: MUST start from a blank browser context, use the real
 * `loginAs` helper (no DEV_SESSION_COOKIE), and exercise the epic's core
 * user journey end-to-end.
 *
 * Covers AC #1 (admin-only), AC #3 (Health content), AC #4 (tab accessibility),
 * AC #5 (HTMX tab switching + hx-push-url), AC #6 (librarian 403,
 * anonymous 303 → /login?next=%2Fadmin).
 *
 * Spec ID "AD" — no ISBNs generated (this spec creates no catalog rows).
 */
import { test, expect } from "@playwright/test";
import { loginAs, logout } from "../../helpers/auth";

test.describe("Epic 8 smoke — admin page shell + Health tab", () => {
  test("admin sees all 5 tabs, Health is default, HTMX swaps update URL + panel", async ({
    page,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    // AC #1: direct nav to /admin renders the full page with the Health tab selected.
    const initial = await page.goto("/admin");
    expect(initial?.status()).toBe(200);
    await expect(page).toHaveURL(/\/admin$/);

    // AC #4: five tabs, i18n-aware matchers (EN + FR).
    const healthTab = page.getByRole("tab", { name: /Health|Santé/i });
    const usersTab = page.getByRole("tab", { name: /Users|Utilisateurs/i });
    const refTab = page.getByRole("tab", { name: /Reference data|Données de référence/i });
    const trashTab = page.getByRole("tab", { name: /Trash|Corbeille/i });
    const systemTab = page.getByRole("tab", { name: /System|Système/i });

    await expect(healthTab).toBeVisible();
    await expect(usersTab).toBeVisible();
    await expect(refTab).toBeVisible();
    await expect(trashTab).toBeVisible();
    await expect(systemTab).toBeVisible();

    // AC #4: Health is selected by default, others are not.
    await expect(healthTab).toHaveAttribute("aria-selected", "true");
    await expect(usersTab).toHaveAttribute("aria-selected", "false");
    await expect(trashTab).toHaveAttribute("aria-selected", "false");

    // AC #3: Health content — app version matches semver shape; DB version is
    // a MariaDB-style string; at least one entity count row + one provider row.
    await expect(page.locator("body")).toContainText(/\d+\.\d+\.\d+/);
    await expect(page.locator("body")).toContainText(/\d+\.\d+/);
    await expect(
      page.getByText(/Titles|Titres|Volumes|Contributors|Contributeurs/i).first(),
    ).toBeVisible();
    // Provider health table — at least one registered provider row. The
    // background ping task starts 10 s after boot, so Unknown is a passing
    // state (pre-ping default). Don't assert Reachable.
    await expect(
      page.getByText(/Reachable|Unreachable|Unknown|Inconnu|Accessible|Inaccessible|n\/a/i).first(),
    ).toBeVisible();

    // AC #5: click Users tab → HTMX swap updates URL + panel.
    await usersTab.click();
    await expect(page).toHaveURL(/\/admin\?tab=users$/);
    await expect(usersTab).toHaveAttribute("aria-selected", "true");
    await expect(healthTab).toHaveAttribute("aria-selected", "false");
    await expect(page.locator("#panel-users")).toContainText("8-2");

    // Reference data — story 8-4 ships the four sub-sections.
    await refTab.click();
    await expect(page).toHaveURL(/\/admin\?tab=reference_data$/);
    await expect(refTab).toHaveAttribute("aria-selected", "true");
    await expect(
      page.getByRole("button", { name: /Add genre|Ajouter un genre/i }),
    ).toBeVisible();

    // Trash — story 8-7 ships the trash list.
    await trashTab.click();
    await expect(page).toHaveURL(/\/admin\?tab=trash$/);
    await expect(trashTab).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#panel-trash")).toBeVisible();

    // System — stub points to a future story.
    await systemTab.click();
    await expect(page).toHaveURL(/\/admin\?tab=system$/);
    await expect(systemTab).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#panel-system")).toBeVisible();

    // Browser Back → previous tab (Trash). The popstate re-fetches /admin?tab=trash
    // as a full page render, so the Trash panel is selected again.
    await page.goBack();
    await expect(page).toHaveURL(/\/admin\?tab=trash$/);
    await expect(page.getByRole("tab", { name: /Trash|Corbeille/i })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  test("librarian is forbidden from /admin (403, not redirect)", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");

    const resp = await page.goto("/admin");
    // AC #6 role split: authenticated-but-insufficient → 403, not 303.
    expect(resp?.status(), "librarian on /admin → 403 Forbidden").toBe(403);
    const body = await resp!.text();
    expect(body, "FeedbackEntry body renders i18n 403 message").toMatch(
      /access denied|accès refusé/i,
    );
  });

  test("anonymous user is redirected to /login?next=%2Fadmin", async ({ page }) => {
    await page.context().clearCookies();

    await page.goto("/admin");
    // AC #6 anonymous path: 303 → /login?next=%2Fadmin.
    await expect(page).toHaveURL(/\/login\?next=%2Fadmin$/);
    // The login form carries `next` as a hidden input so POST /login can
    // bounce back to /admin after credentials are accepted.
    await expect(page.locator('input[name="next"][value="/admin"]')).toHaveCount(1);
  });

  test("nav bar exposes /admin link only for admin role", async ({ page }) => {
    // Anonymous: no /admin link.
    await page.context().clearCookies();
    await page.goto("/catalog");
    await expect(page.locator('nav a[href="/admin"]')).toHaveCount(0);

    // Librarian: still no /admin link.
    await loginAs(page, "librarian");
    await page.goto("/catalog");
    await expect(page.locator('nav a[href="/admin"]')).toHaveCount(0);

    // Admin: the nav link appears in the desktop nav.
    await logout(page);
    await loginAs(page, "admin");
    await page.goto("/catalog");
    await expect(page.locator('nav a[href="/admin"]').first()).toBeVisible();
  });
});
