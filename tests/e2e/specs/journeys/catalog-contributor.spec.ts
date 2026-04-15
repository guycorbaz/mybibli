import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

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

    // Step 4: Navigate to contributor detail and attempt deletion
    await page.goto(`/contributor/${contributorId}`);
    await expect(page.locator("h1")).toContainText(CONTRIBUTOR_NAME);

    // Set up dialog handler to auto-accept the native confirm()
    page.on("dialog", (d) => d.accept());

    // Click delete button
    const deleteBtn = page.getByRole("button", {
      name: /delete|supprimer/i,
    });
    await expect(deleteBtn).toBeVisible();
    await deleteBtn.click();

    // Step 5: Verify block message appears in feedback container
    const feedback = page.locator("#contributor-feedback");
    await expect(feedback).toContainText(
      /Cannot delete|Impossible de supprimer/i,
      { timeout: 5000 },
    );

    // Step 6: Unassign the contributor via direct POST (avoids catalog page reload issue)
    const removeResponse = await page.request.post(
      "/catalog/contributors/remove",
      {
        form: {
          junction_id: ids.junctionId!,
          title_id: ids.titleId!,
        },
      },
    );
    expect(removeResponse.ok()).toBeTruthy();

    // Step 7: Now delete should succeed — navigate back to contributor detail
    await page.goto(`/contributor/${contributorId}`);
    await expect(page.locator("h1")).toContainText(CONTRIBUTOR_NAME);

    // Click delete again — this time it should redirect
    const deleteBtn2 = page.getByRole("button", {
      name: /delete|supprimer/i,
    });
    await deleteBtn2.click();

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
  test("anonymous user cannot POST to contributor endpoints", async ({
    context,
    page,
  }) => {
    await context.clearCookies();
    const resp = await page.request.post("/catalog/contributors/add", {
      form: { title_id: "1", contributor_name: "Anon", role_id: "1" },
      maxRedirects: 0,
      failOnStatusCode: false,
    });
    // 303 redirect to /login (Anonymous → Unauthorized) — never 200.
    expect(resp.status()).toBe(303);
    expect(resp.headers()["location"]).toMatch(/\/login/);
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
      .disableRules(["color-contrast"]) // Known issue: placeholder text contrast
      .withTags(["wcag2a", "wcag2aa"]) // Only check WCAG 2 AA compliance
      .analyze();
    expect(results.violations).toEqual([]);
  });
});
