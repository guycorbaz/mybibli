/**
 * Epic 7 smoke test — anonymous browsing + role gating (Story 7-1).
 *
 * Foundation Rule #7: MUST start from a blank browser context, use the real
 * `loginAs` helper (no DEV_SESSION_COOKIE), and exercise the epic's core
 * user journey end-to-end.
 *
 * Covers AC #1 (anonymous read-only), AC #2 (redirect with ?next=),
 * AC #4 (librarian on admin route gets 403, not redirect), AC #6 (nav items),
 * AC #9 (epic smoke target: DELETE /borrower/{id} as librarian → 403).
 */
import { test, expect } from "@playwright/test";
import { loginAs, logout } from "../../helpers/auth";

test.describe("Epic 7 smoke — anonymous browsing + role gating", () => {
  test("anonymous can browse catalog read-only; librarian gets 403 on admin route", async ({
    page,
  }) => {
    // ── Anonymous phase ────────────────────────────────────────────
    // Start from a truly blank context: no cookies, no session.
    await page.context().clearCookies();

    // AC #1 + AC #6: /catalog is anonymous-readable, no scan-field, no new-title button.
    await page.goto("/catalog");
    await expect(page).toHaveURL(/\/catalog$/);
    await expect(page.locator("#scan-field")).toHaveCount(0);
    await expect(page.locator("#new-title-btn")).toHaveCount(0);

    // AC #6: nav bar exposes Login link and hides admin/loans/borrowers items.
    await expect(page.getByRole("link", { name: /login|connexion/i })).toBeVisible();
    await expect(page.getByRole("link", { name: /^logout$|^déconnexion$/i })).toHaveCount(0);
    // /admin dead link must be gone (Epic 5/6 carry-over).
    await expect(page.locator('a[href="/admin"]')).toHaveCount(0);

    // AC #1: /series is also anonymous-readable.
    await page.goto("/series");
    await expect(page).toHaveURL(/\/series$/);

    // AC #1: /locations tree browser accessible, but no "add root" form.
    await page.goto("/locations");
    await expect(page).toHaveURL(/\/locations$/);
    // "Add root location" <details> section is wrapped in role gate.
    await expect(page.locator('form[action="/locations"]')).toHaveCount(0);

    // AC #2: protected routes redirect to /login?next=<encoded>.
    await page.goto("/loans");
    await expect(page).toHaveURL(/\/login\?next=%2Floans$/);
    // The login form carries the next value as a hidden field.
    await expect(page.locator('input[name="next"][value="/loans"]')).toHaveCount(1);

    await page.goto("/borrowers");
    await expect(page).toHaveURL(/\/login\?next=%2Fborrowers$/);

    // ── Librarian phase ───────────────────────────────────────────
    await loginAs(page, "librarian");

    // After login, catalog shows edit affordances (scan field + new title).
    await page.goto("/catalog");
    await expect(page.locator("#scan-field")).toBeVisible();
    await expect(page.locator("#new-title-btn")).toBeVisible();

    // Loans page accessible to librarian.
    await page.goto("/loans");
    await expect(page).toHaveURL(/\/loans$/);

    // AC #9 403 target: DELETE /borrower/{id} stays Admin (matrix decision 2a).
    // Create a borrower so there's something to target, then attempt DELETE as librarian.
    await page.goto("/borrowers");
    const borrowerName = `RG-ForbiddenTarget-${Date.now()}`;
    await page.fill('input[name="name"]', borrowerName);
    await page.click('button[type="submit"]');
    // Reload the list and read the new borrower's detail URL from the anchor.
    await page.goto("/borrowers");
    const borrowerLink = page.getByRole("link", { name: borrowerName }).first();
    await expect(borrowerLink).toBeVisible({ timeout: 5000 });
    const href = await borrowerLink.getAttribute("href");
    const borrowerId = href?.match(/\/borrower\/(\d+)/)?.[1];
    expect(borrowerId).toBeTruthy();

    // AC #4: librarian hits admin-only DELETE → 403, NOT a redirect, with feedback body.
    const deleteResp = await page.request.delete(`/borrower/${borrowerId}`);
    expect(deleteResp.status(), "AC #4: librarian on admin route → 403 Forbidden").toBe(
      403,
    );
    const body = await deleteResp.text();
    expect(body, "AC #4: feedback entry body (not a redirect)").toMatch(
      /access denied|accès refusé/i,
    );

    // The borrower is still alive (zero state change).
    await page.goto(`/borrower/${borrowerId}`);
    await expect(page.locator("body")).toContainText(borrowerName);

    // ── Admin phase (cleanup + AC #5 happy path) ──────────────────
    await logout(page);
    await loginAs(page, "admin");
    const adminDelete = await page.request.delete(`/borrower/${borrowerId}`);
    expect(
      [200, 204, 303].includes(adminDelete.status()),
      `AC #5: admin DELETE must succeed (got ${adminDelete.status()})`,
    ).toBeTruthy();
  });
});
