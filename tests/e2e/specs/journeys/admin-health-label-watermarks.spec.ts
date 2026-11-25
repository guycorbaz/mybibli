import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";
import { createLocation } from "../../helpers/locations";
import { specVcode } from "../../helpers/vcode";
import { trashVolumeFromTitlePage } from "../../helpers/catalog";

/**
 * Issue #489 — Admin → Health shows the last V-code / L-code in use and
 * the next free number, so a fresh sheet of barcode labels starts right
 * after the occupied range.
 *
 * Smoke journey (Foundation Rule #7): blank browser → real login →
 * create a shelf and a volume → open Admin → Health → read both values.
 *
 * Parallel-safe by design: other specs create volumes and locations at
 * the same time, so the spec never asserts that ITS code is the highest.
 * It asserts the two invariants that hold whatever the neighbours do:
 * the displayed highest is at least the code it just created, and the
 * displayed "next" is exactly highest + 1.
 */
const ISBN = specIsbn("HL", 1);
// V-codes come from the spec's `specVcode` block. L-codes: L7489 here;
// home-search uses L7500–L7509 of the same bucket (grep before changing).
const LCODE = "L7489";

const WATERMARK =
  /^([VL]\d{4}) \((?:next|prochain|nächste|prossimo)\s?: ([VL]\d{4})\)$/;

function codeNumber(code: string): number {
  return Number.parseInt(code.slice(1), 10);
}

/**
 * Create the spec's shelf once. A second run against the same stack (the
 * local dev loop, not CI) finds the L-code already taken and the create
 * form answers "already in use" — the watermark assertions only need the
 * shelf to EXIST, so skip creation when its name is already listed.
 */
async function ensureLocation(page: Page, name: string, lcode: string) {
  await page.goto("/locations");
  if (await page.getByText(name, { exact: true }).count()) {
    return;
  }
  await createLocation(page, name, lcode);
}

async function readWatermark(
  page: Page,
  id: string,
): Promise<{ highest: string; next: string }> {
  const text = (await page.locator(`#${id}`).innerText()).trim();
  const m = WATERMARK.exec(text);
  expect(m, `${id} should read "X0000 (next: X0001)", got "${text}"`).not.toBeNull();
  return { highest: m![1]!, next: m![2]! };
}

test.describe("Issue #489 — Health tab label watermarks", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("shows the highest V-code and L-code with the next free number", async ({
    page,
  }) => {
    const VCODE = specVcode("HL", 0);
    await ensureLocation(page, "HL-Watermark", LCODE);
    await scanTitleAndVolume(page, ISBN, VCODE);

    await page.goto("/admin?tab=health");
    await expect(
      page.getByRole("heading", {
        name: /Barcode labels|Étiquettes à code-barres|Barcode-Etiketten|Etichette con codice a barre/i,
      }),
    ).toBeVisible();

    const v = await readWatermark(page, "health-last-vcode");
    expect(v.highest.startsWith("V")).toBe(true);
    expect(codeNumber(v.highest)).toBeGreaterThanOrEqual(codeNumber(VCODE));
    expect(codeNumber(v.next)).toBe(codeNumber(v.highest) + 1);

    const l = await readWatermark(page, "health-last-lcode");
    expect(l.highest.startsWith("L")).toBe(true);
    expect(codeNumber(l.highest)).toBeGreaterThanOrEqual(codeNumber(LCODE));
    expect(codeNumber(l.next)).toBe(codeNumber(l.highest) + 1);
  });

  test("the watermark survives the volume going to the trash", async ({
    page,
  }) => {
    // A printed sticker outlives the row: once the volume is soft-deleted,
    // the reported highest must not drop below the code it carried.
    const isbn = specIsbn("HL", 2);
    const vcode = specVcode("HL", 1);
    await scanTitleAndVolume(page, isbn, vcode);

    await page.goto("/admin?tab=health");
    const before = await readWatermark(page, "health-last-vcode");
    expect(codeNumber(before.highest)).toBeGreaterThanOrEqual(codeNumber(vcode));

    // Trash the volume from its title page (same journey as #442).
    await page.goto(`/?q=${isbn}`);
    const titleLink = page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first();
    await expect(titleLink).toBeVisible({ timeout: 15000 });
    await page.goto((await titleLink.getAttribute("href"))!);
    await page.waitForURL(/\/title\/\d+/);
    await trashVolumeFromTitlePage(page, vcode);

    await page.goto("/admin?tab=health");
    const after = await readWatermark(page, "health-last-vcode");
    expect(codeNumber(after.highest)).toBeGreaterThanOrEqual(codeNumber(vcode));
    expect(codeNumber(after.highest)).toBeGreaterThanOrEqual(codeNumber(before.highest));
  });
});
