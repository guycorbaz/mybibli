/**
 * #403 E2E — i18n-bundle sweep (inline-form.js + mybibli.js).
 *
 * The de/it copy is the net-new content of #403: the two strings that
 * used to live as hand-synced {en, fr} objects in JS now come from the
 * server-rendered #i18n-bundle data island, resolved in the request
 * locale. Assert end-to-end (real browser, real locale negotiation)
 * that the island carries localized, non-fallback values for all four
 * locales — a regression here silently re-opens the de/it gap.
 *
 * Read-only spec (no DB writes) — parallel-safe.
 */
import { test, expect } from "@playwright/test";

async function bundleFor(
  browser: import("@playwright/test").Browser,
  locale: string,
): Promise<{ modalBusy: string; serverErrorRetry: string }> {
  const ctx = await browser.newContext({ locale });
  const page = await ctx.newPage();
  await page.goto("/");
  const raw = await page.locator("script#i18n-bundle").textContent();
  await ctx.close();
  expect(raw, `#i18n-bundle island must render for locale ${locale}`).toBeTruthy();
  const parsed = JSON.parse(raw!);
  return {
    modalBusy: parsed?.inline_form?.modal_busy ?? "",
    serverErrorRetry: parsed?.errors?.server_error_retry ?? "",
  };
}

test.describe("#403 — i18n-bundle sweep keys", () => {
  test("island carries the two #403 strings localized in en/fr/de/it", async ({
    browser,
  }) => {
    const en = await bundleFor(browser, "en-US");

    // Baseline: non-empty EN values, %{status} preserved for client-side
    // substitution (mybibli.js replaces it with the real HTTP status).
    expect(en.modalBusy.length).toBeGreaterThan(0);
    expect(en.serverErrorRetry).toContain("%{status}");

    for (const locale of ["fr", "de", "it"]) {
      const v = await bundleFor(browser, locale);
      expect(v.modalBusy.length, `${locale} modal_busy`).toBeGreaterThan(0);
      expect(v.serverErrorRetry, `${locale} server_error_retry`).toContain(
        "%{status}",
      );
      // Translated copy, not the EN fallback — the exact gap #403 closes.
      expect(v.modalBusy, `${locale} modal_busy must differ from en`).not.toBe(
        en.modalBusy,
      );
      expect(
        v.serverErrorRetry,
        `${locale} server_error_retry must differ from en`,
      ).not.toBe(en.serverErrorRetry);
    }
  });
});
