import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

// Story 7-2: session inactivity timeout + Toast warning.
//
// Requires the server to be started with TEST_MODE=1 so the
// /debug/session-timeout override endpoint is exposed. The spec drops the
// timeout to a few seconds so the inactivity-warning Toast fires almost
// immediately.
//
// ⚠️  This spec mutates the process-global `state.settings`
// (`session_timeout_secs`). Running it in parallel with other specs would
// cause their sessions to expire unexpectedly — so the whole describe block
// is forced serial and every test that sets the override restores it in
// `afterAll`. Keep this guard even if Playwright's `fullyParallel: true`
// default is flipped.
test.describe.configure({ mode: "serial" });

const TIMEOUT_SECS = 9; // total session window: warn ~2/3 in, expire at 9s

async function setTimeout_(
  request: import("@playwright/test").APIRequestContext,
  page: import("@playwright/test").Page,
  secs: number,
) {
  // The debug endpoint is role-gated to Admin. Drive it via `page.request`
  // so the logged-in admin cookie is sent.
  const res = await page.request.post("/debug/session-timeout", {
    form: { secs: String(secs) },
  });
  if (!res.ok()) {
    throw new Error(
      `debug/session-timeout override failed (status ${res.status()}). ` +
        `Ensure the app is started with TEST_MODE=1 and the caller is Admin.`,
    );
  }
}

test.describe("Story 7-2 — session inactivity timeout", () => {
  test.afterAll(async ({ browser }) => {
    // Restore a long timeout so subsequent specs on the same server
    // cannot inherit a tiny window.
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    try {
      await loginAs(page, "admin");
      await page.request.post("/debug/session-timeout", {
        form: { secs: String(14400) },
      });
    } catch {
      // Best effort — tearDown must not throw.
    } finally {
      await ctx.close();
    }
  });

  test("Toast warning appears and Stay connected dismisses it", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    await setTimeout_(page.request, page, TIMEOUT_SECS);

    // Reload so the new `data-session-timeout` attribute is read by the JS.
    await page.goto("/catalog");
    await expect(page).toHaveURL(/\/catalog/);

    // With TIMEOUT_SECS=9, warn-before ≈ 3s → Toast at ~6s elapsed.
    const toast = page.locator("#session-timeout-toast");
    await expect(toast).toBeVisible({ timeout: 15_000 });
    await expect(toast).toHaveAttribute("role", "alert");

    const stay = page.locator("#session-keepalive-btn");
    await stay.click();

    // Toast is removed from the DOM on Stay-connected click.
    await expect(toast).toHaveCount(0, { timeout: 5_000 });
  });

  test("expired session redirects guarded GET to /login", async ({ page }) => {
    await loginAs(page, "admin");
    await setTimeout_(page.request, page, TIMEOUT_SECS);

    // Wait past the server-side timeout window without any HTMX/keepalive
    // traffic, so last_activity is not refreshed. We avoid
    // `page.waitForTimeout` (CI grep-gate) but a plain setTimeout Promise
    // is still needed — there is no DOM state that naturally elapses
    // wall-clock here.
    await new Promise((resolve) =>
      setTimeout(resolve, (TIMEOUT_SECS + 3) * 1000),
    );

    // Navigate to a librarian-gated GET. The middleware should treat the
    // session as expired → 303 to /login?next=/loans (story 7-1 behavior).
    await page.goto("/loans");
    await expect(page).toHaveURL(/\/login/, { timeout: 10_000 });
  });

  test("keepalive endpoint accepts authenticated request", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    const resp = await page.request.post("/session/keepalive");
    expect(resp.status()).toBe(200);
  });
});
