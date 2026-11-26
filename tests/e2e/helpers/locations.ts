import { Page, expect } from "@playwright/test";
import { specLcode } from "./isbn";

/**
 * Create a root location with a specific name and L-code.
 * Uses the /locations form directly. Returns the L-code.
 */
export async function createLocation(
  page: Page,
  name: string,
  lcode: string,
): Promise<string> {
  await page.goto("/locations");
  await page.locator("summary").filter({ hasText: /add root|ajouter/i }).click();
  await page.locator("#new-name").fill(name);
  // Override the auto-proposed L-code with our unique one
  await page.locator("#new-lcode").fill(lcode);
  // Scope to the root-form submit button — story 7-3 added a language toggle
  // in the nav bar (4 submit buttons: FR/EN desktop + FR/EN mobile) and each
  // tree node also renders a hidden `#add-child-{id}` form, so `.last()` is
  // fragile.
  await page.locator("#add-root-submit").click();
  await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });
  await expect(page.locator(`text=${name}`)).toBeVisible({ timeout: 5000 });
  return lcode;
}

/**
 * Create a child location under an existing one, through the parent's
 * inline "add child" form on /locations (the form issue #185 fixed).
 * Returns the L-code.
 */
export async function createChildLocation(
  page: Page,
  parentName: string,
  name: string,
  lcode: string,
): Promise<string> {
  await page.goto("/locations");
  // Exact accessible name ("Edit {name}" / "Modifier {name}", rendered by
  // routes/locations.rs) so a parent whose name is a prefix of another never
  // matches it. Either language: another spec may have changed the user's.
  const escaped = parentName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const parentEditLink = page.getByRole("link", {
    name: new RegExp(`^(Edit|Modifier) ${escaped}$`),
  });
  await expect(parentEditLink).toBeVisible({ timeout: 5000 });
  const parentId = (await parentEditLink.getAttribute("href"))?.match(
    /\/locations\/(\d+)/,
  )?.[1];
  expect(parentId, `edit link of ${parentName} carries its id`).toBeTruthy();
  await page.locator(`[data-locations-toggle="add-child-${parentId}"]`).click();
  const childForm = page.locator(`#add-child-${parentId}`);
  await expect(childForm).toBeVisible();
  await childForm.locator('input[name="name"]').fill(name);
  await childForm.locator('input[name="label"]').fill(lcode);
  await childForm.locator('button[type="submit"]').click();
  await expect(page).toHaveURL(/\/locations\/?$/, { timeout: 5000 });
  await expect(page.locator(`text=${name}`)).toBeVisible({ timeout: 5000 });
  return lcode;
}

/**
 * Two consecutive L-codes of `bucket` (`specLcode(bucket, seq)` and
 * `seq + 1`) that /locations does not list yet, the first one an even
 * `seq` drawn in [minSeq, maxSeq). A re-run on a non-reset stack never
 * meets an earlier run's locations. Gives up after 50 draws.
 */
export async function freeLcodePair(
  page: Page,
  bucket: number,
  minSeq: number,
  maxSeq: number,
): Promise<[string, string]> {
  await page.goto("/locations");
  const listed = await page.content();
  const starts = Math.floor((maxSeq - minSeq) / 2);
  for (let attempt = 0; attempt < 50; attempt++) {
    const seq = minSeq + Math.floor(Math.random() * starts) * 2;
    const pair: [string, string] = [specLcode(bucket, seq), specLcode(bucket, seq + 1)];
    if (!listed.includes(pair[0]) && !listed.includes(pair[1])) return pair;
  }
  throw new Error(`no free L-code pair left in bucket ${bucket}`);
}
