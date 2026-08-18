/**
 * Story 7-3 — FR/EN language toggle E2E.
 *
 * Three journeys:
 *  1. Anonymous visitor — nav toggle, cookie persistence across navigation.
 *  2. Authenticated persistence — stored `users.preferred_language` survives
 *     logout + cookie clear + fresh login.
 *  3. Return-URL integrity — toggle preserves the current path + query string.
 *
 * Selector policy (per CLAUDE.md): prefer role/text selectors. The toggle
 * emits a `<select id="lang-select">` dropdown (v1.7.0 — was originally a
 * pair of `<button name="lang">` elements). Tests use `selectOption("en")`
 * which fires the `change` event picked up by
 * `mybibli.js::initAutoSubmitSelects`.
 */
import { test, expect, Page } from "@playwright/test";
import { loginAs, logout } from "../../helpers/auth";

// Pin the browser `Accept-Language` to `fr` for the whole describe block so
// the story-3 default (FR when no cookie is present) is exercised
// deterministically. Playwright otherwise defaults to the runner's OS locale,
// which can flip between `en-US`, `fr-FR`, etc. across CI machines.
test.use({ locale: "fr-FR", extraHTTPHeaders: { "Accept-Language": "fr" } });

/**
 * Reset the seeded `librarian` user's stored `preferred_language` back to FR
 * so this spec does not leak state into other specs (which share the live
 * E2E DB). Requires an authenticated librarian session on the page.
 */
async function resetLibrarianPreferenceToFr(page: Page): Promise<void> {
  // Story 8-2: POST /language requires a CSRF token. Read it from the
  // meta tag emitted on whatever page the caller was on.
  const csrfToken = await page
    .locator('meta[name="csrf-token"]')
    .getAttribute("content");
  await page.request.post("/language", {
    form: { _csrf_token: csrfToken ?? "", lang: "fr", next: "/catalog" },
    maxRedirects: 0,
  });
}

test.describe("Story 7-3 — language toggle FR/EN", () => {
  // #470 — serialised, for the same reason as the admin-system-settings
  // describe: these tests mutate shared state that no per-test isolation
  // reaches. `preferred_language` is a column on `users`, not on the session,
  // so all three tests — all logging in as the same seeded `librarian` —
  // write one row.
  //
  // The `beforeEach` below is what makes parallelism actively harmful rather
  // than merely risky: it resets the stored preference to FR, so under
  // `fullyParallel` one test's reset lands between another test's "switch to
  // EN" and its assertion. The observed failure is `html lang` stable at
  // "fr" across every poll — not a timing wobble on a value that eventually
  // arrives, but the wrong value written by a sibling.
  //
  // Serialising trades a few seconds of CI time for a reset that means what
  // it says. A dedicated seeded account would also work, but it buys
  // parallelism across three short tests at the price of a migration and one
  // more account to keep alive.
  test.describe.configure({ mode: "serial" });

  // Ensure a clean `preferred_language` baseline BEFORE each authenticated
  // run — a prior aborted run may have left EN stored.
  test.beforeEach(async ({ browser }) => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    await loginAs(page, "librarian");
    await resetLibrarianPreferenceToFr(page);
    await ctx.close();
  });

  test("anonymous user can toggle to EN and persist across navigation", async ({ page }) => {
    await page.context().clearCookies();

    // Accept-Language pinned to `fr` via `test.use` above → FR default.
    await page.goto("/catalog");
    await expect(page.locator("html")).toHaveAttribute("lang", "fr");
    // i18n-aware matcher (both sides accept a substring, per CLAUDE.md).
    await expect(page.getByRole("link", { name: /Catalogue/i }).first()).toBeVisible();

    // Click the EN toggle — full page reload, not HTMX swap.
    // v1.7.0 (CR #275 / #276) — the nav toggle is now a `<select>`
    // dropdown (4 langs). `selectOption("en")` fires the `change` event,
    // which `mybibli.js::initAutoSubmitSelects` listens for and submits
    // the parent form. The desktop variant has id `lang-select`; the
    // mobile variant `lang-select-mobile`. We always hit the desktop one
    // here — the chromium project runs at desktop viewport.
    await Promise.all([
      page.waitForLoadState("load"),
      page.locator("#lang-select").selectOption("en"),
    ]);

    await expect(page.locator("html")).toHaveAttribute("lang", "en");
    await expect(page.getByRole("link", { name: /Catalog/i }).first()).toBeVisible();

    const cookies = await page.context().cookies();
    const langCookie = cookies.find((c) => c.name === "lang");
    expect(langCookie?.value).toBe("en");

    // Navigate to /series — EN persists (cookie-based).
    await page.goto("/series");
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
  });

  test("authenticated preference survives logout + cookie clear", async ({ page }) => {
    await loginAs(page, "librarian");

    // Toggle to EN while authenticated — the handler persists to
    // `users.preferred_language` AND sets the cookie.
    // v1.7.0 (CR #275 / #276) — the nav toggle is now a `<select>`
    // dropdown (4 langs). `selectOption("en")` fires the `change` event,
    // which `mybibli.js::initAutoSubmitSelects` listens for and submits
    // the parent form. The desktop variant has id `lang-select`; the
    // mobile variant `lang-select-mobile`. We always hit the desktop one
    // here — the chromium project runs at desktop viewport.
    await Promise.all([
      page.waitForLoadState("load"),
      page.locator("#lang-select").selectOption("en"),
    ]);
    await expect(page.locator("html")).toHaveAttribute("lang", "en");

    // `logout()` already clears context cookies; no need to call again.
    await logout(page);

    // Log back in from a blank context. Cookie is gone, Accept-Language is
    // FR, but the stored preference is EN — the login handler must emit a
    // `lang=en` cookie from the DB row.
    await loginAs(page, "librarian");
    await page.goto("/catalog");
    await expect(page.locator("html")).toHaveAttribute("lang", "en");

    // Cleanup: reset the stored preference so subsequent specs start clean.
    await resetLibrarianPreferenceToFr(page);
  });

  test("toggle preserves current path and query string", async ({ page }) => {
    await page.context().clearCookies();

    await page.goto("/catalog?q=tintin&sort=title");
    await expect(page.locator("html")).toHaveAttribute("lang", "fr");

    // v1.7.0 (CR #275 / #276) — the nav toggle is now a `<select>`
    // dropdown (4 langs). `selectOption("en")` fires the `change` event,
    // which `mybibli.js::initAutoSubmitSelects` listens for and submits
    // the parent form. The desktop variant has id `lang-select`; the
    // mobile variant `lang-select-mobile`. We always hit the desktop one
    // here — the chromium project runs at desktop viewport.
    await Promise.all([
      page.waitForLoadState("load"),
      page.locator("#lang-select").selectOption("en"),
    ]);

    // Parse the URL and pin the exact guarantees from AC 18: path must be
    // `/catalog`, both query params must round-trip unchanged, and crucially
    // no `?lang=` leaked into the URL (which would override the cookie on
    // every future request).
    const parsed = new URL(page.url());
    expect(parsed.pathname).toBe("/catalog");
    expect(parsed.searchParams.get("q")).toBe("tintin");
    expect(parsed.searchParams.get("sort")).toBe("title");
    expect(parsed.searchParams.has("lang")).toBe(false);
    await expect(page.locator("html")).toHaveAttribute("lang", "en");
  });
});
