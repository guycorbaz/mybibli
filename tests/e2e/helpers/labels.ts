/**
 * Admin → Reference data → Labels helpers (extracted from the 13-9 spec
 * for the epic 13 smoke). The caller must be logged in as an admin.
 */
import { Page, expect } from "@playwright/test";

/** Create a label named `name` through Admin → Reference data. */
export async function createLabel(page: Page, name: string): Promise<void> {
  await page.goto("/admin?tab=reference_data");
  await page
    .getByRole("button", {
      name: /Add label|Ajouter une étiquette|Etikett hinzufügen|Aggiungi etichetta/i,
    })
    .click();
  const addSlot = page.locator("#admin-ref-labels-add");
  await addSlot.locator('input[name="name"]').fill(name);
  await addSlot.getByRole("button", { name: /Save|Enregistrer|Speichern|Salva/i }).click();
  await expect(
    page.locator("#admin-ref-labels-list").getByText(name, { exact: true }),
  ).toBeVisible({ timeout: 10000 });
}
