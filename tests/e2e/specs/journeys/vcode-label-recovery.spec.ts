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
 * book and re-scanned. The label used to answer "déjà assigné à ?". Story
 * 19-3 (D9) settled it: the label of a volume in the Trash is refused, both on
 * /catalog and on the title page, naming that volume's title and pointing at
 * the Trash (restore it, or delete it permanently) — reusing it would destroy
 * the trashed copy the Trash promises to keep for 30 days.
 *
 * Spec ID "VR" — generated ISBNs never collide with other specs.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { specVcode } from "../../helpers/vcode";
import { trashVolumeFromTitlePage } from "../../helpers/catalog";
import { expectHtmxProcessed } from "../../helpers/htmx";

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
  // Confirm only once HTMX has wired the swapped-in form: clicked earlier, it
  // is a native GET submit that deletes nothing (story 19-3 review).
  const confirm = page.locator("#modal-slot dialog[open] [data-modal-confirm]");
  await expectHtmxProcessed(confirm);
  await Promise.all([
    page.waitForURL((url) => !leavingUrl.test(url.pathname), {
      timeout: 15000,
    }),
    confirm.click(),
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
    const vcode = specVcode("VR", 0);
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
    await scanField.fill(vcode);
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

  test("#442 — a deleted volume's label is refused, naming its title and the Trash", async ({
    page,
  }) => {
    const label = specVcode("VR", 1);
    const firstIsbn = specIsbn("VR", 2);
    const secondIsbn = specIsbn("VR", 3);
    // The title named is the first book's (asserted by the Rust tests; its
    // name here depends on when the mock metadata landed).
    const refusal =
      /belongs to a volume of .+ that is in the Trash|appartient à un volume de .+ qui est dans la corbeille/i;

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
    await trashVolumeFromTitlePage(page, label);

    // The same physical sticker goes on a different book: /catalog refuses.
    await page.goto("/catalog");
    await catalogTitle(page, secondIsbn);
    await scanField.fill(label);
    await scanField.press("Enter");

    const feedback = page.locator(".feedback-entry").filter({ hasText: refusal });
    await expect(feedback).toBeVisible({ timeout: 10000 });
    await expect(feedback).toContainText(label);
    await expect(feedback).not.toContainText(/already assigned|déjà assigné/i);

    // The title page of the second book answers the same refusal, next to
    // its field, and adds nothing.
    await openTitlePage(page, secondIsbn);
    const field = page.locator("#title-volume-vcode");
    await field.fill(label);
    await field.press("Enter");
    const answer = page.locator("#title-volume-add-feedback");
    await expect(answer).toContainText(refusal, { timeout: 10000 });
    await expect(page.locator("#title-volumes-list")).not.toContainText(label);
  });
});
