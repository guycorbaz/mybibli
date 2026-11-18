import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { simulateScan } from "../../helpers/scanner";

test.describe("Location Hierarchy CRUD (Story 2-1)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // AC6: Tree display + empty state
  test("locations page loads with title", async ({ page }) => {
    await page.goto("/locations");
    await expect(page.locator("h1")).toBeVisible();
  });

  // AC8: L-code auto-proposed
  test("L-code is auto-proposed in create form", async ({ page }) => {
    await page.goto("/locations");

    await page.locator("summary").filter({ hasText: /add root/i }).click();

    const lcodeInput = page.locator("#new-lcode");
    const value = await lcodeInput.inputValue();
    expect(value).toMatch(/^L\d{4}$/);
  });

  // AC1: Create root location
  test("create root location → appears in tree", async ({ page }) => {
    await page.goto("/locations");

    await page.locator("summary").filter({ hasText: /add root/i }).click();
    await page.locator("#new-name").fill("LO-TestMaison");
    await page.locator("#new-lcode").fill("L5001");
    await page.locator("#add-root-submit").click();

    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });
    await expect(page.locator("text=LO-TestMaison")).toBeVisible();
  });

  // AC1: Create child location (nested under parent)
  test("create child location → appears nested", async ({ page }) => {
    await page.goto("/locations");

    // Create parent first
    await page.locator("summary").filter({ hasText: /add root/i }).click();
    await page.locator("#new-name").fill("LO-ParentLoc");
    await page.locator("#new-lcode").fill("L5002");
    await page.locator("#add-root-submit").click();
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });
    await expect(page.locator("text=LO-ParentLoc")).toBeVisible();

    // Get parent's ID from its edit link
    const editLink = page.locator('a[aria-label*="LO-ParentLoc"][href*="/edit"]').first();
    await expect(editLink).toBeVisible({ timeout: 3000 });
    const href = await editLink.getAttribute("href");
    const parentId = href?.match(/\/locations\/(\d+)/)?.[1];
    expect(parentId).toBeTruthy();

    // Create child as root first
    await page.locator("summary").filter({ hasText: /add root/i }).click();
    await page.locator("#new-name").fill("LO-ChildLoc");
    await page.locator("#new-lcode").fill("L5003");
    await page.locator("#add-root-submit").click();
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });
    await expect(page.locator("text=LO-ChildLoc")).toBeVisible();

    // Edit child to set parent
    const childEditLink = page.locator('a[aria-label*="LO-ChildLoc"][href*="/edit"]').first();
    await expect(childEditLink).toBeVisible({ timeout: 3000 });
    await childEditLink.click();
    await expect(page).toHaveURL(/\/locations\/\d+\/edit/);

    const parentSelect = page.locator("#edit-parent");
    await parentSelect.selectOption(parentId!);
    await page.locator("#edit-location-submit").click();
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });

    // Both parent and child should be visible in the tree
    await expect(page.locator("text=LO-ParentLoc")).toBeVisible();
    await expect(page.locator("text=LO-ChildLoc")).toBeVisible();
  });

  // AC2: Edit location name
  test("edit location name → redirects back to tree", async ({ page }) => {
    await page.goto("/locations");

    // Create a location first
    await page.locator("summary").filter({ hasText: /add root/i }).click();
    await page.locator("#new-name").fill("LO-EditTest");
    await page.locator("#new-lcode").fill("L5004");
    await page.locator("#add-root-submit").click();
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });

    // Click edit on the specific location
    const editLink = page.locator('a[aria-label*="LO-EditTest"][href*="/edit"]').first();
    await expect(editLink).toBeVisible({ timeout: 5000 });
    await editLink.click();
    await expect(page).toHaveURL(/\/locations\/\d+\/edit/);

    // Change name and submit. Fix #296 — the empty-`parent_id=` form
    // body now deserializes cleanly to `None`, so the previous
    // workaround that stripped the `name="parent_id"` attribute
    // client-side is gone. If this test starts 422-ing again, the
    // `deserialize_optional_u64` wiring on `UpdateLocationForm` got
    // dropped.
    const nameInput = page.locator("#edit-name");
    await nameInput.clear();
    await nameInput.fill("LO-EditedName");

    await page.locator("#edit-location-submit").click();

    // Should redirect back to locations
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });
    await expect(page.locator("text=LO-EditedName")).toBeVisible();
  });

  // Fix #296 — production-reproducer. Ticking "Emplacement
  // organisationnel" (CR #280) on a ROOT location used to fail with
  // `Failed to deserialize form body: parent_id: cannot parse
  // integer from empty string` because the `<select name="parent_id">
  // <option value="">None</option></select>` submits an empty string
  // for root locations. The form struct now uses
  // `deserialize_optional_u64`, so this round-trips cleanly.
  test("Fix #296 — toggle organisationnel on a root location", async ({
    page,
  }) => {
    await page.goto("/locations");

    // Create a root location.
    await page.locator("summary").filter({ hasText: /add root/i }).click();
    await page.locator("#new-name").fill("LO-OrgRoot");
    await page.locator("#new-lcode").fill("L5099");
    await page.locator("#add-root-submit").click();
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });

    // Open its edit form.
    const editLink = page
      .locator('a[aria-label*="LO-OrgRoot"][href*="/edit"]')
      .first();
    await expect(editLink).toBeVisible({ timeout: 5000 });
    await editLink.click();
    await expect(page).toHaveURL(/\/locations\/\d+\/edit/);

    // Sanity: parent is None — this is the empty-string-on-submit path.
    const parentSelect = page.locator("#edit-parent");
    await expect(parentSelect).toBeVisible();
    expect(await parentSelect.inputValue()).toBe("");

    // Tick the organisationnel checkbox + submit.
    await page.locator("#edit-organizational").check();
    await page.locator("#edit-location-submit").click();

    // Must redirect to /locations (NOT 400 the deserialize error
    // body). Pre-fix this was 400 with the bare error string.
    await expect(page).toHaveURL(/\/locations/, { timeout: 5000 });
    await expect(page.locator("text=LO-OrgRoot")).toBeVisible();
  });

  // AC3: Delete empty location — #698: through the UX-DR8 modal (the native
  // `hx-confirm` was English and not watched by scanner-guard).
  test("delete empty location through the modal → removed from tree", async ({ page }) => {
    await page.goto("/locations");

    await page.locator("summary").filter({ hasText: /add root|ajouter/i }).click();
    await page.locator("#new-name").fill("LO-ToDelete");
    await page.locator("#new-lcode").fill("L5005");
    await page.locator("#add-root-submit").click();
    const row = page.locator('[role="treeitem"]').filter({ hasText: "LO-ToDelete" }).first();
    await expect(row).toBeVisible({ timeout: 5000 });

    const deleteBtn = row.locator("button[data-modal-trigger]");
    await expect(deleteBtn).toHaveAttribute("aria-label", /Delete LO-ToDelete|Supprimer LO-ToDelete/);
    await deleteBtn.click();

    const dialog = page.locator("#modal-slot dialog[open]");
    await expect(dialog).toBeVisible({ timeout: 5000 });
    await expect(dialog).toContainText(/LO-ToDelete \(L5005\)/);
    await expect(page.locator("[data-modal-default-focus]")).toBeFocused({ timeout: 2000 });

    // #698 — a scanner burst ending in Enter while Confirm holds focus must
    // not confirm the deletion (scanner-guard watches the modal).
    await page.keyboard.press("Tab");
    const confirmBtn = page.locator("[data-modal-confirm]");
    await expect(confirmBtn).toBeFocused({ timeout: 2000 });
    const deletes: string[] = [];
    page.on("request", (r) => {
      if (r.method() === "DELETE" && r.url().includes("/locations/")) deletes.push(r.url());
    });
    await simulateScan(page, "body", "9782070360246");
    await expect(dialog).toBeVisible({ timeout: 1000 });
    await expect(confirmBtn).toBeFocused();
    expect(deletes, "the scanner's Enter must not send the delete").toHaveLength(0);

    await confirmBtn.click();
    await expect(dialog).toHaveCount(0, { timeout: 5000 });
    expect(deletes).toHaveLength(1);
    await expect(page.locator('[role="treeitem"]').filter({ hasText: "LO-ToDelete" })).toHaveCount(0, {
      timeout: 5000,
    });
    await expect(page.locator("#feedback-list .feedback-entry").first()).toContainText(
      /Location deleted|Emplacement supprimé/,
    );
    // Rule #21 — the trigger left with the row: focus goes to the
    // "add root location" summary (a root row's fallback), not <body>.
    await expect(page.locator("#add-root-summary")).toBeFocused();
  });

  // #698 — a refusal (child locations) shows inside the open modal, in the
  // request language, and nothing is deleted.
  test("deleting a location with children is refused inside the modal", async ({ page }) => {
    await page.goto("/locations");
    await page.locator("summary").filter({ hasText: /add root|ajouter/i }).click();
    await page.locator("#new-name").fill("LO-698-Parent");
    await page.locator("#new-lcode").fill("L5698");
    await page.locator("#add-root-submit").click();
    const parent = page.locator('[role="treeitem"]').filter({ hasText: "LO-698-Parent" }).first();
    await expect(parent).toBeVisible({ timeout: 5000 });

    await parent.locator("[data-locations-toggle]").first().click();
    const childForm = parent.locator('form[id^="add-child-"]').first();
    await childForm.locator('input[name="name"]').fill("LO-698-Child");
    await childForm.locator('input[name="label"]').fill("L5699");
    await childForm.locator('button[type="submit"]').click();
    await expect(page.locator('[role="treeitem"]').filter({ hasText: "LO-698-Child" }).first()).toBeVisible({
      timeout: 5000,
    });

    const parentRow = page.locator('[role="treeitem"] > div').filter({ hasText: "LO-698-Parent" }).first();
    await parentRow.locator("button[data-modal-trigger]").click();
    const dialog = page.locator("#modal-slot dialog[open]");
    await expect(dialog).toBeVisible({ timeout: 5000 });
    await page.locator("[data-modal-confirm]").click();

    await expect(dialog.locator("[data-modal-error]")).toContainText(
      /has child locations|contient des emplacements enfants/,
      { timeout: 5000 },
    );
    await expect(dialog).toBeVisible();
    // Shown once, in the modal — no generic "Server error" line behind it.
    await expect(page.locator("#feedback-list")).not.toContainText(/Server error|Erreur serveur/i);
    await page.keyboard.press("Escape");
    await expect(parent).toBeVisible();

    // Deleting the child: focus falls back to the parent row's link (Rule #21).
    const childRow = page.locator('[role="treeitem"] > div').filter({ hasText: "LO-698-Child" }).first();
    await childRow.locator("button[data-modal-trigger]").click();
    await expect(dialog).toBeVisible({ timeout: 5000 });
    await page.locator("[data-modal-confirm]").click();
    await expect(dialog).toHaveCount(0, { timeout: 5000 });
    await expect(page.locator('[role="treeitem"]').filter({ hasText: "LO-698-Child" })).toHaveCount(0);
    await expect(parentRow.locator('a[href^="/location/"]')).toBeFocused();
  });

  // #698 — a Librarian adds and edits, but only an Admin may delete: no dead
  // delete control for a Librarian.
  test("a librarian sees add and edit controls but no delete", async ({ page, context }) => {
    await page.goto("/locations");
    await page.locator("summary").filter({ hasText: /add root|ajouter/i }).click();
    await page.locator("#new-name").fill("LO-698-Librarian");
    await page.locator("#new-lcode").fill("L5700");
    await page.locator("#add-root-submit").click();
    await expect(page.locator("text=LO-698-Librarian")).toBeVisible({ timeout: 5000 });

    await context.clearCookies();
    await loginAs(page, "librarian");
    await page.goto("/locations");
    const row = page.locator('[role="treeitem"] > div').filter({ hasText: "LO-698-Librarian" }).first();
    await expect(row.locator('a[href$="/edit"]')).toHaveCount(1);
    await expect(row.locator("[data-locations-toggle]")).toHaveCount(1);
    await expect(row.locator("button[data-modal-trigger]")).toHaveCount(0);
  });

  // AC9: Node type dropdown has options
  test("node type dropdown shows configured types", async ({ page }) => {
    await page.goto("/locations");

    await page.locator("summary").filter({ hasText: /add root/i }).click();

    const typeSelect = page.locator("#new-type");
    const options = await typeSelect.locator("option").allTextContents();

    // Should have at least the 4 seeded types
    expect(options.length).toBeGreaterThanOrEqual(4);
    expect(options).toContain("Room");
    expect(options).toContain("Furniture");
    expect(options).toContain("Shelf");
    expect(options).toContain("Box");
  });

  // #695 regression — the row's add / edit / delete controls are visible at
  // rest (dimmed), at full opacity on hover and on keyboard focus. Computed
  // opacity, because Playwright's toBeVisible() treats opacity 0 as visible —
  // and the E2E stack is the Docker image, where the bug lived.
  test("tree row controls are visible at rest, full on hover and focus", async ({ page }) => {
    await page.goto("/locations");
    await page.locator("summary").filter({ hasText: /add root|ajouter/i }).click();
    await page.locator("#new-name").fill("LO-Controls695");
    await page.locator("#new-lcode").fill("L5695");
    await page.locator("#add-root-submit").click();

    const row = page
      .locator('[role="treeitem"] > div')
      .filter({ hasText: "LO-Controls695" })
      .first();
    const editLink = row.locator('a[aria-label="Edit LO-Controls695"]');
    // Wait for the created row (the URL is /locations before and after).
    await expect(editLink).toHaveCount(1, { timeout: 5000 });
    const controls = editLink.locator("xpath=..");
    const opacity = () => controls.evaluate((el) => Number(getComputedStyle(el).opacity));

    await page.mouse.move(0, 0);
    await expect.poll(opacity).toBeGreaterThan(0.3);
    await expect.poll(opacity).toBeLessThan(1);

    await row.hover();
    await expect.poll(opacity).toBe(1);

    // A real keyboard journey: Tab from the row's name link onto its controls.
    await page.mouse.move(0, 0);
    await row.locator('a[href^="/location/"]').focus();
    await page.keyboard.press("Tab");
    await expect(row.locator('button[aria-label^="Add a location under LO-Controls695"]')).toBeFocused();
    await expect.poll(opacity).toBe(1);
  });

  // AC4/AC5: Delete guards tested via API (HTMX delete returns error HTML)
  // These are harder to test in E2E without seeded data with volumes/children,
  // but the unit tests cover the service logic. The E2E verifies the UI flow.

  // Issue #185 regression — inline "add child" form must carry the CSRF token
  // and create the sub-location in-place (not silently redirect to /).
  test("inline add-child form creates the sub-location and stays on /locations", async ({
    page,
  }) => {
    await page.goto("/locations");

    // Create the parent.
    await page.locator("summary").filter({ hasText: /add root/i }).click();
    await page.locator("#new-name").fill("LO-IssueOneEightFive-Parent");
    await page.locator("#new-lcode").fill("L5185");
    await page.locator("#add-root-submit").click();
    await expect(page).toHaveURL(/\/locations\/?$/, { timeout: 5000 });
    await expect(
      page.locator("text=LO-IssueOneEightFive-Parent"),
    ).toBeVisible();

    // Extract the parent's id from its edit link so we can target its own
    // inline "add child" toggle button (`data-locations-toggle="add-child-{id}"`).
    const parentEditLink = page
      .locator('a[aria-label*="LO-IssueOneEightFive-Parent"][href*="/edit"]')
      .first();
    await expect(parentEditLink).toBeVisible({ timeout: 3000 });
    const parentHref = await parentEditLink.getAttribute("href");
    const parentId = parentHref?.match(/\/locations\/(\d+)/)?.[1];
    expect(parentId).toBeTruthy();

    // Expand the inline child form (issue #185: this form is rendered by Rust
    // HTML literal, not via Askama template — it bypassed the CSRF audit).
    await page
      .locator(`[data-locations-toggle="add-child-${parentId}"]`)
      .click();

    const childForm = page.locator(`#add-child-${parentId}`);
    await expect(childForm).toBeVisible();

    // The CSRF synchronizer token MUST be present as a hidden input — this is
    // exactly what was missing in 1.1.0 and what caused the silent redirect.
    await expect(
      childForm.locator('input[name="_csrf_token"]'),
    ).toHaveCount(1);

    // Fill the form and submit.
    await childForm.locator('input[name="name"]').fill(
      "LO-IssueOneEightFive-Child",
    );
    await childForm.locator('input[name="label"]').fill("L5186");
    await childForm.locator('button[type="submit"]').click();

    // Before the fix: the POST was rejected, the user landed on /. After the
    // fix: the POST succeeds and we redirect back to /locations.
    await expect(page).toHaveURL(/\/locations\/?$/, { timeout: 5000 });
    await expect(page).not.toHaveURL(/^\/(\?|$)/);
    await expect(
      page.locator("text=LO-IssueOneEightFive-Child"),
    ).toBeVisible();
  });

  // ─── #457 — the proposed L-code must never be one a deleted row holds ──
  //
  // `storage_locations.label` is globally UNIQUE and soft-deletion does not
  // release it, while the proposal used to be computed over live rows only.
  // Deleting a location therefore walked the pre-filled code backwards onto the
  // one the deleted row still held, and accepting it failed on the database
  // index with no explanation. This is the exact cycle that broke the
  // accessibility spec's seed on every run after the first.
  test("creating a location after deleting one reuses no code and succeeds", async ({
    page,
  }) => {
    await page.goto("/locations");

    // 1. Create, taking whatever the form proposes.
    await page.locator("summary").filter({ hasText: /add root|ajouter/i }).click();
    await page.locator("#new-name").fill("L457 First");
    const firstCode = await page.locator("#new-lcode").inputValue();
    expect(firstCode).toMatch(/^L\d{4}$/);
    await page.locator("#add-root-submit").click();

    const firstRow = page
      .locator('[role="treeitem"]')
      .filter({ hasText: "L457 First" })
      .first();
    await expect(firstRow).toBeVisible({ timeout: 10000 });

    // 2. Delete it (soft delete — the row keeps its label).
    await firstRow.locator("button[data-modal-trigger]").first().click();
    await expect(page.locator("#modal-slot dialog[open]")).toBeVisible({ timeout: 5000 });
    await page.locator("[data-modal-confirm]").click();
    await expect(firstRow).toHaveCount(0, { timeout: 10000 });

    // 3. Create again. The proposal must have moved on, not back.
    await page.reload();
    await page.locator("summary").filter({ hasText: /add root|ajouter/i }).click();
    const secondCode = await page.locator("#new-lcode").inputValue();
    expect(
      secondCode,
      "the proposal must not hand back the code the deleted row still holds",
    ).not.toBe(firstCode);

    await page.locator("#new-name").fill("L457 Second");
    await page.locator("#add-root-submit").click();
    await expect(
      page.locator('[role="treeitem"]').filter({ hasText: "L457 Second" }).first(),
    ).toBeVisible({ timeout: 10000 });
  });
});
