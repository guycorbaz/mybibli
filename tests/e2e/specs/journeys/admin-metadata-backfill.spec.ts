import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

/**
 * #389 Palier 1 — admin metadata-backfill button on the Health panel.
 *
 * Verifies the new admin action end-to-end: the button renders on the
 * Health panel (template + 4-locale label wiring) and POSTing to its
 * route runs the handler through CSRF and returns a valid feedback.
 *
 * #439 renamed the button: the action now draws on both national libraries
 * (BnF and Library of Congress), so the copy no longer names a single one.
 * The matcher below accepts the current wording in all four locales.
 *
 * The backfill shares the single stack-wide bulk-metadata status lock with
 * cover-refetch, so the button's enabled state is non-deterministic under
 * parallel execution. We therefore assert the button's PRESENCE via the UI
 * and drive the route via a direct POST (CSRF-authenticated) whose outcome
 * is asserted by shape — started | empty | already-running are all valid,
 * lock-state-dependent responses. Mirrors the direct-POST style of
 * bulk-cover-refetch.spec.ts.
 */
test.describe("#389 — admin BnF metadata backfill", () => {
  const backfillButton =
    /Backfill metadata from libraries|Compléter les métadonnées via les bibliothèques|Metadaten von Bibliotheken nachladen|Recupera metadati dalle biblioteche/i;

  // Any of the three lock-state-dependent outcomes proves the handler ran.
  const validOutcome =
    /Bulk metadata backfill started|Complément des métadonnées en lot lancé|already in progress|déjà en cours|No titles with a lookup code|Aucun titre avec un code/i;

  test("backfill button renders and its route runs through CSRF", async ({
    page,
    request,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=health");

    // 1. UI wiring: the button is present with its localized label.
    await expect(
      page.getByRole("button", { name: backfillButton }),
    ).toBeVisible();

    // 2. Route end-to-end via a direct, CSRF-authenticated POST — robust to
    //    the shared bulk lock's state under parallel runs.
    const csrf = await page
      .locator('meta[name="csrf-token"]')
      .getAttribute("content");
    expect(csrf).toBeTruthy();
    const cookies = await page.context().cookies();
    const cookieHeader = cookies.map((c) => `${c.name}=${c.value}`).join("; ");

    const resp = await request.post("/admin/health/bulk-metadata-backfill", {
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
        Cookie: cookieHeader,
        "HX-Request": "true",
      },
      data: `_csrf_token=${encodeURIComponent(csrf!)}`,
      maxRedirects: 0,
    });

    // Conflict (409) is the legitimate "already running" response; 200 is
    // started/empty. Both mean the handler executed correctly.
    expect([200, 409]).toContain(resp.status());
    expect(await resp.text()).toMatch(validOutcome);
  });

  test("backfill route rejects an anonymous (unauthenticated) POST", async ({
    request,
  }) => {
    // No session cookie, no CSRF: the admin guard must not run the backfill.
    const resp = await request.post("/admin/health/bulk-metadata-backfill", {
      headers: { "HX-Request": "true" },
      data: "",
      maxRedirects: 0,
    });
    // CSRF rejection (403) or auth bounce (303/401/403) — never a 200 run.
    expect(resp.status()).not.toBe(200);
  });
});
