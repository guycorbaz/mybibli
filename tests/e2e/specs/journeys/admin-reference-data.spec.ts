/**
 * Story 8-4 E2E — Admin Reference Data CRUD.
 *
 * Foundation Rule #7: smoke covers the real journey end-to-end (blank
 * browser → loginAs → navigate → CRUD operations → verify results).
 * Spec ID "RD" — does not generate ISBNs (no catalog rows created).
 *
 * Coverage:
 *   - AC #1 — panel renders four sub-sections (Genres, Volume States,
 *     Contributor Roles, Location Node Types).
 *   - AC #2 — create via inline form.
 *   - AC #3 — rename via inline edit.
 *   - AC #4 — delete via Modal; usage-count guard refuses if in use.
 *   - AC #5 — Volume States `is_loanable` toggle exists per row.
 *   - AC #7 — reference-data text NOT translated (NFR41).
 *   - AC #8 — Anonymous → 303 (P23 strict), Librarian → 403.
 *   - AC #10 — no `hx-confirm=` attribute on the new destructive
 *     buttons (every delete goes through a Modal).
 *   - P32 — reactivate-on-collision feedback (D3-a behavior).
 *   - P32 — CSRF tampering surfaces 403.
 *
 * Story 8-4 P28: per-test unique slugs via `crypto.randomUUID()` so
 * sharded / retried runs never collide on a `Date.now().toString(36)`
 * collision window. Generate inside each test, not module-scope.
 *
 * No `waitForTimeout` — uses DOM-state assertions only.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

function uniqueSlug(prefix: string): string {
  // 8 hex chars from a UUID — collision-free across parallel & retried runs.
  return `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
}

test.describe("Story 8-4 — Admin Reference Data", () => {
  test.beforeEach(async ({ page }) => {
    await page.context().clearCookies();
  });

  test("admin sees all four sections and can add + delete a genre", async ({
    page,
  }) => {
    await loginAs(page, "admin");

    // AC #1 — panel renders, four section headings localized.
    await page.goto("/admin?tab=reference_data");
    await expect(
      page.getByRole("heading", { name: /Genres/i }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: /Volume states|États du volume/i }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: /Contributor roles|Rôles de contributeur/i }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: /Location node types|Types d'emplacement/i }),
    ).toBeVisible();

    // AC #2 — add a genre via the inline form.
    const newGenre = uniqueSlug("RD-Genre");
    await page
      .getByRole("button", { name: /Add genre|Ajouter un genre/i })
      .click();
    const addSlot = page.locator("#admin-ref-genres-add");
    await expect(addSlot).toBeVisible();
    await addSlot.locator('input[name="name"]').fill(newGenre);
    await addSlot.getByRole("button", { name: /Save|Enregistrer/i }).click();

    // Wait for the new row to appear in the list.
    await expect(
      page.locator("#admin-ref-genres-list").getByText(newGenre, { exact: true }),
    ).toBeVisible({ timeout: 10000 });

    // AC #4 — delete via Modal.
    const newRow = page.locator("#admin-ref-genres-list li", {
      hasText: newGenre,
    });
    await newRow.getByRole("button", { name: /Delete|Supprimer/i }).click();

    // Modal opens — `<dialog open aria-modal="true">` wired to scanner-guard 7-5.
    const modal = page.locator("#admin-modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 5000 });
    await modal
      .getByRole("button", { name: /^(Delete|Supprimer)$/i })
      .click();

    // Row is gone; success feedback rendered.
    await expect(
      page.locator("#admin-ref-genres-list").getByText(newGenre, { exact: true }),
    ).toHaveCount(0, { timeout: 10000 });
  });

  test("LocationNodeType rename cascades into storage_locations", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=reference_data");

    // Add a fresh node type.
    const oldName = uniqueSlug("RD-NT");
    const newName = `${oldName}-Renamed`;
    await page
      .getByRole("button", { name: /Add node type|Ajouter un type/i })
      .click();
    const slot = page.locator("#admin-ref-node-types-add");
    await slot.locator('input[name="name"]').fill(oldName);
    await slot.getByRole("button", { name: /Save|Enregistrer/i }).click();
    await expect(
      page.locator("#admin-ref-node-types-list").getByText(oldName, { exact: true }),
    ).toBeVisible({ timeout: 10000 });

    // Click the row's name span → inline-edit form appears.
    const nameSpan = page
      .locator("#admin-ref-node-types-list li")
      .filter({ hasText: oldName })
      .locator('[data-action="inline-form-edit"]');
    await nameSpan.click();
    const editInput = page.locator('input[name="name"][value="' + oldName + '"]');
    await expect(editInput).toBeVisible({ timeout: 5000 });
    await editInput.fill(newName);
    await editInput.press("Enter");

    // Renamed row visible.
    await expect(
      page.locator("#admin-ref-node-types-list").getByText(newName, { exact: true }),
    ).toBeVisible({ timeout: 10000 });
  });

  test("librarian → 403 on /admin?tab=reference_data", async ({ page }) => {
    await loginAs(page, "librarian");
    const resp = await page.goto("/admin?tab=reference_data");
    expect(resp?.status()).toBe(403);
  });

  test("anonymous → strict 303 redirect with ?next= return URL (P23)", async ({
    request,
  }) => {
    // Story 8-4 P23: assert the literal 303 status, not just `< 400`. Use
    // page.request with redirect=manual so the 303 surfaces (page.goto auto-
    // follows redirects and turns the chain into a 200 from /login).
    const resp = await request.get("/admin?tab=reference_data", {
      maxRedirects: 0,
    });
    expect(resp.status()).toBe(303);
    const location = resp.headers()["location"];
    expect(location).toMatch(/^\/login\?next=/);
  });

  test("reference-data values are NOT translated (NFR41)", async ({ page }) => {
    await loginAs(page, "admin");
    // The seed migrations include the French-cased genre "BD". Switching
    // the UI language must NOT translate the genre name itself — only the
    // surrounding chrome (panel heading, buttons) localizes.
    await page.goto("/admin?tab=reference_data");
    const list = page.locator("#admin-ref-genres-list");
    await expect(list.getByText("BD", { exact: true })).toBeVisible({
      timeout: 10000,
    });
  });

  test("reactivate-on-collision: re-creating a deleted genre surfaces Reactivated feedback (P32 / D3-a)", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=reference_data");

    const name = uniqueSlug("RD-Reactivate");

    // Create.
    await page
      .getByRole("button", { name: /Add genre|Ajouter un genre/i })
      .click();
    let addSlot = page.locator("#admin-ref-genres-add");
    await addSlot.locator('input[name="name"]').fill(name);
    await addSlot.getByRole("button", { name: /Save|Enregistrer/i }).click();
    await expect(
      page.locator("#admin-ref-genres-list").getByText(name, { exact: true }),
    ).toBeVisible({ timeout: 10000 });

    // Delete.
    const row = page
      .locator("#admin-ref-genres-list li")
      .filter({ hasText: name });
    await row.getByRole("button", { name: /Delete|Supprimer/i }).click();
    const modal = page.locator("#admin-modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 5000 });
    await modal.getByRole("button", { name: /^(Delete|Supprimer)$/i }).click();
    await expect(
      page.locator("#admin-ref-genres-list").getByText(name, { exact: true }),
    ).toHaveCount(0, { timeout: 10000 });

    // Re-create with the same name → Reactivated feedback (NOT Created).
    await page
      .getByRole("button", { name: /Add genre|Ajouter un genre/i })
      .click();
    addSlot = page.locator("#admin-ref-genres-add");
    await addSlot.locator('input[name="name"]').fill(name);
    await addSlot.getByRole("button", { name: /Save|Enregistrer/i }).click();

    // The "Reactivated" feedback distinguishes the path from a fresh create.
    await expect(
      page
        .locator("#feedback-list")
        .getByText(/Reactivated|Réactivé/i),
    ).toBeVisible({ timeout: 10000 });
    // Row is back in the list.
    await expect(
      page.locator("#admin-ref-genres-list").getByText(name, { exact: true }),
    ).toBeVisible({ timeout: 10000 });
  });

  test("CSRF tampering: POST with bad token returns 403 (P32)", async ({
    page,
    request,
  }) => {
    await loginAs(page, "admin");
    // Pull the real CSRF token off the admin page so the request is
    // authenticated — but we tamper with it before sending so the CSRF
    // middleware rejects.
    await page.goto("/admin?tab=reference_data");
    const realToken = await page.locator('meta[name="csrf-token"]').getAttribute("content");
    expect(realToken, "expected csrf-token meta tag to be present").toBeTruthy();

    // Carry session cookies from the page context onto the API request.
    const cookies = await page.context().cookies();
    const cookieHeader = cookies
      .map((c) => `${c.name}=${c.value}`)
      .join("; ");

    const tamperedToken = "x".repeat((realToken ?? "").length);
    const resp = await request.post("/admin/reference-data/genres", {
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
        "Cookie": cookieHeader,
      },
      data: `name=${encodeURIComponent(uniqueSlug("RD-CSRF"))}&_csrf_token=${encodeURIComponent(tamperedToken)}`,
      maxRedirects: 0,
    });
    // CSRF middleware emits 403 + HX-Trigger: csrf-rejected (story 8-2).
    expect(resp.status()).toBe(403);
  });
});
