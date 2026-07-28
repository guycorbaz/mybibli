import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

/**
 * #440 — cataloging several CDs in a row.
 *
 * The first UPC of a browser session goes through the MediaTypeSelector, which
 * always set the active title correctly. Once "CD" is picked, a
 * `media_type_preference` session cookie is remembered and every subsequent UPC
 * takes a different code path — the one that used to create the title without
 * ever making it the session's active scan context. The librarian then scanned
 * the volume label and it attached to the PREVIOUS title, or failed outright
 * with "Introuvable" when that title had since been deleted.
 *
 * This is the production incident of 2026-07-27 replayed as a journey: two CDs
 * in a row, then a volume label.
 *
 * Codes are 13-digit EAN product barcodes unique to this spec (the 12-digit
 * UPC-A mod-10 guard does not apply at this length). The MusicBrainz mock only
 * knows 0093624738626, so these resolve to no metadata and each title keeps its
 * raw code as its name — which makes the assertions below unambiguous about
 * WHICH title a volume landed on.
 */
const CD_FIRST = "0093624700011";
const CD_SECOND = "0093624700028";
const VOLUME_LABEL = "V0440";

async function scan(page: import("@playwright/test").Page, code: string) {
  const scanField = page.locator("[data-mybibli-scan-field]");
  await scanField.fill(code);
  await scanField.press("Enter");
}

test.describe("Cataloging consecutive CDs (#440)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("the second CD becomes the active title, and its volume attaches to it", async ({
    page,
  }) => {
    // ── First CD: goes through the MediaTypeSelector ──────────────────
    await scan(page, CD_FIRST);

    const cdButton = page.locator('button[role="radio"]', { hasText: "CD" });
    await expect(cdButton).toBeVisible({ timeout: 10000 });
    await cdButton.click();

    const banner = page.locator("#context-banner");
    await expect(banner).toContainText(CD_FIRST, { timeout: 10000 });

    // ── Second CD: the media type is now remembered, so no selector ───
    await scan(page, CD_SECOND);

    // The regression: the banner used to keep showing the FIRST CD, leaving
    // the librarian no cue that the scan context had not moved.
    await expect(banner).toContainText(CD_SECOND, { timeout: 10000 });
    await expect(banner).not.toContainText(CD_FIRST);

    // ── The volume label must land on the second CD ───────────────────
    await scan(page, VOLUME_LABEL);

    const feedback = page.locator(".feedback-entry").first();
    await expect(feedback).toContainText(VOLUME_LABEL, { timeout: 10000 });
    await expect(feedback).toContainText(CD_SECOND);
    await expect(feedback).not.toContainText(CD_FIRST);
  });

  test("re-scanning an already catalogued CD reports it as existing", async ({
    page,
  }) => {
    await scan(page, CD_FIRST);
    const cdButton = page.locator('button[role="radio"]', { hasText: "CD" });
    await expect(cdButton).toBeVisible({ timeout: 10000 });
    await cdButton.click();
    await expect(page.locator("#context-banner")).toContainText(CD_FIRST, {
      timeout: 10000,
    });

    // Same disc again — the cookie arm used to discard `is_new` and replay the
    // whole provider chain, showing the "fetching metadata" skeleton for a
    // title already in the catalog.
    await scan(page, CD_FIRST);

    const feedback = page.locator(".feedback-entry").first();
    await expect(feedback).toContainText(
      /already in your catalog|déjà dans votre catalogue/i,
      { timeout: 10000 },
    );
  });
});
