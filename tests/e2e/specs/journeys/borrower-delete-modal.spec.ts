import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { createBorrower } from "../../helpers/loans";
import { simulateScan } from "../../helpers/scanner";

// Story 9-10 — UX-DR8 Modal foundation + first migration (delete borrower).
// Smoke test exercising:
//   - Modal opens via hx-get → fragment swap into #modal-slot.
//   - Cancel button receives initial focus (UX-DR8 invariant).
//   - Escape closes the modal.
//   - Tab cycles inside the modal (focus trap).
//   - Confirm submits → HX-Redirect → /borrowers + borrower removed.
//   - Scanner-guard 7-5 protection inherited automatically: while the
//     modal is open, a USB barcode burst MUST NOT leak into background
//     fields.

test.describe("Borrower delete modal (Story 9-10)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page, "admin");
  });

  test("smoke: open modal → focus cancel → tab → confirm → borrower deleted", async ({
    page,
  }) => {
    const NAME = `BD-Delete-${Date.now() % 100000}`;

    await createBorrower(page, NAME);

    // Navigate to borrower detail.
    const link = page
      .locator('a[href^="/borrower/"]')
      .filter({ hasText: new RegExp(`^\\s*${NAME}\\s*$`) });
    await link.click();
    await expect(page.locator("h1")).toContainText(NAME, { timeout: 5000 });

    // Click the Delete trigger.
    const deleteBtn = page.locator("button[data-modal-trigger]");
    await expect(deleteBtn).toBeVisible({ timeout: 3000 });
    await deleteBtn.click();

    // Modal opens — assert dialog is visible inside #modal-slot.
    const dialog = page.locator("#modal-slot dialog[open]");
    await expect(dialog).toBeVisible({ timeout: 5000 });

    // Cancel must be the focused element (UX-DR8 invariant).
    const cancelBtn = page.locator("[data-modal-default-focus]");
    await expect(cancelBtn).toBeFocused({ timeout: 2000 });

    // Escape closes the modal AND restores focus to the trigger.
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0, { timeout: 2000 });
    await expect(deleteBtn).toBeFocused({ timeout: 2000 });

    // Re-open and exercise the focus trap.
    await deleteBtn.click();
    await expect(dialog).toBeVisible({ timeout: 5000 });
    await expect(cancelBtn).toBeFocused({ timeout: 2000 });

    // Tab → Confirm button.
    await page.keyboard.press("Tab");
    const confirmBtn = page.locator("[data-modal-confirm]");
    await expect(confirmBtn).toBeFocused({ timeout: 2000 });

    // Shift+Tab cycles back to Cancel.
    await page.keyboard.press("Shift+Tab");
    await expect(cancelBtn).toBeFocused({ timeout: 2000 });

    // Scanner-guard inheritance: a USB scanner burst into the page body
    // while the modal is open MUST NOT leak printable chars or activate
    // the focused Cancel button (Enter terminator). Sending a burst to
    // `body` exercises the document-level capture in scanner-guard.js.
    //
    // LOAD-BEARING — Cancel is focused (from the Shift+Tab above), so if
    // scanner-guard were broken the burst's final Enter would activate
    // Cancel and close the modal. The dialog-visible + cancel-focused
    // assertions below would both fail in that case.
    await simulateScan(page, "body", "9782070360246");

    await expect(dialog).toBeVisible({ timeout: 1000 });
    await expect(cancelBtn).toBeFocused({ timeout: 1000 });

    // Click Confirm → submits hx-delete → HX-Redirect to /borrowers.
    await confirmBtn.click();
    await page.waitForURL(/\/borrowers(\?|$)/, { timeout: 5000 });

    // Borrower must no longer appear in the list.
    await expect(
      page
        .locator('a[href^="/borrower/"]')
        .filter({ hasText: new RegExp(`^\\s*${NAME}\\s*$`) }),
    ).toHaveCount(0, { timeout: 5000 });
  });
});
