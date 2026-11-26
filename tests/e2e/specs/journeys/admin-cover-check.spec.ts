/**
 * Story 19-9 E2E (#658, AC7) — the cover check and its repair.
 *
 * Real journey from a blank browser (loginAs admin): the spec makes an
 * orphan cover file of its own with existing flows only — a title of its
 * own ISBN (specIsbn("KC", 1)), a cover uploaded on its page, the title
 * deleted, then permanently deleted from the Trash (D13: the purge leaves
 * the file, named after the title id). Admin → Backups → "Check covers"
 * lists that file; "Repair…" opens the UX-DR8 modal; "Repair" deletes it
 * (one retry when a backup or restore holds the guard, D11 — parallel specs
 * write backups); a new check no longer lists it.
 *
 * The repair acts on the whole instance, so this spec runs in its own
 * Playwright project after every other test (`instance-cover-check`,
 * playwright.config.ts); run it alone with `--no-deps`.
 *
 * Rule #23: assertions only on this spec's own file, never on counts. A
 * missing file cannot be made from the browser (the covers folder is inside
 * the container): tests/cover_consistency.rs covers it.
 */
import { test, expect, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { simulateScan } from "../../helpers/scanner";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { expectHtmxProcessed } from "../../helpers/htmx";

const UPLOAD_BTN = /Upload cover|Téléverser une couverture|Cover hochladen|Carica una copertina/i;
const CHECK_BTN = /Check covers|Contrôler les couvertures|Cover prüfen|Controlla le copertine/i;
const REPAIR_OPEN = /^(Repair…|Réparer…|Reparieren…|Ripara…)$/;
const BUSY =
  /backup or a restore is already running|sauvegarde ou une restauration est déjà en cours|Sicherung oder eine Wiederherstellung|già in corso un backup o un ripristino/i;
const REPAIRED = /Covers repaired|Couvertures réparées|Cover repariert|Copertine riparate/;

/** A valid 1×1 RGBA PNG. */
const PNG_1X1 = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

/** Scan `isbn` on /catalog; the new title's id once its metadata is in. */
async function scanTitle(page: Page, isbn: string): Promise<string> {
  await page.goto("/catalog");
  await simulateScan(page, "#scan-field", isbn);
  const id = await titleIdFromSkeleton(page);
  // The mock answers "Test Title <isbn>": wait for it, so the Trash search
  // below finds the title by its ISBN.
  await expect(async () => {
    await page.goto(`/title/${id}`);
    await expect(page.locator("h1").filter({ hasText: isbn })).toHaveCount(1);
  }).toPass({ timeout: 30000 });
  return id;
}

async function uploadCover(page: Page, id: string): Promise<void> {
  await page.goto(`/title/${id}`);
  await page.getByRole("button", { name: UPLOAD_BTN }).click();
  const fileInput = page.locator('#modal-slot input[type="file"]');
  await expect(fileInput).toBeVisible();
  await fileInput.setInputFiles({ name: "cover.png", mimeType: "image/png", buffer: PNG_1X1 });
  const confirm = page.locator("#modal-slot [data-modal-confirm]");
  await expectHtmxProcessed(confirm);
  await confirm.click();
  await expect(async () => {
    await page.goto(`/title/${id}`);
    await expect(page.locator(`img[src*="/covers/${id}.jpg"]`).first()).toBeVisible();
  }).toPass({ timeout: 20000 });
}

async function deleteTitle(page: Page, id: string): Promise<void> {
  await page.goto(`/title/${id}`);
  await page.locator("#title-delete-button").click();
  const dialog = page.locator("#modal-slot dialog[open]");
  await expect(dialog).toBeVisible({ timeout: 10000 });
  const confirm = dialog.locator("[data-modal-confirm]");
  await expectHtmxProcessed(confirm);
  await confirm.click();
  await page.waitForURL((url) => url.pathname === "/", { timeout: 10000 });
}

/**
 * Permanently delete title `id` from the Trash (type-to-confirm modal).
 * A direct navigation to `/admin/trash?…` drops the filters
 * (`admin_trash_panel` renders the default panel), so the Trash tab is
 * opened and searched through its own field (an HTMX panel swap on keyup:
 * `fill` alone fires none, hence the key press). The ISBN is this spec's
 * own, so the search alone narrows the list to its title.
 */
async function purgeTitle(page: Page, id: string, isbn: string): Promise<void> {
  const panelSwap = (needle: string) =>
    page.waitForResponse(
      (r) =>
        new URL(r.url()).pathname === "/admin/trash" &&
        r.request().method() === "GET" &&
        r.url().includes(needle) &&
        r.ok(),
    );
  await page.goto("/admin?tab=trash");
  const search = page.locator("#search-trash");
  await expectHtmxProcessed(search);
  const searched = panelSwap(`search=${isbn}`);
  await search.fill(isbn);
  await search.press("End");
  await searched;
  const button = page.locator(`button[hx-get^="/admin/trash/titles/${id}/permanent-delete"]`);
  await expect(button).toHaveCount(1, { timeout: 10000 });
  const row = page.locator("tbody tr").filter({ has: button });
  const name = ((await row.locator("td").first().textContent()) ?? "").trim();
  expect(name).toContain(isbn);
  await button.click();
  const modal = page.locator("#modal-slot dialog[open]");
  await expect(modal).toBeVisible({ timeout: 10000 });
  await modal.locator('input[type="text"]').fill(name);
  const confirm = modal.locator("[data-modal-confirm]");
  await expect(confirm).toBeEnabled();
  await confirm.click();
  await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0, { timeout: 10000 });
  await expect(page.locator(`button[hx-get^="/admin/trash/titles/${id}/permanent-delete"]`)).toHaveCount(0);
}

/** Press "Check covers" on the Backups tab; the result region. */
async function checkCovers(page: Page) {
  const response = page.waitForResponse(
    (r) => new URL(r.url()).pathname === "/admin/covers/check" && r.request().method() === "POST",
  );
  await page.getByRole("button", { name: CHECK_BTN }).click();
  expect((await response).ok()).toBe(true);
  return page.locator("#admin-covers-result");
}

test.describe("Story 19-9 — cover check and repair", () => {
  test("an orphan cover file is listed, repaired through the modal, and gone", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    const isbn = specIsbn("KC", 1);
    const id = await scanTitle(page, isbn);
    await uploadCover(page, id);
    await deleteTitle(page, id);
    await purgeTitle(page, id, isbn);

    // The check lists the file the purge left behind (D13).
    const own = `${id}.jpg · `;
    await page.goto("/admin?tab=backups");
    let result = await checkCovers(page);
    await expect(result.locator("#admin-covers-orphans")).toContainText(own);

    // "Repair…" opens the UX-DR8 modal; "Repair" deletes. One retry when
    // a backup or a restore holds the guard (D11).
    const open = page.getByRole("button", { name: REPAIR_OPEN });
    await expectHtmxProcessed(open);
    await open.click();
    const modal = page.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 10000 });
    const confirm = modal.locator("[data-modal-confirm]");
    await expectHtmxProcessed(confirm);
    let presses = 0;
    await expect(async () => {
      if (presses === 2) return;
      presses += 1;
      const answer = page.waitForResponse(
        (r) => new URL(r.url()).pathname === "/admin/covers/repair" && r.request().method() === "POST",
        { timeout: 60000 },
      );
      await confirm.click();
      const body = await (await answer).text();
      expect(body, "the guard was free").not.toMatch(BUSY);
    }).toPass({ intervals: [5000], timeout: 150000 });
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0, { timeout: 10000 });
    await expect(page.locator("#admin-covers-result")).toContainText(REPAIRED);
    await expect(page.locator("#admin-covers-result")).not.toContainText(own);

    // A new check no longer lists it.
    result = await checkCovers(page);
    await expect(result).not.toContainText(REPAIRED);
    await expect(result).not.toContainText(own);
  });
});
