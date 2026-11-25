import { test, expect, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { csrfHeaders } from "../../helpers/csrf";
import { expectHtmxProcessed } from "../../helpers/htmx";

/**
 * Story 19-3 (#541, D6/D7) — one live contributor per name: a rename onto
 * a live namesake and a Trash restore next to one are refused, in words.
 *
 * No rename form exists (D6): the rename refusal posts the route the way a
 * page would (CSRF header from the meta tag) and reads the answer. Every
 * name carries a per-run stamp, so the spec only meets its own rows.
 */

const NAME_TAKEN = /Another contributor already has this name\.|Un autre contributeur porte déjà ce nom\./;

/** Scan `isbn` on /catalog and return the title id. */
async function seedTitle(page: Page, isbn: string): Promise<string> {
  await page.goto("/catalog");
  await page.locator("#scan-field").fill(isbn);
  await page.locator("#scan-field").press("Enter");
  await page.waitForSelector(".feedback-skeleton, .feedback-entry", { timeout: 10000 });
  const res = await page.request.get(`/scan?code=${isbn}`);
  const id = /\/title\/(\d+)/.exec(res.url())?.[1];
  expect(id, res.url()).toBeTruthy();
  return id as string;
}

/** The first role offered by the title's contributor form. */
async function firstRoleId(page: Page, titleId: string): Promise<string> {
  const html = await (
    await page.request.get(`/title/${titleId}/contributor-form`, { headers: { "HX-Request": "true" } })
  ).text();
  const id = /<option value="(\d+)"/.exec(html)?.[1];
  expect(id, html).toBeTruthy();
  return id as string;
}

/** Add `name` to the title; returns the junction id of the new credit. */
async function addContributor(
  page: Page,
  titleId: string,
  roleId: string,
  name: string,
): Promise<string> {
  const res = await page.request.post("/catalog/contributors/add", {
    headers: { ...(await csrfHeaders(page)), "HX-Request": "true" },
    form: { title_id: titleId, contributor_name: name, role_id: roleId },
  });
  expect(res.ok()).toBe(true);
  // The list also holds the credit the metadata fetch may have added: pick
  // the remove button of this name (`name` is regex-safe: letters, digits).
  const text = await res.text();
  const junction = new RegExp(`aria-label="[^"]*${name}[^"]*"[^>]*"junction_id":(\\d+)`).exec(text)?.[1];
  expect(junction, text).toBeTruthy();
  return junction as string;
}

/** Ids of the live contributors whose name contains `name`. */
async function contributorIds(page: Page, name: string): Promise<number[]> {
  const res = await page.request.get(`/catalog/contributors/search?q=${encodeURIComponent(name)}`);
  const rows = (await res.json()) as { id: number; name: string }[];
  return rows.map((r) => r.id);
}

test.describe("Story 19-3: one live contributor per name", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page, "admin");
  });

  test("renaming a contributor to a live contributor's name is refused", async ({ page }) => {
    const stamp = Date.now() % 1000000;
    const titleId = await seedTitle(page, specIsbn("CU", 1));
    const roleId = await firstRoleId(page, titleId);
    const taken = `CU Taken ${stamp}`;
    const other = `CU Other ${stamp}`;
    await addContributor(page, titleId, roleId, taken);
    await addContributor(page, titleId, roleId, other);
    const [otherId] = await contributorIds(page, other);

    // Case differs: the names still compare equal.
    const res = await page.request.post("/catalog/contributors/update", {
      headers: { ...(await csrfHeaders(page)), "HX-Request": "true" },
      form: { id: String(otherId), name: taken.toUpperCase() },
    });
    expect(res.ok()).toBe(true);
    expect(await res.text()).toMatch(NAME_TAKEN);
    expect(await contributorIds(page, other)).toEqual([otherId]);
  });

  test("restoring a contributor whose name is live again is refused, the row stays in the Trash", async ({
    page,
  }) => {
    const stamp = Date.now() % 1000000;
    const titleId = await seedTitle(page, specIsbn("CU", 2));
    const roleId = await firstRoleId(page, titleId);
    const name = `CU Restore ${stamp}`;

    // A contributor, taken off the title and deleted…
    const junction = await addContributor(page, titleId, roleId, name);
    const [oldId] = await contributorIds(page, name);
    const headers = { ...(await csrfHeaders(page)), "HX-Request": "true" };
    expect(
      (
        await page.request.post("/catalog/contributors/remove", {
          headers,
          form: { junction_id: junction, title_id: titleId },
        })
      ).ok(),
    ).toBe(true);
    expect((await page.request.delete(`/catalog/contributors/${oldId}`, { headers })).ok()).toBe(true);
    // …and a new one under the same name.
    await addContributor(page, titleId, roleId, name);
    const live = await contributorIds(page, name);
    expect(live).toHaveLength(1);
    expect(live[0]).not.toBe(oldId);

    await page.goto("/admin?tab=trash", { waitUntil: "domcontentloaded" });
    await page.locator("#filter-entity-type").selectOption("contributors");
    const row = page.locator("tbody tr").filter({ hasText: name });
    const restore = row.getByRole("button", { name: /^(restore|restaurer)$/i });
    await expect(restore).toHaveAttribute("hx-post", /entity_type=contributors/, { timeout: 10000 });
    await expectHtmxProcessed(page.locator("#admin-trash-panel"));
    await restore.click();

    // Its removed credit makes the restore ask first (conflict modal); the
    // Confirm's refusal lands in the modal's error region.
    const modal = page.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 10000 });
    await modal.locator("[data-modal-confirm]").click();
    await expect(modal.locator("[data-modal-error]")).toContainText(
      new RegExp(`${name}.*(another contributor already has this name|un autre contributeur porte déjà ce nom)`, "i"),
      { timeout: 10000 },
    );

    // Still in the Trash; still one live contributor of that name.
    await page.goto("/admin?tab=trash", { waitUntil: "domcontentloaded" });
    await page.locator("#filter-entity-type").selectOption("contributors");
    await expect(
      page.locator("tbody tr").filter({ hasText: name }).getByRole("button", { name: /^(restore|restaurer)$/i }),
    ).toHaveAttribute("hx-post", /entity_type=contributors/, { timeout: 10000 });
    expect(await contributorIds(page, name)).toEqual(live);
  });
});
