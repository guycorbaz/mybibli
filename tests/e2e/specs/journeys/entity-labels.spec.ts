import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";

/**
 * CR #443 tranche 2 — applying labels to titles and volumes.
 *
 * The journey the requester described: create the vocabulary once as admin,
 * then flag items with it while cataloguing. What this spec pins beyond the
 * happy path is requirement 6 — an anonymous visitor must never see labels,
 * "not on cards, not on detail pages, not through any URL, not in counts".
 */
function uniqueSlug(prefix: string): string {
  return `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
}

/** Create a label through the admin panel and return its name. */
async function createLabel(page: import("@playwright/test").Page): Promise<string> {
  const name = uniqueSlug("EL-Label");
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

/** Scan an ISBN and land on the resulting title page. */
async function openTitlePage(page: import("@playwright/test").Page, isbn: string) {
  await page.goto("/catalog");
  await page.locator("#scan-field").fill(isbn);
  await page.locator("#scan-field").press("Enter");
  await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });

  await page.goto(`/?q=${isbn}`);
  await page
    .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
    .first()
    .click();
  await expect(page.locator("h1")).toBeVisible({ timeout: 5000 });
}

test.describe("CR #443 — labels on titles and volumes", () => {
  test("librarian attaches a label to a title, then removes it", async ({ page }) => {
    await loginAs(page, "admin");
    const label = await createLabel(page);

    await openTitlePage(page, specIsbn("EL", 1));

    const region = page.locator("#entity-labels");
    await expect(region).toBeVisible();
    // Nothing attached yet.
    await expect(region.locator(`[data-label-chip="${label}"]`)).toHaveCount(0);

    await region.locator("#entity-label-select").selectOption({ label });
    await region.getByRole("button", { name: /^(Add|Ajouter|Hinzufügen|Aggiungi)$/i }).click();

    // The region swaps itself out; the chip carries the label name.
    await expect(page.locator(`#entity-labels [data-label-chip="${label}"]`)).toBeVisible({
      timeout: 10000,
    });

    // Removing it takes the chip away again.
    await page
      .locator("#entity-labels")
      .getByRole("button", { name: new RegExp(`(Remove|Retirer|entfernen|Rimuovi).*${label}`, "i") })
      .click();
    await expect(page.locator(`#entity-labels [data-label-chip="${label}"]`)).toHaveCount(0, {
      timeout: 10000,
    });
  });

  test("the same vocabulary applies to a volume", async ({ page }) => {
    await loginAs(page, "admin");
    const label = await createLabel(page);

    // Catalogue a title and a volume through the canonical helper rather
    // than re-rolling the scan flow: it also handles the #300 phantom-volume
    // confirmation modal, which opens as soon as the title already has a
    // volume — the case my hand-rolled version silently hung on under
    // parallel load.
    const isbn = specIsbn("EL", 2);
    const vcode = `V9${crypto.randomUUID().replace(/\D/g, "").slice(0, 3).padEnd(3, "0")}`;
    await scanTitleAndVolume(page, isbn, vcode);

    await page.goto(`/?q=${isbn}`);
    await page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first()
      .click();
    await page.locator(`a[href^="/volume/"]`).first().click();
    await expect(page).toHaveURL(/\/volume\/\d+/);

    const region = page.locator("#entity-labels");
    await expect(region).toBeVisible();
    await region.locator("#entity-label-select").selectOption({ label });
    await region.getByRole("button", { name: /^(Add|Ajouter|Hinzufügen|Aggiungi)$/i }).click();
    await expect(page.locator(`#entity-labels [data-label-chip="${label}"]`)).toBeVisible({
      timeout: 10000,
    });
  });

  test("an anonymous visitor sees no labels and cannot attach one", async ({ page, browser }) => {
    // Seed a labelled title as admin first.
    await loginAs(page, "admin");
    const label = await createLabel(page);
    await openTitlePage(page, specIsbn("EL", 3));
    const region = page.locator("#entity-labels");
    await region.locator("#entity-label-select").selectOption({ label });
    await region.getByRole("button", { name: /^(Add|Ajouter|Hinzufügen|Aggiungi)$/i }).click();
    await expect(page.locator(`#entity-labels [data-label-chip="${label}"]`)).toBeVisible({
      timeout: 10000,
    });
    const titleUrl = page.url();

    // Requirement 6: a fresh anonymous context must see none of it.
    const anon = await browser.newContext();
    const anonPage = await anon.newPage();
    await anonPage.goto(titleUrl);
    await expect(anonPage.locator("h1")).toBeVisible({ timeout: 5000 });

    await expect(anonPage.locator("#entity-labels")).toHaveCount(0);
    await expect(anonPage.locator("body")).not.toContainText(label);

    // And the endpoints refuse them outright, not merely hide the affordance.
    // `maxRedirects: 0` is load-bearing: an unauthenticated mutation answers
    // 303 to /login (see AppError::Unauthorized), and Playwright follows
    // redirects by default — which would report the login page's 200 and make
    // a working refusal look like a breach.
    const attach = await anonPage.request.post(
      `${titleUrl.replace(/\?.*$/, "")}/labels/attach`,
      {
        form: { label_id: "1", _csrf_token: "x" },
        failOnStatusCode: false,
        maxRedirects: 0,
      },
    );
    expect([303, 401, 403]).toContain(attach.status());

    // And nothing was written: re-reading as admin still shows one chip.
    await page.reload();
    await expect(page.locator(`#entity-labels [data-label-chip="${label}"]`)).toHaveCount(1);

    await anon.close();
  });
});
