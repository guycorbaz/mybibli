import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";

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
    await loginAs(page, "librarian");
    const isbn = specIsbn("PV", 1);

    // Scan ISBN + first V-code — count goes 0 → 1, guard does NOT trigger
    // (the helper waits for the 1st volume's feedback).
    await scanTitleAndVolume(page, isbn, "V0300");

    // Scan a 2nd V-code via the catalog form. The guard fires because the
    // active title now has 1 volume — the response is the modal markup
    // retargeted to #modal-slot, NOT a new volume.
    await page.goto("/catalog");
    await page.locator("#scan-field").fill("V0301");
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
      page.locator(".feedback-entry").filter({ hasText: /V0301/i }).first(),
    ).toBeVisible({ timeout: 5000 });
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);
  });

  test("modal Cancel closes without creating a volume", async ({ page }) => {
    await loginAs(page, "librarian");
    const isbn = specIsbn("PV", 2);
    await scanTitleAndVolume(page, isbn, "V0302");

    await page.goto("/catalog");
    await page.locator("#scan-field").fill("V0303");
    await page.locator("#scan-field").press("Enter");

    const modal = page.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible();

    // UX-DR8 invariant: Cancel button has data-modal-cancel.
    await modal.locator("[data-modal-cancel]").click();
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0);

    // No feedback entry mentions V0303 — Cancel did not create the volume.
    await expect(
      page.locator(".feedback-entry").filter({ hasText: /V0303/i }),
    ).toHaveCount(0);
  });
});
