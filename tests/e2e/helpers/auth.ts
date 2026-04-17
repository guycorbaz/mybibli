import { Page } from "@playwright/test";

export type Role = "admin" | "librarian";

/**
 * Perform a real browser login as one of the seeded users.
 *
 * Uses stable id selectors `#username` and `#password` from templates/pages/login.html.
 * - `"admin"` (default) — seeded by migrations/20260331000004_fix_dev_user_hash.sql.
 *   Override password via `TEST_ADMIN_PASSWORD` env var.
 * - `"librarian"` — seeded by migrations/20260414000001_seed_librarian_user.sql.
 *   Override password via `TEST_LIBRARIAN_PASSWORD` env var.
 *
 * Smoke tests (one per epic, per CLAUDE.md Foundation Rule #7) MUST use this helper
 * instead of injecting DEV_SESSION_COOKIE.
 */
export async function loginAs(page: Page, role: Role = "admin"): Promise<void> {
  const username = role;
  // Guard empty-string env overrides (common CI misconfig when a secret is unset):
  // `??` only guards null/undefined, so `TEST_*_PASSWORD=""` would leak through.
  const envPassword =
    role === "admin" ? process.env.TEST_ADMIN_PASSWORD : process.env.TEST_LIBRARIAN_PASSWORD;
  const password = envPassword && envPassword.length > 0 ? envPassword : role;
  await page.goto("/login");
  await page.fill("#username", username);
  await page.fill("#password", password);
  // Story 7-3 added a language-toggle form to every page (including /login),
  // so `button[type="submit"]` is no longer unique. `#login-submit` points
  // specifically at the login form's submit button.
  await page.click("#login-submit");
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
