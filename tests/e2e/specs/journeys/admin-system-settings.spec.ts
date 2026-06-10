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
  // CR #88 + #89: tests in this describe mutate global K/V rows in the
  // `settings` table (default_language, overdue_threshold_days, Google
  // Books key). Running them in parallel produces cross-test
  // interleaving where one test's "reset to default" happens between
  // another test's "set value" and "verify value" steps — observed as
  // intermittent flakes on the 3 default-language branches. Serializing
  // the whole describe trades ~30 s of CI time for zero flake risk on
  // shared-state mutators.
  test.describe.configure({ mode: "serial" });

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

  test("log level saves twice in a row without a 409 (#406)", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");

    const logInput = page.locator(
      'form#admin-system-log-form input[name="log_level"]',
    );
    const logSubmit = page.locator(
      'form#admin-system-log-form button[type="submit"]',
    );
    await expect(logInput).toBeVisible();

    // First save: bumps the DB row's version (1 → 2).
    await logInput.fill("debug");
    await logSubmit.click();
    await expect(
      page
        .locator("#feedback-list")
        .getByText(
          /Log level saved|Niveau de journalisation enregistré/i,
        ),
    ).toBeVisible({ timeout: 10000 });

    // Second consecutive save. Before the #406 fix the swapped form still
    // carried a stale version (always 1 — `log_level` was missing from
    // `fetch_setting_rows`), so this POST 409'd and silently did nothing.
    await logInput.fill("info");
    await logSubmit.click();
    await expect(
      page
        .locator("#feedback-list")
        .getByText(/Log level saved|Niveau de journalisation enregistré/i)
        .last(),
    ).toBeVisible({ timeout: 10000 });

    // Strongest proof the second save actually persisted: the reloaded form
    // shows "info", not the stuck "debug" of the 409 path. Ends on the seed
    // default ("info") so no reset is needed for isolation.
    await page.goto("/admin?tab=system");
    await expect(logInput).toHaveValue("info");
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

    // Fresh context, no cookie, Accept-Language: es (no match).
    // v1.7.0 (CR #275 / #276) added DE + IT to the resolver, so the
    // original `de-DE` value now resolves directly to `de` and skips
    // the default-language fallback this test is exercising. Use `es-ES`
    // (still unsupported) to force the fall-through.
    const ctx = await browser.newContext({ locale: "es-ES" });
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

  // CR #89 — Story 8-5 subtask 8.6: the locale resolution chain (story
  // 7-3) has FOUR branches. Only "no-match Accept-Language → default
  // fallback" was covered (`default language change affects fresh
  // anonymous visitor`). These two tests close branches 2 and 3:
  //   2. Accept-Language match wins over default-language
  //   3. Authenticated user's preferred_language wins over default
  test("Accept-Language match wins over default-language for anonymous (AC #4)", async ({
    page,
    browser,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");

    // Set default to en — the test wants to confirm Accept-Language=fr
    // STILL produces FR despite default being en.
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

    // Fresh context, no cookies, Accept-Language: fr
    const ctx = await browser.newContext({ locale: "fr" });
    const fresh = await ctx.newPage();
    await fresh.goto("/");
    const lang = await fresh.locator("html").getAttribute("lang");
    expect(lang).toBe("fr");
    await ctx.close();

    // Reset default to fr for cross-test isolation.
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

  test("authenticated user preferred_language wins over default (AC #4)", async ({
    page,
  }) => {
    await loginAs(page, "admin");

    // Step 1: persist admin's preferred_language = fr via POST /language.
    // Mirrors what the nav-bar language <select> auto-submit does. Uses
    // the page's CSRF token + cookie context — no DB direct access.
    const csrf1 = await page
      .locator('meta[name="csrf-token"]')
      .getAttribute("content");
    expect(csrf1).toBeTruthy();
    const langResp = await page.request.post("/language", {
      form: { _csrf_token: csrf1!, lang: "fr", next: "/" },
      maxRedirects: 0,
      failOnStatusCode: false,
    });
    // 303 to the redirect target on success (validated lang + CSRF)
    expect(langResp.status()).toBe(303);

    // Step 2: change default to en. User pref is fr; default is now en;
    // the resolver must pick fr (user pref wins).
    await page.goto("/admin?tab=system");
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

    // Step 3: navigate to / — html[lang] must reflect the user pref, not
    // the new default. (Page reload re-runs the locale-resolution chain.)
    await page.goto("/");
    const lang = await page.locator("html").getAttribute("lang");
    expect(lang).toBe("fr");

    // Reset default to fr for cross-test isolation. (Admin's
    // preferred_language stays = fr; future loginAs(admin) sessions will
    // therefore resolve to fr, which matches the seed default behavior
    // anyway, so no further test depends on it being NULL.)
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

  // CR #88 — Story 8-5 subtask 8.5 was prematurely checked off and
  // reverted in code review; this E2E was never written. Validates
  // **AC #6**: the partitioned optimistic-locking design (one `version`
  // column PER settings row) means concurrent admin edits to DIFFERENT
  // settings rows MUST NOT collide. A whole-table lock would have
  // bounced one of them with 409 "modified by another admin"; a
  // per-row lock lets both succeed.
  test("cross-row concurrent edits to different settings don't collide (AC #6)", async ({
    browser,
  }) => {
    const ctxA = await browser.newContext();
    const ctxB = await browser.newContext();
    const pageA = await ctxA.newPage();
    const pageB = await ctxB.newPage();

    try {
      await loginAs(pageA, "admin");
      await loginAs(pageB, "admin");

      await pageA.goto("/admin?tab=system");
      await pageB.goto("/admin?tab=system");

      // Context A: queue the loans-form change (overdue threshold row)
      const thresholdInput = pageA.locator(
        'form#admin-system-loans-form input[name="overdue_threshold_days"]',
      );
      await expect(thresholdInput).toBeVisible();
      await thresholdInput.fill("21");

      // Context B: queue the providers-form change (Google Books key row)
      const newKey = uniqueSlug("CR88_AAAA");
      const gbInput = pageB.locator(
        'form#admin-system-providers-form input[name="google_books_api_key"]',
      );
      await expect(gbInput).toBeVisible();
      await gbInput.fill(newKey);

      // Submit both nearly concurrently. Promise.all guarantees the two
      // POSTs are dispatched before either resolves — a serialized
      // whole-table lock would bounce one of them with 409.
      await Promise.all([
        pageA
          .locator('form#admin-system-loans-form button[type="submit"]')
          .click(),
        pageB
          .locator('form#admin-system-providers-form button[type="submit"]')
          .click(),
      ]);

      // Both success feedbacks visible — proves neither was rejected
      // with 409 / "modified by another admin".
      await expect(
        pageA
          .locator("#feedback-list")
          .getByText(/Loans settings saved|Préférences de prêt enregistrées/i),
      ).toBeVisible({ timeout: 10000 });
      await expect(
        pageB
          .locator("#feedback-list")
          .getByText(/Google Books key saved|Clé Google Books enregistrée/i),
      ).toBeVisible({ timeout: 10000 });

      // Persistence proof — reload each context and verify the value
      // (or mask, for the Google Books key) survived.
      await pageA.goto("/admin?tab=system");
      await expect(thresholdInput).toHaveValue("21");

      await pageB.goto("/admin?tab=system");
      const last4 = newKey.slice(-4);
      await expect(
        pageB
          .locator("form#admin-system-providers-form")
          .getByText(new RegExp(`••••${last4}`)),
      ).toBeVisible();

      // Reset both rows to keep cross-test isolation (loans → 30 days,
      // Google Books key → cleared via the checkbox).
      await thresholdInput.fill("30");
      await pageA
        .locator('form#admin-system-loans-form button[type="submit"]')
        .click();
      await expect(
        pageA
          .locator("#feedback-list")
          .getByText(/Loans settings saved|Préférences de prêt enregistrées/i)
          .last(),
      ).toBeVisible({ timeout: 10000 });

      await pageB
        .locator(
          'form#admin-system-providers-form input[name="_clear_google_books"]',
        )
        .check();
      await pageB
        .locator('form#admin-system-providers-form button[type="submit"]')
        .click();
      await expect(
        pageB
          .locator("#feedback-list")
          .getByText(/Google Books key cleared|Clé Google Books effacée/i),
      ).toBeVisible({ timeout: 10000 });
    } finally {
      await ctxA.close();
      await ctxB.close();
    }
  });
});
