import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";
import { createLocation } from "../../helpers/locations";

/**
 * Issue #428 — "labels in use up to V…· L…" info line on /catalog, so
 * freshly printed label sheets start after the highest used numbers.
 *
 * Determinism in the shared parallel DB: V9999/L9999 are reserved as
 * never-created by loans.spec.ts / shelving.spec.ts (they assert the
 * not-found path on them), so once this spec creates V9998 + L9998 no
 * other spec can out-rank them — the MAX display is stable regardless
 * of scheduling order.
 */
test.describe("Issue #428 — highest V/L-code line on /catalog", () => {
  test("librarian sees the high-water marks after creating them", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    // scanTitleAndVolume is rerun-safe (the phantom-volume modal handles
    // an already-existing V9998); the location creation is NOT (UNIQUE
    // L-code collision on a persistent DB), so only create it when a
    // previous run hasn't already.
    await scanTitleAndVolume(page, specIsbn("HC", 1), "V9998");
    await page.goto("/locations");
    const existing = page.locator("text=L9998");
    if (!(await existing.isVisible({ timeout: 2000 }).catch(() => false))) {
      await createLocation(page, "HC-428 shelf", "L9998");
    }

    await page.goto("/catalog");
    const line = page.locator("#highest-codes-line");
    await expect(line).toBeVisible();
    await expect(line).toContainText("V9998");
    await expect(line).toContainText("L9998");
  });

  test("anonymous visitor does not see the line", async ({ page }) => {
    // No login — /catalog is anonymous-accessible but the label-printing
    // line is a Librarian+ affordance.
    await page.goto("/catalog");
    // Page rendered (guide strip is anonymous-visible; the scan field is
    // itself a Librarian+ affordance) — and the label line is absent.
    await expect(page.locator("#guide-strip")).toBeVisible();
    await expect(page.locator("#highest-codes-line")).toHaveCount(0);
  });
});
