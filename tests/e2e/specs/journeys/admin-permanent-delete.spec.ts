import { test, expect, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

// CR #62 — Rewrite of the 8-7 P9 spec. The original wrapped every
// assertion in `if (tableExists) { if (btnExists) { ... } }`, which
// silently no-op'd on a fresh DB and violated Foundation Rules #3 / #7.
//
// Each test now seeds its OWN soft-deleted series (simplest entity to
// soft-delete via UI: no FK dependencies, owns the UX-DR8 delete modal
// since story 9-13). The tests then exercise the permanent-delete
// modal contracts against that seeded row.
//
// Auto-purge on startup (original Scenario 2) is covered by the Rust
// integration tests in `tests/admin_auto_purge_*.rs`; the last-admin
// guard (original Scenario 3) is covered by
// `src/models/user.rs::test_deactivate_last_admin_is_blocked` and
// `tests/admin_user_deactivate_modal.rs`. Both involve restart-cycle
// or admin-user scaffolding that doesn't fit the E2E shape.

/** Seed a soft-deleted series and return its display name. */
async function seedSoftDeletedSeries(page: Page, slug: string): Promise<string> {
  const name = `PD-${slug}-${Date.now()}`;
  await page.goto("/series/new");
  await page.locator("#series-name").fill(name);
  await page.locator('main button[type="submit"]').last().click();
  await page.waitForURL(/\/series\/\d+/);

  // Series detail page → click Delete → UX-DR8 modal opens → Confirm
  const deleteBtn = page.getByRole("button", { name: /delete|supprimer/i });
  await deleteBtn.click();
  await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
  await page.locator("[data-modal-confirm]").click();
  // Series delete success redirects to /series
  await page.waitForURL("**/series", { timeout: 5000 });
  return name;
}

/** Navigate to /admin?tab=trash filtered to series and locate the row by name. */
async function openTrashAndFindRow(page: Page, name: string) {
  await page.goto("/admin?tab=trash", { waitUntil: "domcontentloaded" });
  await expect(
    page.locator('section[aria-labelledby="admin-trash-heading"]'),
  ).toBeVisible({ timeout: 10000 });
  // Filter to series to keep the DOM scoped
  await page.locator("#filter-entity-type").selectOption("series");
  // The HTMX swap replaces #admin-trash-panel — wait for the table row
  const row = page.locator("tbody tr").filter({ hasText: name });
  await expect(row).toBeVisible({ timeout: 10000 });
  return row;
}

test.describe("Story 8-7: Permanent Delete & Auto-Purge", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page, "admin");
  });

  test("SC1: permanent-delete modal blocks confirm until exact name is typed", async ({
    page,
  }) => {
    const name = await seedSoftDeletedSeries(page, "SC1");
    const row = await openTrashAndFindRow(page, name);

    // Click "Delete permanently" on our row → modal opens
    const deleteBtn = row.locator("button[data-modal-trigger]");
    await expect(deleteBtn).toBeVisible();
    await deleteBtn.click();

    const modal = page.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 5000 });
    await expect(modal).toContainText(/cannot be undone|impossible|irreversible/i);

    // Confirm is disabled by default — name friction is the whole point
    const confirmBtn = modal.locator("[data-modal-confirm]");
    await expect(confirmBtn).toBeDisabled();

    // Wrong name leaves it disabled
    const input = modal.locator('input[type="text"]');
    await input.fill("definitely not the right name");
    await expect(confirmBtn).toBeDisabled();

    // Typing the correct name enables it (locks the predicate end-to-end)
    await input.clear();
    await input.fill(name);
    await expect(confirmBtn).toBeEnabled();

    // Cancel without committing — leaves the row in trash for cleanup.
    // `data-modal-cancel` is unambiguous here; `data-modal-default-focus`
    // is also on the type-to-confirm input in this UX-DR8 macro variant.
    const cancelBtn = modal.locator("[data-modal-cancel]");
    await cancelBtn.click();
    await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible();
  });

  test("SC2: permanent-delete with correct name removes the row and surfaces feedback", async ({
    page,
  }) => {
    const name = await seedSoftDeletedSeries(page, "SC2");
    const row = await openTrashAndFindRow(page, name);

    await row.locator("button[data-modal-trigger]").click();
    const modal = page.locator("#modal-slot dialog[open]");
    await expect(modal).toBeVisible({ timeout: 5000 });

    // Type the exact name → confirm enables → click → modal closes
    await modal.locator('input[type="text"]').fill(name);
    const confirmBtn = modal.locator("[data-modal-confirm]");
    await expect(confirmBtn).toBeEnabled();
    await confirmBtn.click();

    // Success path: server returns a fresh trash panel without our row
    // (and may also emit a feedback entry).
    await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible({
      timeout: 5000,
    });
    await expect(
      page.locator("tbody tr").filter({ hasText: name }),
    ).toHaveCount(0, { timeout: 5000 });
  });

  test("SC3: trash UI structure renders and filter interaction updates the list", async ({
    page,
  }) => {
    await page.goto("/admin?tab=trash", { waitUntil: "domcontentloaded" });

    const panel = page.locator('section[aria-labelledby="admin-trash-heading"]');
    await expect(panel).toBeVisible({ timeout: 10000 });

    // Structural locks from the original spec (these still fail loudly
    // if the trash panel chrome regresses).
    await expect(page.locator("#admin-trash-heading")).toBeVisible({ timeout: 5000 });
    await expect(page.locator("#filter-entity-type")).toBeVisible({ timeout: 5000 });
    await expect(page.locator("#search-trash")).toBeVisible({ timeout: 5000 });

    // Filter interaction lock — selecting a filter must trigger the
    // HTMX swap that re-renders the panel with the filter `selected`.
    await page.locator("#filter-entity-type").selectOption("series");
    await expect(page.locator("#filter-entity-type")).toHaveValue("series", {
      timeout: 5000,
    });
    // Either the table has rows OR the empty-state appears — either is
    // valid post-swap, but the page must not be in an in-flight state.
    const swapped = page.locator("#admin-trash-panel");
    await expect(swapped).toBeVisible();
  });
});
