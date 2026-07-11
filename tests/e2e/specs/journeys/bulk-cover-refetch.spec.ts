import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

/**
 * Issue #419 — bulk cover-refetch pacing + completion summary.
 *
 * Prod evidence (2026-07-10): two back-to-back runs over ~113 titles at
 * ~1.2 s/title tripped Google Books' throttling (503 storm) and both
 * runs "completed" silently with ~0 covers — indistinguishable from
 * "no covers exist". This spec covers the two admin-visible halves of
 * the fix:
 *   1. the new inter-title delay knob on the /admin > System timeouts
 *      form (save, re-render, server-side validation), and
 *   2. the Health-panel completion summary (recovered / provider
 *      errors / no cover available) after a real bulk run.
 *
 * Serial: both tests mutate the global `bulk_refetch_delay_ms` K/V row,
 * and the bulk run itself is single-instance stack-wide.
 */
test.describe("Issue #419 — bulk cover-refetch pacing + summary", () => {
  test.describe.configure({ mode: "serial" });

  const delayInput = 'form#admin-system-timeouts-form input[name="bulk_refetch_delay_ms"]';
  const timeoutsSubmit = 'form#admin-system-timeouts-form button[type="submit"]';
  const timeoutsSaved = /Timeouts saved|Délais d'attente enregistrés/i;

  test("bulk refetch delay saves, re-renders, and rejects out-of-range", async ({
    page,
    request,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");

    const input = page.locator(delayInput);
    await expect(input).toBeVisible();
    // No seed-value assertion here: the shared E2E DB persists across
    // runs, so the row may hold whatever the previous invocation saved.
    // The save → reload → re-render cycle below is the real contract.

    await input.fill("2500");
    await page.locator(timeoutsSubmit).click();
    await expect(
      page.locator("#feedback-list").getByText(timeoutsSaved),
    ).toBeVisible({ timeout: 10000 });

    // Persisted value re-renders on a fresh page load.
    await page.goto("/admin?tab=system");
    await expect(page.locator(delayInput)).toHaveValue("2500");

    // Server-side validation: 60001 > 60000 → 400. Direct POST because
    // the browser input's max attribute blocks the UI path.
    const realToken = await page
      .locator('meta[name="csrf-token"]')
      .getAttribute("content");
    expect(realToken).toBeTruthy();
    const cookies = await page.context().cookies();
    const cookieHeader = cookies.map((c) => `${c.name}=${c.value}`).join("; ");
    const chainVersion = await page
      .locator('form#admin-system-timeouts-form input[name="metadata_chain_timeout_version"]')
      .getAttribute("value");
    const probeVersion = await page
      .locator('form#admin-system-timeouts-form input[name="provider_health_timeout_version"]')
      .getAttribute("value");
    const delayVersion = await page
      .locator('form#admin-system-timeouts-form input[name="bulk_refetch_delay_version"]')
      .getAttribute("value");
    const resp = await request.post("/admin/system/timeouts", {
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
        Cookie: cookieHeader,
        "HX-Request": "true",
      },
      data:
        `metadata_chain_timeout_secs=5&metadata_chain_timeout_version=${chainVersion}` +
        `&provider_health_timeout_secs=10&provider_health_timeout_version=${probeVersion}` +
        `&bulk_refetch_delay_ms=60001&bulk_refetch_delay_version=${delayVersion}` +
        `&_csrf_token=${encodeURIComponent(realToken!)}`,
      maxRedirects: 0,
    });
    expect(resp.status()).toBe(400);

    // Prep for the journey test: 0 ms = no pacing, so the bulk run over
    // the shared E2E DB's accumulated cover-less titles stays fast.
    await page.goto("/admin?tab=system");
    const prepInput = page.locator(delayInput);
    await prepInput.fill("0");
    await page.locator(timeoutsSubmit).click();
    await expect(
      page.locator("#feedback-list").getByText(timeoutsSaved).last(),
    ).toBeVisible({ timeout: 10000 });
  });

  test("bulk refetch run completes with a summary on the Health panel", async ({
    page,
  }) => {
    // The run iterates every missing-cover title in the shared E2E DB
    // (synthetic BnF catch-all titles never get covers), so give the
    // whole journey a generous budget even at 0 ms pacing.
    test.setTimeout(300_000);

    await loginAs(page, "admin");

    // Guarantee at least one missing-cover title exists regardless of
    // suite composition: the BnF catch-all resolves this unique ISBN
    // with metadata but no cover URL.
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(specIsbn("BF", 1));
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
      timeout: 10000,
    });

    await page.goto("/admin?tab=health");
    const refetchButton = page.getByRole("button", {
      name: /Re-fetch missing covers|Récupérer les couvertures manquantes/i,
    });
    await expect(refetchButton).toBeEnabled();
    await refetchButton.click();
    await expect(
      page.getByText(
        /Bulk cover-refetch started|Récupération en lot lancée/i,
      ),
    ).toBeVisible({ timeout: 10000 });

    // Poll the Health panel until the run completes and the #419
    // summary label lands. Counts are non-deterministic (shared DB) —
    // assert the shape, not the numbers.
    const summaryPattern =
      /(covers recovered|couvertures récupérées).*(provider errors|erreurs fournisseur)/i;
    await expect(async () => {
      await page.goto("/admin?tab=health");
      await expect(page.getByText(summaryPattern)).toBeVisible({
        timeout: 2000,
      });
    }).toPass({ timeout: 240_000, intervals: [5_000] });

    // Reset the shared delay row to the seeded default so the knob does
    // not leak into other suites/runs.
    await page.goto("/admin?tab=system");
    const input = page.locator(delayInput);
    await input.fill("1000");
    await page.locator(timeoutsSubmit).click();
    await expect(
      page.locator("#feedback-list").getByText(timeoutsSaved).last(),
    ).toBeVisible({ timeout: 10000 });
  });
});
