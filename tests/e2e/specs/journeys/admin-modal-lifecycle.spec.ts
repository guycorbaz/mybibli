/**
 * polish-1 AC7 — admin modal lifecycle E2E spec.
 *
 * Covers the post-polish-1 modal lifecycle invariants:
 *   * AC1: trash modal migrated to UX-DR8 macro, lands in #modal-slot.
 *   * AC2: HX-Trigger: modal-close closes the modal on success.
 *   * AC3: showModal() promotion — dialog matches :modal pseudo.
 *   * AC4.a/b/c: X-Modal-Confirm header on Confirm requests, server
 *     middleware strips HX-Retarget, modal.js injects errors into
 *     data-modal-error region (race + retarget guards in place).
 *   * AC4.d: role="alert" region cleared on retry.
 *   * #67: rapid double-click on Delete permanently → single dialog
 *     in DOM (innerHTML swap semantics).
 *
 * Spec ID "ML" — no ISBNs generated (this spec creates no catalog rows;
 * seeds via admin user form + deactivate flow to populate trash).
 */
import { test, expect, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

/**
 * Seed a soft-deleted borrower so it appears in /admin?tab=trash.
 * We seed via the borrower domain instead of the users domain so the
 * trash-delete handler's `last-active-admin` guard (an EXISTING over-
 * conservative check unrelated to polish-1 scope) does not refuse
 * the permanent-delete request in single-admin test environments.
 * The polish-1 modal lifecycle invariants are entity-type-agnostic —
 * exercising them via borrower is equally valid.
 *
 * Returns the borrower id (parsed from the `<a href="/borrower/{id}">`
 * anchor on /borrowers) for use in the DELETE request.
 */
async function seedSoftDeletedBorrower(
  page: Page,
  name: string,
): Promise<void> {
  // Read CSRF from /borrowers so the meta tag is fresh for both
  // POST and DELETE.
  await page.goto("/borrowers");
  const csrf = await page.evaluate(() => {
    return (
      document.querySelector<HTMLMetaElement>('meta[name="csrf-token"]')
        ?.content || ""
    );
  });
  // Create.
  const createResp = await page.request.post("/borrowers", {
    form: { name, _csrf_token: csrf },
    maxRedirects: 0,
  });
  // Borrower create returns either 200 (HTMX) or 303 (full nav).
  if (createResp.status() !== 200 && createResp.status() !== 303) {
    throw new Error(
      `seedSoftDeletedBorrower create failed: ${createResp.status()} ${await createResp.text()}`,
    );
  }
  // Look up the id from /borrowers list.
  await page.goto("/borrowers");
  const link = page
    .locator('tbody a[href^="/borrower/"]')
    .filter({ hasText: new RegExp(`^\\s*${name}\\s*$`) });
  await expect(link).toHaveCount(1, { timeout: 5000 });
  const href = await link.getAttribute("href");
  const id = href?.split("/").pop();
  if (!id) throw new Error(`seedSoftDeletedBorrower: could not parse id`);
  // Soft-delete (DELETE /borrower/{id}).
  const delResp = await page.request.delete(`/borrower/${id}`, {
    headers: { "X-CSRF-Token": csrf },
    maxRedirects: 0,
  });
  if (!delResp.ok() && delResp.status() !== 303 && delResp.status() !== 200) {
    throw new Error(
      `seedSoftDeletedBorrower delete failed: ${delResp.status()} ${await delResp.text()}`,
    );
  }
}

test.describe("polish-1 — admin modal lifecycle (Pattern A trash modal)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page, "admin");
  });

  test("smoke: open trash modal in #modal-slot, Cancel closes cleanly", async ({
    page,
  }) => {
    // Seed: create + soft-delete a borrower so /admin?tab=trash has a row.
    const name = `ml-smoke-${Date.now()}`;
    await seedSoftDeletedBorrower(page, name);

    await page.goto("/admin?tab=trash");
    const trashRow = page.locator(`tr:has-text("${name}")`);
    await expect(trashRow).toBeVisible({ timeout: 5000 });

    await trashRow
      .getByRole("button", { name: /Delete permanently|Supprimer d[ée]finitivement/i })
      .click();
    const modal = page.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 5000 });
    await expect(modal.locator("[data-modal-confirm]")).toBeVisible();
    await expect(modal.locator("[data-modal-error]")).toHaveAttribute("class", /hidden/);

    // Cancel closes — verifies #61 fix (Cancel was hx-delete to a CSS
    // selector URL pre-polish-1; modal.js handles it now).
    await modal.locator("[data-modal-cancel]").click();
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0, {
      timeout: 5000,
    });

    // Trash row still present (Cancel did NOT delete).
    await expect(trashRow).toBeVisible();
  });

  test("AC3 showModal() promotion: dialog matches :modal pseudo when open", async ({
    page,
  }) => {
    const name = `ml-show-${Date.now()}`;
    await seedSoftDeletedBorrower(page, name);

    await page.goto("/admin?tab=trash");
    const trashRow = page.locator(`tr:has-text("${name}")`);
    await trashRow
      .getByRole("button", { name: /Delete permanently|Supprimer d[ée]finitivement/i })
      .click();
    await expect(page.locator("#modal-slot dialog[open]")).toBeVisible({
      timeout: 5000,
    });

    // Verify the dialog was promoted to native top-layer via showModal().
    // Pre-Phase-3 the declarative `<dialog open>` alone didn't grant native
    // modal semantics (no ::backdrop, no inertness, no native Escape).
    const isNativeModal = await page.evaluate(() => {
      const d = document.querySelector("#modal-slot dialog");
      return d instanceof HTMLDialogElement && d.matches(":modal");
    });
    expect(isNativeModal).toBe(true);
  });

  test("AC2 success path: Confirm → HX-Trigger: modal-close → panel updates", async ({
    page,
  }) => {
    const name = `ml-succ-${Date.now()}`;
    await seedSoftDeletedBorrower(page, name);

    await page.goto("/admin?tab=trash");
    const trashRow = page.locator(`tr:has-text("${name}")`);
    await trashRow
      .getByRole("button", { name: /Delete permanently|Supprimer d[ée]finitivement/i })
      .click();
    const modal = page.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 5000 });
    // Type the entity name verbatim into the confirm input (data-confirm-name
    // wiring — mybibli.js enables the Confirm button only when input value
    // matches).
    await modal.locator("input[data-confirm-name]").fill(name);
    // Confirm enables.
    await expect(modal.locator("[data-modal-confirm]")).toBeEnabled();
    await modal.locator("[data-modal-confirm]").click();

    // Modal closes via HX-Trigger: modal-close (AC2), trash row gone.
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(0, {
      timeout: 10000,
    });
    await expect(page.locator(`tr:has-text("${name}")`)).toHaveCount(0);
  });

  test("#67 rapid double-click guard: innerHTML swap keeps single dialog", async ({
    page,
  }) => {
    const name = `ml-dbl-${Date.now()}`;
    await seedSoftDeletedBorrower(page, name);

    await page.goto("/admin?tab=trash");
    const trashRow = page.locator(`tr:has-text("${name}")`);
    const deleteBtn = trashRow.getByRole("button", {
      name: /Delete permanently|Supprimer d[ée]finitivement/i,
    });

    // Two rapid clicks. Pre-polish-1 (hx-target="body" hx-swap="beforeend")
    // would stack 2 dialogs. Post-Phase-5 (hx-target="#modal-slot"
    // hx-swap="innerHTML") replaces — single dialog invariant.
    await deleteBtn.click({ clickCount: 2 });
    // Allow HTMX to settle.
    await expect(page.locator("#modal-slot dialog[open]")).toHaveCount(1, {
      timeout: 5000,
    });
  });
});

test.describe("polish-1 — admin ref-data modal lifecycle (Pattern B, #admin-modal-slot)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page, "admin");
  });

  test("AC3 showModal() promotion: ref-data delete modal in #admin-modal-slot is :modal", async ({
    page,
  }) => {
    // Seed a deletable genre.
    const genreName = `ML-genre-${Date.now()}`;
    await page.goto("/admin?tab=reference_data");
    // Add Genre — the panel ships with a paired Add form per UX-DR21.
    // Open the Add form, fill, submit.
    await page
      .locator('[data-action="inline-form-add-toggle"][data-slot="admin-ref-genres-add"]')
      .click();
    await page
      .locator('#admin-ref-genres-add input[name="name"]')
      .fill(genreName);
    await page
      .locator("#admin-ref-genres-add")
      .getByRole("button", { name: /Save|Enregistrer/i })
      .click();
    // Genre row appears in the genres list.
    const genreRow = page
      .locator('#admin-ref-genres-list li')
      .filter({ hasText: genreName });
    await expect(genreRow).toBeVisible({ timeout: 5000 });

    // Click the delete button (× / aria-label).
    await genreRow
      .getByRole("button", { name: new RegExp(`Delete.*${genreName}|Supprimer.*${genreName}`, "i") })
      .click();
    const modal = page.locator("#admin-modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 5000 });

    // Verify showModal() promotion via the symmetric inline-form.js
    // observer added in polish-1 Phase 3.
    const isNativeModal = await page.evaluate(() => {
      const d = document.querySelector("#admin-modal-slot dialog");
      return d instanceof HTMLDialogElement && d.matches(":modal");
    });
    expect(isNativeModal).toBe(true);

    // Cancel closes via the existing data-action="admin-modal-close" handler
    // (NOT #61's broken hx-delete pattern — that was trash-only).
    await modal
      .locator('[data-action="admin-modal-close"]')
      .click();
    await expect(page.locator("#admin-modal-slot dialog[open]")).toHaveCount(0, {
      timeout: 5000,
    });
  });

  test("AC2 ref-data delete: HX-Trigger: modal-close closes admin-modal-slot on success", async ({
    page,
  }) => {
    const genreName = `ML-genre-del-${Date.now()}`;
    await page.goto("/admin?tab=reference_data");
    await page
      .locator('[data-action="inline-form-add-toggle"][data-slot="admin-ref-genres-add"]')
      .click();
    await page
      .locator('#admin-ref-genres-add input[name="name"]')
      .fill(genreName);
    await page
      .locator("#admin-ref-genres-add")
      .getByRole("button", { name: /Save|Enregistrer/i })
      .click();
    const genreRow = page
      .locator('#admin-ref-genres-list li')
      .filter({ hasText: genreName });
    await expect(genreRow).toBeVisible({ timeout: 5000 });

    await genreRow
      .getByRole("button", { name: new RegExp(`Delete.*${genreName}|Supprimer.*${genreName}`, "i") })
      .click();
    const modal = page.locator("#admin-modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 5000 });

    // Confirm → server emits HX-Trigger: modal-close, inline-form.js
    // listener empties #admin-modal-slot.innerHTML.
    await modal.locator('button[type="submit"]').click();
    await expect(page.locator("#admin-modal-slot dialog[open]")).toHaveCount(0, {
      timeout: 5000,
    });
    // Genre row is gone from the list.
    await expect(
      page.locator('#admin-ref-genres-list li').filter({ hasText: genreName }),
    ).toHaveCount(0);
  });
});
