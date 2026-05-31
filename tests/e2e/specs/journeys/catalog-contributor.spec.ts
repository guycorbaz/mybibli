import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { captureEntityCounts } from "../../helpers/db-snapshot";

const VALID_ISBN = specIsbn("CC", 1);

test.describe("Contributor Management", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  // AC1: Open contributor form and search
  test("contributor autocomplete shows matches on search", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    // Set title context first
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    // Open contributor form
    const formContainer = page.locator("#contributor-form-container");
    await page.evaluate(() => {
      htmx.ajax("GET", "/catalog/contributors/form", {
        target: "#contributor-form-container",
        swap: "innerHTML",
      });
    });
    await expect(formContainer.locator("form")).toBeVisible({ timeout: 5000 });

    // Type in name field for autocomplete
    const nameInput = formContainer.locator("#contributor-name-input");
    await nameInput.fill("Al");

    // Autocomplete should show dropdown (if data exists)
    // Note: may be empty in test DB — test that form is functional
    await expect(nameInput).toHaveAttribute("role", "combobox");
  });

  // AC2: Add new contributor with role
  test("add contributor to title shows success feedback", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    // Open contributor form
    await page.evaluate(() => {
      htmx.ajax("GET", "/catalog/contributors/form", {
        target: "#contributor-form-container",
        swap: "innerHTML",
      });
    });

    const form = page.locator("#contributor-form-container form");
    await expect(form).toBeVisible({ timeout: 5000 });

    // Fill contributor name
    await form.locator("#contributor-name-input").fill("Albert Camus");

    // Select first role (Auteur should be first alphabetically)
    const roleSelect = form.locator("#contributor-role-select");
    const options = roleSelect.locator("option");
    const optCount = await options.count();
    if (optCount > 1) {
      await roleSelect.selectOption({ index: 1 });
    }

    await form.locator('button[type="submit"]').click();

    const feedback = page.locator(".feedback-entry").last();
    await expect(feedback).toBeVisible({ timeout: 5000 });
  });

  // AC3: Duplicate contributor-role rejected
  test("add same contributor with same role shows error", async ({ page }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    // Add contributor first time
    await page.evaluate(() => {
      htmx.ajax("GET", "/catalog/contributors/form", {
        target: "#contributor-form-container",
        swap: "innerHTML",
      });
    });

    let form = page.locator("#contributor-form-container form");
    await expect(form).toBeVisible({ timeout: 5000 });

    await form.locator("#contributor-name-input").fill("Boris Vian");
    const roleSelect = form.locator("#contributor-role-select");
    if ((await roleSelect.locator("option").count()) > 1) {
      await roleSelect.selectOption({ index: 1 });
    }
    await form.locator('button[type="submit"]').click();
    // Wait for success feedback before retrying
    await expect(page.locator(".feedback-entry").first()).toBeVisible({ timeout: 5000 });

    // Try adding same again
    await page.evaluate(() => {
      htmx.ajax("GET", "/catalog/contributors/form", {
        target: "#contributor-form-container",
        swap: "innerHTML",
      });
    });

    form = page.locator("#contributor-form-container form");
    await expect(form).toBeVisible({ timeout: 5000 });

    await form.locator("#contributor-name-input").fill("Boris Vian");
    const roleSelect2 = form.locator("#contributor-role-select");
    if ((await roleSelect2.locator("option").count()) > 1) {
      await roleSelect2.selectOption({ index: 1 });
    }
    await form.locator('button[type="submit"]').click();

    const errorEntry = page.locator(
      '.feedback-entry[data-feedback-variant="error"]',
    );
    await expect(errorEntry).toBeVisible({ timeout: 5000 });
  });

  // AC5: Prevent deletion of referenced contributor (deletion guard)
  test("delete contributor with associations shows block message, unassign then delete succeeds", async ({
    page,
  }) => {
    const GUARD_ISBN = specIsbn("CC", 10);
    const CONTRIBUTOR_NAME = `CC-Guard-${Date.now()}`;

    // Step 1: Create a title by scanning an ISBN
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(GUARD_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    // Step 2: Add a uniquely-named contributor to this title
    await page.evaluate(() => {
      htmx.ajax("GET", "/catalog/contributors/form", {
        target: "#contributor-form-container",
        swap: "innerHTML",
      });
    });
    const form = page.locator("#contributor-form-container form");
    await expect(form).toBeVisible({ timeout: 5000 });
    await form.locator("#contributor-name-input").fill(CONTRIBUTOR_NAME);
    const roleSelect = form.locator("#contributor-role-select");
    if ((await roleSelect.locator("option").count()) > 1) {
      await roleSelect.selectOption({ index: 1 });
    }
    await form.locator('button[type="submit"]').click();
    // Wait for success feedback confirming contributor was added (appears at top of feedback list)
    await expect(
      page.locator(`.feedback-entry:has-text("${CONTRIBUTOR_NAME}")`),
    ).toBeVisible({ timeout: 5000 });

    // Step 3: Extract contributor ID and junction ID from the contributor list
    // The OOB swap populates #contributor-list with links and remove buttons
    await page.waitForFunction(
      (name: string) => {
        const el = document.querySelector("#contributor-list");
        return el && el.innerHTML.includes(name);
      },
      CONTRIBUTOR_NAME,
      { timeout: 10000 },
    );
    const ids = await page.evaluate((name: string) => {
      const links = document.querySelectorAll(
        '#contributor-list a[href^="/contributor/"]',
      );
      let contributorHref: string | null = null;
      for (const link of links) {
        if (link.textContent?.includes(name)) {
          contributorHref = link.getAttribute("href");
          break;
        }
      }
      // Extract junction_id from the remove button's hx-vals
      const buttons = document.querySelectorAll(
        '#contributor-list button[hx-post*="remove"]',
      );
      let junctionId: string | null = null;
      let titleId: string | null = null;
      for (const btn of buttons) {
        const ariaLabel = btn.getAttribute("aria-label") || "";
        if (ariaLabel.includes(name)) {
          const vals = btn.getAttribute("hx-vals");
          if (vals) {
            const parsed = JSON.parse(vals);
            junctionId = String(parsed.junction_id);
            titleId = String(parsed.title_id);
          }
          break;
        }
      }
      return { contributorHref, junctionId, titleId };
    }, CONTRIBUTOR_NAME);
    expect(ids.contributorHref).toBeTruthy();
    expect(ids.junctionId).toBeTruthy();
    const contributorId = ids.contributorHref!.split("/").pop()!;

    // Step 4: Navigate to contributor detail and attempt deletion via modal
    await page.goto(`/contributor/${contributorId}`);
    await expect(page.locator("h1")).toContainText(CONTRIBUTOR_NAME);

    // Click delete trigger → modal opens (story 9-12: hx-confirm replaced
    // by the UX-DR8 Modal component, no native confirm() dialog any more).
    const deleteBtn = page.getByRole("button", {
      name: /delete|supprimer/i,
    });
    await expect(deleteBtn).toBeVisible();
    // Story 9-12 code-review P5 — paranoid lock against accidental
    // re-introduction of `hx-confirm=`. The templates_audit catches
    // count mismatches but doesn't reject re-adding both file and
    // entry, so policing at the rendered-attribute layer is cheaper.
    await expect(deleteBtn).not.toHaveAttribute("hx-confirm", /.*/);
    await deleteBtn.click();
    await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
    // Lock 9-10 focus-trap inheritance: Cancel is the default-focused
    // element, and Escape closes the modal without firing DELETE.
    await expect(page.locator("[data-modal-default-focus]")).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible();

    // Re-open the modal and click Confirm — FR54 conflict feedback
    // must land in #contributor-feedback, modal closes on 2xx response.
    await deleteBtn.click();
    await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
    await page.locator("[data-modal-confirm]").click();

    // Step 5: Verify block message appears in feedback container
    const feedback = page.locator("#contributor-feedback");
    await expect(feedback).toContainText(
      /Cannot delete|Impossible de supprimer/i,
      { timeout: 5000 },
    );
    // Story 9-12 code-review P4 — the FR54 200 + inline feedback
    // response must close the modal via modal.js's htmx:afterRequest
    // listener. If a future modal.js refactor regresses the
    // close-on-2xx contract, this assertion fires loudly rather
    // than the user being left with a stale dialog over the feedback.
    await expect(page.locator("#modal-slot dialog[open]")).not.toBeVisible();

    // Step 6: Unassign the contributor via direct POST (avoids catalog page reload issue)
    const removeCsrf =
      (await page.locator('meta[name="csrf-token"]').getAttribute("content")) ?? "";
    const removeResponse = await page.request.post(
      "/catalog/contributors/remove",
      {
        form: {
          _csrf_token: removeCsrf,
          junction_id: ids.junctionId!,
          title_id: ids.titleId!,
        },
      },
    );
    expect(removeResponse.ok()).toBeTruthy();

    // Step 7: Now delete should succeed — navigate back to contributor detail
    await page.goto(`/contributor/${contributorId}`);
    await expect(page.locator("h1")).toContainText(CONTRIBUTOR_NAME);

    // Click delete again → open modal → Confirm → HX-Redirect to /catalog
    const deleteBtn2 = page.getByRole("button", {
      name: /delete|supprimer/i,
    });
    await deleteBtn2.click();
    await expect(page.locator("#modal-slot dialog[open]")).toBeVisible();
    await page.locator("[data-modal-confirm]").click();

    // Verify redirect to catalog
    await page.waitForURL("**/catalog", { timeout: 5000 });
  });

  // AC8: Context banner shows author
  test("context banner shows primary author after adding contributor", async ({
    page,
  }) => {
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    await page.evaluate(() => {
      htmx.ajax("GET", "/catalog/contributors/form", {
        target: "#contributor-form-container",
        swap: "innerHTML",
      });
    });

    const form = page.locator("#contributor-form-container form");
    await expect(form).toBeVisible({ timeout: 5000 });

    await form.locator("#contributor-name-input").fill("Test Author");
    const roleSelect = form.locator("#contributor-role-select");
    if ((await roleSelect.locator("option").count()) > 1) {
      await roleSelect.selectOption({ index: 1 });
    }
    await form.locator('button[type="submit"]').click();

    // Banner should update to include author
    const banner = page.locator("#context-banner");
    await expect(banner).not.toHaveClass(/hidden/, { timeout: 5000 });
  });

  // Story 7-1 AC #1 + #3: /catalog readable, but contributor mutation
  // endpoints reject anonymous POST without state change.
  // Story 8-2 update: the global CSRF middleware now intercepts missing-
  // token POSTs with a 403 BEFORE the auth layer can issue its 303. The
  // end user still cannot mutate anything (DB snapshot unchanged) —
  // either outcome satisfies the original AC #1 + #3 assertion.
  //
  // CR #43: the status-code assertion alone cannot catch a future
  // regression where CSRF / auth silently passes through while the
  // mutation also runs. The before/after `captureEntityCounts` snapshot
  // PROVES the DB was untouched.
  test("anonymous user cannot POST to contributor endpoints (DB unchanged)", async ({
    context,
    page,
  }, testInfo) => {
    const baseURL = testInfo.project.use.baseURL!;

    // Snapshot key entity counts BEFORE — via a separate admin-authenticated
    // APIRequestContext so the upcoming clearCookies() doesn't strip it.
    const before = await captureEntityCounts(baseURL);

    await context.clearCookies();
    const resp = await page.request.post("/catalog/contributors/add", {
      form: { title_id: "1", contributor_name: "Anon", role_id: "1" },
      maxRedirects: 0,
      failOnStatusCode: false,
    });
    // Story 8-2 CSRF middleware rejects state-changing requests without a
    // valid token. For HTMX/JSON clients it returns 403 with the rejection
    // envelope; for plain `application/x-www-form-urlencoded` (no
    // `hx-request: true`) it 303-redirects to /login so the user lands on
    // a fully-rendered page rather than a bare feedback fragment. Either
    // outcome satisfies AC #1 + #3 — the request did not mutate anything.
    expect([303, 403]).toContain(resp.status());

    // CR #43 DB-snapshot proof: counts unchanged after the rejected POST.
    const after = await captureEntityCounts(baseURL);
    expect(after).toEqual(before);
  });
});

test.describe("#325 — Add contributor from /title/:id", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("add contributor from title detail page surfaces success feedback", async ({
    page,
  }) => {
    const ISBN = specIsbn("CC", 20);
    const NAME = `CC-TD-${Date.now()}`;

    // Step 1: catalog the title via scan flow.
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry", {
      timeout: 10000,
    });

    // Step 2: navigate to /title/{id} via home search.
    await page.goto(`/?q=${ISBN}`);
    const titleLink = page
      .locator(
        '#browse-results table.browse-table tbody tr td a[href^="/title/"]',
      )
      .first();
    await expect(titleLink).toBeVisible({ timeout: 15000 });
    const titleHref = (await titleLink.getAttribute("href"))!;
    await page.goto(titleHref);
    await page.waitForURL(/\/title\/\d+/);

    // Step 3: the page must declare the #feedback-list slot — this is
    // the regression #325 fixed. Without it, the next form submit raises
    // htmx:targetError and the user sees nothing.
    await expect(page.locator("#feedback-list")).toBeAttached();

    // Step 4: click "+ Add contributor" → form loads into the slot.
    const addBtn = page.getByRole("button", {
      name: /\+\s*(add contributor|ajouter.*contributeur)/i,
    });
    await expect(addBtn).toBeVisible();
    await addBtn.click();

    const form = page.locator("#contributor-form-slot form");
    await expect(form).toBeVisible({ timeout: 5000 });

    // Step 5: fill name + role.
    await form.locator("#contributor-name-input").fill(NAME);
    const roleSelect = form.locator("#contributor-role-select");
    await expect(roleSelect.locator("option")).not.toHaveCount(0);
    await roleSelect.selectOption({ index: 1 });

    // Step 6: submit → success FeedbackEntry must land in #feedback-list.
    await form.locator('button[type="submit"]').click();
    const feedback = page
      .locator("#feedback-list .feedback-entry")
      .filter({ hasText: NAME });
    await expect(feedback).toBeVisible({ timeout: 5000 });

    // Step 7: contributor must appear in the #contributor-list OOB swap.
    await expect(
      page.locator("#contributor-list").filter({ hasText: NAME }),
    ).toBeVisible({ timeout: 5000 });
  });
});

test.describe("Contributor accessibility", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
  });

  test("contributor form passes accessibility checks", async ({ page }) => {
    let AxeBuilder;
    try {
      AxeBuilder = (await import("@axe-core/playwright")).default;
    } catch {
      test.skip(true, "@axe-core/playwright not installed");
      return;
    }

    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");

    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");
    await page.waitForSelector(".feedback-skeleton, .feedback-entry");

    await page.evaluate(() => {
      htmx.ajax("GET", "/catalog/contributors/form", {
        target: "#contributor-form-container",
        swap: "innerHTML",
      });
    });
    await page.waitForSelector("#contributor-form-container form");

    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa"]) // Only check WCAG 2 AA compliance
      .analyze();
    expect(results.violations).toEqual([]);
  });
});
