import { Page } from "@playwright/test";

/**
 * Perform a real browser login as the seeded admin user.
 *
 * Uses stable id selectors `#username` and `#password` from templates/pages/login.html.
 * Credentials default to the seed values in migrations/20260331000004_fix_dev_user_hash.sql
 * (admin/admin). Override via TEST_ADMIN_PASSWORD env var if the seed changes.
 *
 * Smoke tests (one per epic, per CLAUDE.md Foundation Rule #7) MUST use this helper
 * instead of injecting DEV_SESSION_COOKIE. Non-smoke tests MAY continue to use cookie
 * injection as a speed optimization for auth-independent flows.
 */
export async function loginAs(page: Page): Promise<void> {
  const password = process.env.TEST_ADMIN_PASSWORD || "admin";
  await page.goto("/login");
  await page.fill("#username", "admin");
  await page.fill("#password", password);
  await page.click('button[type="submit"]');
  // Login currently redirects to /catalog; accept any URL that is not /login.
  await page.waitForURL(/^(?!.*\/login).*$/, { timeout: 5000 });
}

/**
 * Clear session cookies and navigate to login. Mirrors manual logout via clearing
 * the session cookie; no dedicated /logout endpoint is required for this stub.
 */
export async function logout(page: Page): Promise<void> {
  await page.context().clearCookies();
  await page.goto("/login");
}
