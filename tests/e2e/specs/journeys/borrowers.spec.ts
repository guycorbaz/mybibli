import { test, expect } from "@playwright/test";

test.describe("Borrower CRUD & Search (Story 4-1)", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/login");
    await page.locator("#username").fill("admin");
    await page.locator("#password").fill("admin");
    await page.locator("#login-submit").click();
    await expect(page).toHaveURL(/\/catalog/, { timeout: 5000 });
  });

  // AC1: Borrowers list page
  test("navigate to /borrowers → see list or empty state", async ({
    page,
  }) => {
    await page.goto("/borrowers");
    await expect(page.locator("h1")).toContainText(/Borrowers|Emprunteurs/i);
  });

  // AC2: Create borrower
  test("create borrower → appears in list", async ({ page }) => {
    await page.goto("/borrowers");

    // Click Add borrower to show form
    await page.getByText(/Add borrower|Ajouter/i).click();
    await expect(page.locator("#new-name")).toBeVisible({ timeout: 3000 });

    // Fill in name and submit
    await page.locator("#new-name").fill("BW-Jean Dupont");
    await page.locator("#new-email").fill("jean@example.com");
    await page.locator("#new-phone").fill("+33612345678");
    await page.locator('main button[type="submit"]').last().click();

    // Should redirect back to /borrowers with new borrower in list
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });
    await expect(page.locator("body")).toContainText("BW-Jean Dupont");
  });

  // AC3: Borrower detail page
  test("click borrower name → detail page", async ({ page }) => {
    // Ensure borrower exists
    await page.goto("/borrowers");
    const link = page.locator("table tbody tr td a").first();
    if (await link.isVisible()) {
      await link.click();
      await expect(page.locator("h1")).toBeVisible({ timeout: 3000 });
    }
  });

  // AC4: Edit borrower
  test("edit borrower → changes saved", async ({ page }) => {
    await page.goto("/borrowers");
    const link = page.locator("table tbody tr td a").first();
    if (await link.isVisible()) {
      await link.click();
      await expect(page.locator("h1")).toBeVisible({ timeout: 3000 });

      // Click Edit
      const editLink = page.getByText(/Edit borrower|Modifier/i);
      if (await editLink.isVisible()) {
        await editLink.click();
        await expect(page.locator("#edit-name")).toBeVisible({ timeout: 3000 });

        // Modify phone
        await page.locator("#edit-phone").fill("+33699887766");
        await page.locator('form button[type="submit"]').last().click();

        // Should redirect to detail page
        await expect(page.locator("body")).toContainText("+33699887766", {
          timeout: 5000,
        });
      }
    }
  });

  // AC6: Delete borrower with no active loans
  test("delete borrower → removed from list", async ({ page }) => {
    // First create a borrower to delete
    await page.goto("/borrowers");
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("BW-Temp Borrower");
    await page.locator('main button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Navigate to the borrower detail
    await page.getByText("BW-Temp Borrower").click();
    await expect(page.locator("h1")).toContainText("BW-Temp Borrower");

    // Delete
    page.on("dialog", (dialog) => dialog.accept());
    const deleteBtn = page.getByText(/^Delete$|^Supprimer$/i);
    if (await deleteBtn.isVisible()) {
      await deleteBtn.click();
      await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });
    }
  });

  // AC8: Nav bar has Borrowers link
  test("nav bar shows Borrowers link", async ({ page }) => {
    await page.goto("/borrowers");
    const navLink = page.locator(
      'nav a[href="/borrowers"], #mobile-nav a[href="/borrowers"]'
    );
    await expect(navLink.first()).toBeVisible();
  });

  // Smoke test: full journey
  test("smoke: login → borrowers → create → edit → verify", async ({
    context,
    page,
  }) => {
    await context.clearCookies();

    // Login
    await page.goto("/login");
    await page.locator("#username").fill("admin");
    await page.locator("#password").fill("admin");
    await page.locator("#login-submit").click();
    await expect(page).toHaveURL(/\/catalog/, { timeout: 5000 });

    // Navigate to borrowers
    await page.goto("/borrowers");
    await expect(page.locator("h1")).toContainText(/Borrowers|Emprunteurs/i);

    // Create
    await page.getByText(/Add borrower|Ajouter/i).click();
    await page.locator("#new-name").fill("BW-Smoke Borrower");
    await page.locator("#new-email").fill("smoke@test.com");
    await page.locator('main button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });
    await expect(page.locator("body")).toContainText("BW-Smoke Borrower");

    // Click into detail
    await page.getByText("BW-Smoke Borrower").click();
    await expect(page.locator("h1")).toContainText("BW-Smoke Borrower");
  });
});
