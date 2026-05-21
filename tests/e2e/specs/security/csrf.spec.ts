/**
 * Story 8-2 smoke — CSRF synchronizer-token middleware.
 *
 * Foundation Rule #7: starts from a blank browser context, uses `loginAs`
 * (no DEV_SESSION_COOKIE), and exercises the real end-to-end user journey.
 *
 * Covers:
 *   - AC 5 / AC 6: valid-token → accept, missing-token → 403 with
 *     server-rendered FeedbackEntry (HX-Trigger: csrf-rejected swap).
 *   - AC 11 / AC 12: GET /logout returns 405; logout now flows via a
 *     POST form with the hidden `_csrf_token` input.
 *   - AC 15: FR locale serves the localized "Session expirée" message.
 *   - AC 3: anonymous first-hit gets a `<meta name=csrf-token>` so the
 *     language toggle works even before login.
 *
 * Spec ID "CS" — this spec does NOT scan any ISBN, but the convention
 * stands for any future addition.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

test.describe("Story 8-2 smoke — CSRF", () => {
  test("logged-in language toggle with a valid token succeeds", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    await page.goto("/catalog");
    const token = await page
      .locator('meta[name="csrf-token"]')
      .getAttribute("content");
    expect(token, "base.html must emit a non-empty csrf-token meta tag").toBeTruthy();
    expect(token!.length).toBeGreaterThanOrEqual(20);

    // v1.7.0: the nav-bar language toggle is now a `<select>` dropdown
    // (id `lang-select`). `selectOption` fires `change` → form submit via
    // `mybibli.js::initAutoSubmitSelects`. The hidden `_csrf_token` input
    // is carried unchanged. Picking the OTHER locale forces a real submit
    // (selecting the already-selected option is a no-op in some browsers).
    await page.locator("#lang-select").selectOption("en");
    await expect(page).toHaveURL(/\/catalog/);
  });

  test("tampered token on HTMX mutation produces a server-rendered 403 FeedbackEntry", async ({
    page,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");
    await page.goto("/catalog");

    // Mutate the in-page meta tag so csrf.js's htmx:configRequest
    // listener injects a bogus token. Do NOT change the DB — this is the
    // hostile-page mimicry scenario.
    await page.evaluate(() => {
      const meta = document.querySelector('meta[name="csrf-token"]') as HTMLMetaElement | null;
      if (meta) meta.content = "tampered-token-value";
    });

    // Fire a 403 via an HTMX-driven keepalive ping. We do it via
    // fetch + the tampered token so the response flows through the
    // middleware just like any HTMX POST would.
    const response = await page.evaluate(async () => {
      const r = await fetch("/session/keepalive", {
        method: "POST",
        headers: { "X-CSRF-Token": "tampered-token-value" },
      });
      return { status: r.status, trigger: r.headers.get("HX-Trigger") };
    });
    expect(response.status).toBe(403);
    expect(response.trigger).toBe("csrf-rejected");
  });

  test("GET /logout returns 405 (POST-only)", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    const res = await page.request.get("/logout");
    expect(res.status()).toBe(405);
  });

  test("nav-bar logout submits a POST form and redirects home", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");
    await page.goto("/catalog");

    // The logout UI is now a button inside a POST form (i18n-aware).
    const logoutButton = page.getByRole("button", { name: /Log out|Déconnexion/i });
    await expect(logoutButton).toBeVisible();

    const [response] = await Promise.all([
      page.waitForResponse((r) => r.url().endsWith("/logout") && r.request().method() === "POST"),
      logoutButton.click(),
    ]);
    expect([200, 303]).toContain(response.status());
    await expect(page).toHaveURL(/\/$|\/login$/);
  });

  test("FR locale renders the localized CSRF rejection message", async ({ page, context }) => {
    await page.context().clearCookies();
    await context.addCookies([
      { name: "lang", value: "fr", url: "http://localhost:8080" },
    ]);
    await loginAs(page, "admin");
    await page.goto("/catalog");

    const tamperedResponse = await page.evaluate(async () => {
      const r = await fetch("/session/keepalive", {
        method: "POST",
        headers: { "X-CSRF-Token": "tampered" },
      });
      return { status: r.status, body: await r.text() };
    });
    expect(tamperedResponse.status).toBe(403);
    // Strict FR assertion — passing on the EN key would mask a locale
    // resolution regression (the point of AC 15 is to verify that the
    // `lang=fr` cookie actually routes through `rust_i18n::t!`).
    expect(tamperedResponse.body).toMatch(/Session expirée/);
    expect(tamperedResponse.body).not.toMatch(/Session expired/);
  });

  test("anonymous visitor receives a CSRF token on first hit", async ({ page }) => {
    await page.context().clearCookies();
    await page.goto("/");
    const token = await page
      .locator('meta[name="csrf-token"]')
      .getAttribute("content");
    expect(token, "anonymous first-hit must still carry a CSRF meta tag").toBeTruthy();
    expect(token!.length).toBeGreaterThanOrEqual(20);

    // And the language toggle (anonymous-allowed mutation) accepts the token.
    // v1.7.0: nav-bar lang toggle is now a <select>; selecting EN (we
    // landed in FR via Accept-Language) fires the auto-submit handler.
    await page.locator("#lang-select").selectOption("en");
    await expect(page).toHaveURL(/\/$/);
  });

  /**
   * Story 10-2 (issue #45 H4): an authenticated user whose plain-form POST
   * drifts on the CSRF token sees a full-chrome 403 page with a Reload CTA,
   * NOT a silent 303 → /login → / redirect that eats the action.
   */
  test("authenticated plain-form CSRF drift renders 403 full-chrome page", async ({
    page,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");
    await page.goto("/catalog");

    // Plain form POST with a wrong `_csrf_token` body, no `hx-request` header.
    // Routes the middleware down the authenticated-form-drift branch.
    const response = await page.request.post("/language", {
      form: { _csrf_token: "tampered-by-test", next: "/catalog", lang: "fr" },
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      maxRedirects: 0,
    });
    expect(response.status()).toBe(403);
    // The response is a full HTML page, not a 303 redirect — body must
    // contain a doctype and the EN/FR reload-CTA i18n strings.
    const body = await response.text();
    expect(body).toContain("<!DOCTYPE html>");
    expect(body).toMatch(/Reload page|Recharger la page/);
    // Cache-Control: no-store prevents the browser back-cache from serving
    // a stale rejection after the user reloads and re-establishes a session.
    expect(response.headers()["cache-control"]).toBe("no-store");
  });
});
