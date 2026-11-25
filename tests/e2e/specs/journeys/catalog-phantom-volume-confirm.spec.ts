import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";
import { specVcode } from "../../helpers/vcode";
import { csrfHeaders } from "../../helpers/csrf";

// CR #300 — phantom-volume guard. `current_title_id` is sticky in the session
// (set on ISBN scan, never cleared). Scanning a fresh V-code without
// re-scanning the new book's ISBN would silently attach a volume to the
// previous title. The fix: when the active title already has ≥1 volume, the
// V-code scan returns a UX-DR8 confirmation modal instead of creating blind.
// Confirm = bypass the guard (multi-copy titles), Cancel = no creation.

const MODAL_TITLE = /Add another copy\?|Ajouter un autre exemplaire|Weiteres Exemplar hinzuf|Aggiungere un'altra copia/i;
const CONFIRM_LABEL = /Add another copy|Ajouter un exemplaire|Exemplar hinzufügen|Aggiungi una copia/i;

test.describe("Catalog — phantom-volume guard on V-code scan (#300)", () => {
  test("scanning a 2nd V-code on a title that already has 1 volume opens the confirmation modal; confirm creates the volume", async ({
    page,
  }) => {
    const first = specVcode("PV", 0);
    const second = specVcode("PV", 1);
    await loginAs(page, "librarian");
    const isbn = specIsbn("PV", 1);

    // Scan ISBN + first V-code — count goes 0 → 1, guard does NOT trigger
    // (the helper waits for the 1st volume's feedback).
    await scanTitleAndVolume(page, isbn, first);

    // Scan a 2nd V-code via the catalog form. The guard fires because the
    // active title now has 1 volume — the response is the modal markup
    // retargeted to #modal-slot, NOT a new volume.
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(second);
    await page.locator("#scan-field").press("Enter");

    // Modal appears with title + confirm label localized.
    const modal = page.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible();
    await expect(modal).toContainText(MODAL_TITLE);
    const confirmBtn = modal.getByRole("button", { name: CONFIRM_LABEL });
    await expect(confirmBtn).toBeVisible();

    // Click Confirm → re-POSTs /catalog/scan with confirmed=true → creates the
    // 2nd volume (feedback contains the V-code label) and closes the modal.
    await confirmBtn.click();
    await expect(
      page.locator(".feedback-entry").filter({ hasText: new RegExp(second, "i") }).first(),
    ).toBeVisible({ timeout: 5000 });
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);
  });

  test("modal Cancel closes without creating a volume", async ({ page }) => {
    const first = specVcode("PV", 2);
    const second = specVcode("PV", 3);
    await loginAs(page, "librarian");
    const isbn = specIsbn("PV", 2);
    await scanTitleAndVolume(page, isbn, first);

    await page.goto("/catalog");
    await page.locator("#scan-field").fill(second);
    await page.locator("#scan-field").press("Enter");

    const modal = page.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible();

    // UX-DR8 invariant: Cancel button has data-modal-cancel.
    await modal.locator("[data-modal-cancel]").click();
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);

    // No feedback entry mentions the second code — Cancel did not create the volume.
    await expect(
      page.locator(".feedback-entry").filter({ hasText: new RegExp(second, "i") }),
    ).toHaveCount(0);
  });

  // Story 19-3 (AC4, #663 item 3) — the title is deleted while the modal is
  // open: the Confirm's answer says there is no title any more, clears the
  // context banner and resets the guide, and closes the modal (it used to
  // stay open over the warning).
  test("confirming after the title was deleted meanwhile closes the modal and resets the context", async ({
    page,
  }) => {
    const first = specVcode("PV", 4);
    const second = specVcode("PV", 5);
    await loginAs(page, "librarian");
    const isbn = specIsbn("PV", 3);
    await scanTitleAndVolume(page, isbn, first);

    // Re-scan the ISBN so the context banner names the title.
    await page.goto("/catalog");
    await page.locator("#scan-field").fill(isbn);
    await page.locator("#scan-field").press("Enter");
    await expect(page.locator("#context-banner")).not.toBeEmpty({ timeout: 10000 });
    await page.locator("#scan-field").fill(second);
    await page.locator("#scan-field").press("Enter");
    const modal = page.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible();

    // Meanwhile (another tab): the volume, then the title, are deleted.
    const headers = { ...(await csrfHeaders(page)), "HX-Request": "true" };
    const volumePage = await page.request.get(`/scan?code=${first}`);
    const volumeId = /\/volume\/(\d+)/.exec(volumePage.url())?.[1];
    expect(volumeId, volumePage.url()).toBeTruthy();
    expect((await page.request.delete(`/volume/${volumeId}`, { headers })).ok()).toBe(true);
    const titlePage = await page.request.get(`/scan?code=${isbn}`);
    const titleId = /\/title\/(\d+)/.exec(titlePage.url())?.[1];
    expect(titleId, titlePage.url()).toBeTruthy();
    expect((await page.request.delete(`/title/${titleId}`, { headers })).ok()).toBe(true);

    await modal.getByRole("button", { name: CONFIRM_LABEL }).click();

    await expect(
      page.locator(".feedback-entry").filter({ hasText: /No title selected|Aucun titre sélectionné/i }).first(),
    ).toBeVisible({ timeout: 5000 });
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);
    await expect(page.locator("#context-banner")).toBeEmpty();
    await expect(
      page.locator(".feedback-entry").filter({ hasText: new RegExp(second, "i") }),
    ).toHaveCount(0);
  });
});
