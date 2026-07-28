/**
 * #441 + #442 — recovering from the two dead ends the librarian hit while
 * cataloging on 2026-07-27.
 *
 * #441: the active title is deleted, then a V-code is scanned. The scan used to
 * answer "Introuvable — l'élément a peut-être été déplacé ou supprimé", which
 * blames the label the librarian just scanned. It must instead say there is no
 * active item, and clear the stale context so the next scan works.
 *
 * #442: a volume is deleted, then its physical label is re-stuck on another
 * book and re-scanned. The label used to stay locked by the UNIQUE index —
 * "déjà assigné à ?" — until an admin permanently deleted the row from the
 * Trash. It must now be reusable, with the librarian told that the previous
 * copy's data was discarded.
 *
 * Spec ID "VR" — generated ISBNs never collide with other specs.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

async function catalogTitle(
  page: import("@playwright/test").Page,
  isbn: string,
) {
  const scanField = page.locator("#scan-field");
  await scanField.fill(isbn);
  await scanField.press("Enter");
  await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
    timeout: 10000,
  });
}

/**
 * Confirm a UX-DR8 delete modal and wait for the `HX-Redirect` it triggers to
 * land.
 *
 * Deleting a title redirects to `/`; deleting a volume redirects to
 * `/title/:id`. Navigating on our own before that redirect completes aborts it
 * — `net::ERR_ABORTED` — which is a genuine race, not a slow page. So we wait
 * for the URL to leave the page we were on rather than guessing a duration.
 */
async function confirmDeleteModal(
  page: import("@playwright/test").Page,
  leavingUrl: RegExp,
) {
  const dialog = page.locator("#modal-slot dialog[open]");
  await expect(dialog).toBeVisible();
  await Promise.all([
    page.waitForURL((url) => !leavingUrl.test(url.pathname), {
      timeout: 15000,
    }),
    dialog
      .getByRole("button", { name: /Delete|Supprimer|Confirm|Confirmer/i })
      .last()
      .click(),
  ]);
}

async function openTitlePage(
  page: import("@playwright/test").Page,
  isbn: string,
) {
  await page.goto(`/?q=${isbn}`);
  const titleLink = page
    .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
    .first();
  await expect(titleLink).toBeVisible({ timeout: 15000 });
  await page.goto((await titleLink.getAttribute("href"))!);
  await page.waitForURL(/\/title\/\d+/);
}

test.describe("Recovering a V-code label (#441, #442)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page, "librarian");
  });

  test("#441 — scanning a V-code after the active title was deleted says so plainly", async ({
    page,
  }) => {
    const isbn = specIsbn("VR", 1);

    await page.goto("/catalog");
    await catalogTitle(page, isbn);

    // Delete the title that is currently active in the scan session.
    await openTitlePage(page, isbn);
    await page
      .getByRole("button", { name: /Delete|Supprimer/i })
      .first()
      .click();
    // Title delete redirects to "/" — wait for it before navigating ourselves.
    await confirmDeleteModal(page, /^\/title\/\d+$/);

    // Back to the scan field, with the session still pointing at the dead title.
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill("V0441");
    await scanField.press("Enter");

    const feedback = page.locator(".feedback-entry").first();
    await expect(feedback).toContainText(
      /No title selected|Aucun titre sélectionné/i,
      { timeout: 10000 },
    );
    await expect(feedback).not.toContainText(
      /may have been moved or deleted|peut-être été déplacé ou supprimé/i,
    );
  });

  test("#442 — a deleted volume's label can be re-stuck on another book", async ({
    page,
  }) => {
    const firstIsbn = specIsbn("VR", 2);
    const secondIsbn = specIsbn("VR", 3);
    const label = "V0442";

    // Catalog the first book and give it the label.
    await page.goto("/catalog");
    await catalogTitle(page, firstIsbn);
    const scanField = page.locator("#scan-field");
    await scanField.fill(label);
    await scanField.press("Enter");
    await expect(
      page.locator(".feedback-entry").filter({ hasText: label }),
    ).toBeVisible({ timeout: 10000 });

    // The librarian realises it went on the wrong book and deletes the volume.
    await openTitlePage(page, firstIsbn);
    const volumeRow = page.locator("#title-volumes table tbody tr").first();
    await expect(volumeRow).toContainText(label);
    await volumeRow.getByRole("button", { name: /Delete|Supprimer/i }).click();
    // Volume delete redirects back to the same /title/:id — wait for the
    // reload rather than racing it.
    await Promise.all([
      page.waitForLoadState("load"),
      (async () => {
        const dialog = page.locator("#modal-slot dialog[open]");
        await expect(dialog).toBeVisible();
        await dialog
          .getByRole("button", { name: /Delete|Supprimer|Confirm|Confirmer/i })
          .last()
          .click();
      })(),
    ]);
    await expect(page.locator("#title-volumes")).not.toContainText(label);

    // Now the same physical sticker goes on a different book.
    await page.goto("/catalog");
    await catalogTitle(page, secondIsbn);
    await scanField.fill(label);
    await scanField.press("Enter");

    const feedback = page.locator(".feedback-entry").first();
    // It must succeed, and it must say the previous copy's data was dropped.
    await expect(feedback).toContainText(label, { timeout: 10000 });
    await expect(feedback).toContainText(
      /reused|réutilisé|wiederverwendet|riutilizzata/i,
    );
    await expect(feedback).not.toContainText(/already assigned|déjà assigné/i);

    // And the volume now hangs off the second title.
    await openTitlePage(page, secondIsbn);
    await expect(page.locator("#title-volumes")).toContainText(label);
  });
});
