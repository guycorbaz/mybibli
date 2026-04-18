import { test, expect } from "@playwright/test";
import { specIsbn } from "../../helpers/isbn";

// NO cookie injection in this file — this is the smoke test
const VALID_ISBN = specIsbn("LS", 1);

test.describe("Login/Logout & Epic 1 Smoke Test (Story 1-9)", () => {
  // AC6: FULL USER JOURNEY — blank browser, no cookies
  test("complete journey: login → catalog → scan ISBN → title created", async ({
    page,
  }) => {
    // Start from home page — no cookies
    await page.goto("/");

    // Click login link in nav bar
    const loginLink = page.locator('a[href="/login"]');
    await expect(loginLink).toBeVisible({ timeout: 5000 });
    await loginLink.click();

    // Verify login form is displayed
    await expect(page).toHaveURL(/\/login/);
    const usernameInput = page.locator("#username");
    const passwordInput = page.locator("#password");
    await expect(usernameInput).toBeVisible();
    await expect(passwordInput).toBeVisible();

    // Fill credentials and submit
    await usernameInput.fill("admin");
    await passwordInput.fill("admin");
    await page.locator("#login-submit").click();

    // Verify redirect to catalog
    await expect(page).toHaveURL(/\/catalog/, { timeout: 5000 });

    // Verify scan field is visible (authenticated)
    const scanField = page.locator("#scan-field");
    await expect(scanField).toBeVisible();

    // Scan an ISBN
    await scanField.fill(VALID_ISBN);
    await scanField.press("Enter");

    // Verify feedback appears (skeleton or resolved)
    const feedback = page.locator(
      "#feedback-list .feedback-skeleton, #feedback-list .feedback-entry"
    );
    await expect(feedback.first()).toBeVisible({ timeout: 5000 });
  });

  // AC3: Failed authentication
  test("invalid credentials show error message", async ({ page }) => {
    await page.goto("/login");

    await page.locator("#username").fill("wrong_user");
    await page.locator("#password").fill("wrong_pass");
    await page.locator("#login-submit").click();

    // Should stay on login page with error
    await expect(page).toHaveURL(/\/login/);
    const errorMsg = page.locator('[role="alert"]');
    await expect(errorMsg).toBeVisible({ timeout: 5000 });
  });

  // AC4: Logout
  test("logout clears session and redirects to home", async ({
    page,
  }) => {
    // First login
    await page.goto("/login");
    await page.locator("#username").fill("admin");
    await page.locator("#password").fill("admin");
    await page.locator("#login-submit").click();
    await expect(page).toHaveURL(/\/catalog/, { timeout: 5000 });

    // Click logout. Story 8-2 converted the nav-bar logout from an
    // `<a href="/logout">` to a POST form + submit button that carries
    // a hidden CSRF input.
    const logoutBtn = page
      .locator('form[action="/logout"] button[type="submit"]')
      .first();
    await expect(logoutBtn).toBeVisible();
    await logoutBtn.click();

    // Should redirect to home
    await expect(page).toHaveURL("/", { timeout: 5000 });

    // Story 7-1 AC #1: /catalog is now anonymous-readable; verify /loans
    // (still librarian-guarded) redirects to /login with next param instead.
    await page.goto("/loans");
    await expect(page).toHaveURL(/\/login\?next=%2Floans/, { timeout: 5000 });
  });

  // AC1: Login form accessibility
  test("login form has proper labels and autocomplete", async ({
    page,
  }) => {
    await page.goto("/login");

    const username = page.locator("#username");
    const password = page.locator("#password");

    // Verify autocomplete attributes
    await expect(username).toHaveAttribute("autocomplete", "username");
    await expect(password).toHaveAttribute(
      "autocomplete",
      "current-password"
    );

    // Verify labels exist
    const usernameLabel = page.locator('label[for="username"]');
    const passwordLabel = page.locator('label[for="password"]');
    await expect(usernameLabel).toBeVisible();
    await expect(passwordLabel).toBeVisible();

    // Verify autofocus on username
    await expect(username).toHaveAttribute("autofocus", "");
  });

  // Story 7-1 AC #1: /catalog is anonymous-readable now. The redirect contract
  // moved to /loans (still librarian-guarded) with a next= round-trip.
  test("anonymous user accessing /loans is redirected to /login with next param", async ({
    page,
  }) => {
    await page.goto("/loans");
    await expect(page).toHaveURL(/\/login\?next=%2Floans/, { timeout: 5000 });
  });
});
