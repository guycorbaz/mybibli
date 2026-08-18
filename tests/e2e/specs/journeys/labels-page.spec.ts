import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";

/**
 * CR #443 tranche 3 — the /labels index and its drill-down.
 *
 * The navigation chain the requester asked for: labels list → members of one
 * label → the item's detail page. What this spec pins beyond that chain is the
 * two decisions the issue settled on 2026-07-28:
 *   - members render as TWO sections (titles above volumes), not one merged
 *     table;
 *   - an empty section shows its empty state rather than disappearing, so
 *     "no volumes carry this label" stays distinguishable from "the section
 *     failed to load".
 */
function uniqueSlug(prefix: string): string {
  return `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
}

async function createLabel(page: import("@playwright/test").Page): Promise<string> {
  const name = uniqueSlug("LP-Label");
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
  return name;
}

test.describe("CR #443 — labels page and drill-down", () => {
  test("a fresh label shows both empty sections, not a missing one", async ({ page }) => {
    await loginAs(page, "admin");
    const label = await createLabel(page);

    await page.goto("/labels");
    await page.getByRole("link", { name: label, exact: true }).click();
    await expect(page).toHaveURL(/\/labels\/\d+/);

    // Both sections are present, each stating its own emptiness. A section
    // that simply vanished would read as a load failure.
    await expect(page.locator("#label-titles-empty")).toBeVisible();
    await expect(page.locator("#label-volumes-empty")).toBeVisible();
    await expect(page.locator("#label-titles-table")).toHaveCount(0);
    await expect(page.locator("#label-volumes-table")).toHaveCount(0);
  });

  test("labelled title and volume appear in their own sections, and link through", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    const label = await createLabel(page);

    const isbn = specIsbn("LP", 1);
    const vcode = `V8${crypto.randomUUID().replace(/\D/g, "").slice(0, 3).padEnd(3, "0")}`;
    await scanTitleAndVolume(page, isbn, vcode);

    // Attach on the title page.
    await page.goto(`/?q=${isbn}`);
    await page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first()
      .click();
    const titleUrl = page.url();
    let region = page.locator("#entity-labels");
    await region.locator("#entity-label-select").selectOption({ label });
    await region.getByRole("button", { name: /^(Add|Ajouter|Hinzufügen|Aggiungi)$/i }).click();
    await expect(page.locator(`#entity-labels [data-label-chip="${label}"]`)).toBeVisible({
      timeout: 10000,
    });

    // And on the volume page.
    await page.goto(titleUrl);
    await page.locator('a[href^="/volume/"]').first().click();
    await expect(page).toHaveURL(/\/volume\/\d+/);
    region = page.locator("#entity-labels");
    await region.locator("#entity-label-select").selectOption({ label });
    await region.getByRole("button", { name: /^(Add|Ajouter|Hinzufügen|Aggiungi)$/i }).click();
    await expect(page.locator(`#entity-labels [data-label-chip="${label}"]`)).toBeVisible({
      timeout: 10000,
    });

    // The drill-down now shows one member in each section — the point of a
    // shared vocabulary spanning two entity kinds.
    await page.goto("/labels");
    await page.getByRole("link", { name: label, exact: true }).click();

    await expect(page.locator("#label-titles-table")).toBeVisible();
    await expect(page.locator("#label-volumes-table")).toBeVisible();
    await expect(page.locator(`#label-volumes-table a:has-text("${vcode}")`)).toBeVisible();

    // Requirement 5: clicking an entry reaches its detail page.
    await page.locator("#label-titles-table a[href^='/title/']").first().click();
    await expect(page).toHaveURL(/\/title\/\d+/);
  });

  test("an anonymous visitor cannot reach the labels page or its nav entry", async ({
    page,
  }) => {
    await page.goto("/");
    // The nav entry lives inside the Librarian+ block.
    await expect(page.locator('a[href="/labels"]')).toHaveCount(0);

    const resp = await page.request.get("/labels", {
      failOnStatusCode: false,
      maxRedirects: 0,
    });
    expect([303, 401, 403]).toContain(resp.status());
  });
});
