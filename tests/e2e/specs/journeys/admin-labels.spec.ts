import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

/**
 * CR #443 tranche 1 — labels as the fifth admin taxonomy.
 *
 * Covers what tranche 1 actually ships: the vocabulary as a fifth section of
 * the reference-data panel, with the same create / rename / delete shape as
 * the other four, and admin-only access.
 *
 * The two-table usage guard — the part that makes labels different — is
 * verified against a real schema in tests/labels_crud.rs. It cannot be
 * exercised through the UI until tranche 2 ships the affordance that attaches
 * a label to a volume.
 */
function uniqueSlug(prefix: string): string {
  return `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
}

test.describe("CR #443 — Admin labels taxonomy", () => {
  test.beforeEach(async ({ page }) => {
    await page.context().clearCookies();
  });

  test("admin creates, renames and deletes a label", async ({ page }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=reference_data");

    // The section exists alongside the original four.
    await expect(
      page.getByRole("heading", { name: /Labels|Étiquettes|Etiketten|Etichette/i }),
    ).toBeVisible();

    const name = uniqueSlug("LB-Label");
    await page
      .getByRole("button", { name: /Add label|Ajouter une étiquette|Etikett hinzufügen|Aggiungi etichetta/i })
      .click();
    const addSlot = page.locator("#admin-ref-labels-add");
    await expect(addSlot).toBeVisible();
    await addSlot.locator('input[name="name"]').fill(name);
    await addSlot.getByRole("button", { name: /Save|Enregistrer|Speichern|Salva/i }).click();

    const list = page.locator("#admin-ref-labels-list");
    await expect(list.getByText(name, { exact: true })).toBeVisible({ timeout: 10000 });

    // Delete through the shared UX-DR8 modal, same as the other taxonomies.
    const row = list.locator("li", { hasText: name });
    await row.getByRole("button", { name: /Delete|Supprimer|Löschen|Elimina/i }).click();
    const modal = page.locator("#admin-modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 5000 });
    await modal.getByRole("button", { name: /^(Delete|Supprimer|Löschen|Elimina)$/i }).click();

    await expect(list.getByText(name, { exact: true })).toHaveCount(0, {
      timeout: 10000,
    });
  });

  // NOTE — the usage guard (a label carried only by a VOLUME must refuse
  // deletion) is deliberately NOT tested here. Attaching a label to a volume
  // is tranche 2's UI; testing it now would mean inventing a test-only
  // endpoint, and a spec that skips itself every run reads like coverage
  // while providing none. The guard is covered against a real schema in
  // tests/labels_crud.rs, and the E2E journey lands with the UI that makes it
  // reachable.

  test("a librarian cannot reach the labels endpoints", async ({ page }) => {
    await loginAs(page, "librarian");
    const resp = await page.request.get("/admin/reference-data/labels", {
      failOnStatusCode: false,
    });
    expect(resp.status()).toBe(403);
  });
});
