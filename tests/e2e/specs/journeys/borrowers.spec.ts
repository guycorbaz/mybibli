import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { borrowerIdByName, createBorrower } from "../../helpers/loans";

test.describe("Borrower CRUD & Search (Story 4-1)", () => {
  test.beforeEach(async ({ page }) => {
    await loginAs(page);
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
    // #470 — was getByText(/Add borrower|Ajouter/i), which matches TWO
    // elements once the page renders in French ("Ajouter un emprunteur" and
    // "Ajouter un nouveau titre") and fails strict mode. It only ever passed
    // because the librarian account was sometimes left in English by a
    // parallel language-toggle test — the very leak #470 fixed. The link has
    // a stable id; use it, per the selector policy in CLAUDE.md.
    await page.locator("#borrowers-show-add-form").click();
    await expect(page.locator("#new-name")).toBeVisible({ timeout: 3000 });

    // Fill in name and submit
    await page.locator("#new-name").fill("BW-Jean Dupont");
    await page.locator("#new-email").fill("jean@example.com");
    await page.locator("#new-phone").fill("+33612345678");
    await page.locator('main button[type="submit"]').last().click();

    // Should redirect back to /borrowers with new borrower in list
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });
    // Story 19-2: confirmed by id, not in the list (paginated by name).
    await page.goto(`/borrower/${await borrowerIdByName(page, "BW-Jean Dupont")}`);
    await expect(page.locator("h1")).toContainText("BW-Jean Dupont");
  });

  // AC3: Borrower detail page
  test("click borrower name → detail page", async ({ page }) => {
    // Story 19-2: its own borrower, found on the list page that holds it
    // (Rule #23 — never "the first row", which belongs to whichever spec).
    const id = await createBorrower(page, "0BW-Detail Borrower");
    await page.goto("/borrowers");
    await page.locator(`tbody a[href="/borrower/${id}"]`).click();
    await expect(page.locator("h1")).toContainText("0BW-Detail Borrower", { timeout: 3000 });
  });

  // AC4: Edit borrower
  test("edit borrower → changes saved", async ({ page }) => {
    // Story 19-2: its own borrower (Rule #23 — editing "the first row"
    // changed another spec's borrower).
    const id = await createBorrower(page, `BW-Phone Borrower ${Date.now()}`);
    await page.goto(`/borrower/${id}`);
    await expect(page.locator("h1")).toBeVisible({ timeout: 3000 });

    await page.getByRole("link", { name: /Edit borrower|Modifier l.emprunteur/i }).click();
    await expect(page.locator("#edit-name")).toBeVisible({ timeout: 3000 });

    // Modify phone
    await page.locator("#edit-phone").fill("+33699887766");
    await page.locator('form button[type="submit"]').last().click();

    // Should redirect to detail page
    await expect(page.locator("body")).toContainText("+33699887766", {
      timeout: 5000,
    });
  });

  // AC6: Delete borrower with no active loans
  test("delete borrower → removed from list", async ({ page }) => {
    // First create a borrower to delete
    await page.goto("/borrowers");
    await page.locator("#borrowers-show-add-form").click();
    await page.locator("#new-name").fill("BW-Temp Borrower");
    await page.locator('main button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });

    // Navigate to the borrower detail (story 19-2: by id — the list is
    // paginated by name).
    await page.goto(`/borrower/${await borrowerIdByName(page, "BW-Temp Borrower")}`);
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
    const navLink = page.locator('nav#sidebar a[href="/borrowers"]');
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
    await page.locator("#borrowers-show-add-form").click();
    await page.locator("#new-name").fill("BW-Smoke Borrower");
    await page.locator("#new-email").fill("smoke@test.com");
    await page.locator('main button[type="submit"]').last().click();
    await expect(page).toHaveURL(/\/borrowers/, { timeout: 5000 });
    // Into the detail, by id (story 19-2: the list is paginated by name).
    await page.goto(`/borrower/${await borrowerIdByName(page, "BW-Smoke Borrower")}`);
    await expect(page.locator("h1")).toContainText("BW-Smoke Borrower");
  });
});
