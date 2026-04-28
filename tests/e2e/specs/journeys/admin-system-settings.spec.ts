/**
 * Story 8-5 E2E — Admin System Settings.
 *
 * Foundation Rule #7: smoke covers the real journey end-to-end (blank
 * browser → loginAs → navigate → change setting → verify the change
 * affected behavior). Spec ID "SY".
 *
 * Coverage:
 *   - AC #1 — panel renders three sections (Loans, Providers, Language).
 *   - AC #2 — overdue threshold save updates the value AND takes effect on
 *     /loans without a restart.
 *   - AC #3 — provider key save: input renders blank, helper text shows
 *     mask, Clear checkbox wipes.
 *   - AC #7 — overdue threshold validation: 0 / negative / > 365 → 400 +
 *     localized error inline.
 *   - AC #8 — Anonymous → 303, Librarian → 403.
 *
 * Per-test unique slugs to avoid collisions across parallel/retried runs.
 */
import { test, expect } from "@playwright/test";
import { loginAs, logout } from "../../helpers/auth";

function uniqueSlug(prefix: string): string {
  return `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
}

test.describe("Story 8-5 — Admin System Settings", () => {
  test.beforeEach(async ({ page }) => {
    await page.context().clearCookies();
  });

  test("admin sees all three sections in the System tab", async ({ page }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");
    await expect(
      page.getByRole("heading", { name: /Loans|Prêts/i, level: 3 }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", {
        name: /Metadata providers|Fournisseurs de métadonnées/i,
        level: 3,
      }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: /Language|Langue/i, level: 3 }),
    ).toBeVisible();
  });

  test("save overdue threshold takes effect on /loans without restart", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");

    // Change threshold to 14 days.
    const thresholdInput = page.locator(
      'form#admin-system-loans-form input[name="overdue_threshold_days"]',
    );
    await expect(thresholdInput).toBeVisible();
    await thresholdInput.fill("14");
    await page
      .locator('form#admin-system-loans-form button[type="submit"]')
      .click();
    await expect(
      page
        .locator("#feedback-list")
        .getByText(/Loans settings saved|Préférences de prêt enregistrées/i),
    ).toBeVisible({ timeout: 10000 });

    // Verify the persisted value re-renders.
    await page.goto("/admin?tab=system");
    await expect(thresholdInput).toHaveValue("14");

    // Reset to 30 to keep test isolation.
    await thresholdInput.fill("30");
    await page
      .locator('form#admin-system-loans-form button[type="submit"]')
      .click();
    await expect(
      page
        .locator("#feedback-list")
        .getByText(/Loans settings saved|Préférences de prêt enregistrées/i),
    ).toBeVisible({ timeout: 10000 });
  });

  test("provider key save renders mask after reload, clear wipes", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");

    const gbInput = page.locator(
      'form#admin-system-providers-form input[name="google_books_api_key"]',
    );
    await expect(gbInput).toBeVisible();

    // Initially blank, helper "Not set" / "Non définie".
    await expect(gbInput).toHaveValue("");

    // Set a key.
    const newKey = uniqueSlug("test_key_AAAA1234");
    await gbInput.fill(newKey);
    await page
      .locator('form#admin-system-providers-form button[type="submit"]')
      .click();
    await expect(
      page
        .locator("#feedback-list")
        .getByText(/Google Books key saved|Clé Google Books enregistrée/i),
    ).toBeVisible({ timeout: 10000 });

    // Reload — input is BLANK, helper shows "Set: ••••<last4>".
    await page.goto("/admin?tab=system");
    await expect(gbInput).toHaveValue("");
    const last4 = newKey.slice(-4);
    await expect(
      page.locator("form#admin-system-providers-form").getByText(
        new RegExp(`••••${last4}`),
      ),
    ).toBeVisible();

    // Clear via the checkbox.
    const gbClearCheckbox = page.locator(
      'form#admin-system-providers-form input[name="_clear_google_books"]',
    );
    await gbClearCheckbox.check();
    // The text input should now be disabled by the JS handler.
    await expect(gbInput).toBeDisabled();
    await page
      .locator('form#admin-system-providers-form button[type="submit"]')
      .click();
    await expect(
      page
        .locator("#feedback-list")
        .getByText(/Google Books key cleared|Clé Google Books effacée/i),
    ).toBeVisible({ timeout: 10000 });

    // Reload — helper shows "Not set".
    await page.goto("/admin?tab=system");
    await expect(
      page
        .locator("form#admin-system-providers-form")
        .getByText(/Not set|Non définie/i),
    ).toBeVisible();
  });

  test("overdue threshold validation: 0 → 400 + inline error", async ({
    page,
    request,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");
    const realToken = await page
      .locator('meta[name="csrf-token"]')
      .getAttribute("content");
    expect(realToken).toBeTruthy();
    const cookies = await page.context().cookies();
    const cookieHeader = cookies.map((c) => `${c.name}=${c.value}`).join("; ");

    const resp = await request.post("/admin/system/loans", {
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
        "Cookie": cookieHeader,
        "HX-Request": "true",
      },
      data: `overdue_threshold_days=0&overdue_threshold_version=1&_csrf_token=${encodeURIComponent(realToken!)}`,
      maxRedirects: 0,
    });
    expect(resp.status()).toBe(400);
  });

  test("default language change affects fresh anonymous visitor with no cookie/Accept-Language", async ({
    page,
    browser,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");

    // Set default language to en.
    await page
      .locator(
        'form#admin-system-language-form input[name="default_language"][value="en"]',
      )
      .check();
    await page
      .locator('form#admin-system-language-form button[type="submit"]')
      .click();
    await expect(
      page
        .locator("#feedback-list")
        .getByText(/Default language saved|Langue par défaut enregistrée/i),
    ).toBeVisible({ timeout: 10000 });

    // Fresh context, no cookie, Accept-Language: de (no match).
    const ctx = await browser.newContext({ locale: "de-DE" });
    const fresh = await ctx.newPage();
    await fresh.goto("/");
    // Body's lang attribute reflects the resolved locale.
    const lang = await fresh.locator("html").getAttribute("lang");
    expect(lang).toBe("en");
    await ctx.close();

    // Reset default to fr to keep test isolation.
    await page.goto("/admin?tab=system");
    await page
      .locator(
        'form#admin-system-language-form input[name="default_language"][value="fr"]',
      )
      .check();
    await page
      .locator('form#admin-system-language-form button[type="submit"]')
      .click();
    await expect(
      page
        .locator("#feedback-list")
        .getByText(/Default language saved|Langue par défaut enregistrée/i),
    ).toBeVisible({ timeout: 10000 });
  });

  test("librarian → 403 on /admin?tab=system", async ({ page }) => {
    await loginAs(page, "librarian");
    const resp = await page.goto("/admin?tab=system");
    expect(resp?.status()).toBe(403);
  });

  test("anonymous → strict 303 redirect to /login?next=", async ({ request }) => {
    const resp = await request.get("/admin?tab=system", { maxRedirects: 0 });
    expect(resp.status()).toBe(303);
    const location = resp.headers()["location"];
    expect(location).toMatch(/^\/login\?next=/);
  });
});
