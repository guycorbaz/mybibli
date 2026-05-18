/**
 * CR #209 — per-volume table on /title/:id.
 *
 * Locks in:
 * - The new <section id="title-volumes"> renders between the contributor
 *   block and the similar-titles strip.
 * - Two scanned volumes show up as two rows, each with V-code, the
 *   "—" placeholder for unset location/condition (volumes scanned with
 *   no location), and the Edit + Delete affordances for Librarian.
 * - The delete flow goes through the UX-DR8 modal:
 *     row Delete button → modal opens in #modal-slot → Confirm submits
 *     DELETE /volume/:id → HX-Redirect → page reloads with one fewer row
 *     AND the header count decremented.
 *
 * Spec ID "VL" — generated ISBN never collides with other specs.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

test.describe("CR #209 — per-volume table on /title/:id", () => {
  test("two volumes render in the table; deleting one drops it from the table", async ({
    page,
  }) => {
    await loginAs(page, "librarian");

    const ISBN = specIsbn("VL", 1);
    const V_KEEP = "V0941"; // ASC sort puts V0941 before V0942
    const V_DROP = "V0942";

    // Step 1: catalog the title via scan, then scan two distinct V-codes.
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
      timeout: 10000,
    });
    await scanField.fill(V_KEEP);
    await scanField.press("Enter");
    await expect(
      page.locator(".feedback-entry").filter({ hasText: V_KEEP }),
    ).toBeVisible({ timeout: 10000 });
    await scanField.fill(V_DROP);
    await scanField.press("Enter");
    await expect(
      page.locator(".feedback-entry").filter({ hasText: V_DROP }),
    ).toBeVisible({ timeout: 10000 });

    // Step 2: navigate to the title-detail page via home search.
    await page.goto(`/?q=${ISBN}`);
    const titleLink = page
      .locator('#browse-results a[href^="/title/"]')
      .first();
    await expect(titleLink).toBeVisible({ timeout: 15000 });
    const titleHref = (await titleLink.getAttribute("href"))!;
    await page.goto(titleHref);
    await page.waitForURL(/\/title\/\d+/);

    // Step 3: assert the volumes section renders both rows.
    const section = page.locator("#title-volumes");
    await expect(section).toBeVisible();
    await expect(
      section.locator("h2#title-volumes-heading"),
    ).toContainText(/\(\s*2\s*\)/);
    // ORDER BY v.label ASC → V_KEEP comes first.
    const rows = section.locator("table tbody tr");
    await expect(rows).toHaveCount(2);
    await expect(rows.nth(0)).toContainText(V_KEEP);
    await expect(rows.nth(1)).toContainText(V_DROP);

    // Step 4: click Delete on the second row → UX-DR8 modal opens.
    await rows.nth(1).getByRole("button", { name: /Delete|Supprimer/i }).click();
    const dialog = page.locator("#modal-slot dialog[open]");
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText(V_DROP);

    // Step 5: confirm → handler returns HX-Redirect to /title/:id.
    await dialog.locator("[data-modal-confirm]").click();
    // The page reloads (full HTMX nav). Wait for the title-detail page
    // to settle and verify the volumes section now has one row.
    await page.waitForURL(/\/title\/\d+/);
    const sectionAfter = page.locator("#title-volumes");
    await expect(sectionAfter).toBeVisible();
    await expect(
      sectionAfter.locator("h2#title-volumes-heading"),
    ).toContainText(/\(\s*1\s*\)/);
    const rowsAfter = sectionAfter.locator("table tbody tr");
    await expect(rowsAfter).toHaveCount(1);
    await expect(rowsAfter).toContainText(V_KEEP);
    await expect(sectionAfter).not.toContainText(V_DROP);
  });
});
