import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

// Story 6-2 smoke: proves the librarian seed + role-aware loginAs() end-to-end,
// starting from a blank browser context. Uses read-only navigation only, because
// create/edit/delete of locations still requires the Admin role pending Epic 7.
test.describe("Librarian login smoke (story 6-2)", () => {
  test("librarian can log in and reach catalog + locations pages", async ({ page }) => {
    await loginAs(page, "librarian");

    // /catalog requires >= Librarian — the scan field should render.
    await page.goto("/catalog");
    await expect(page.locator("#scan-field")).toBeVisible({ timeout: 5000 });

    // /locations requires >= Librarian (read access to the tree).
    await page.goto("/locations");
    await expect(page).toHaveURL(/\/locations/);
    await expect(page.locator("h1").first()).toBeVisible({ timeout: 5000 });

    // Session resolves to librarian role server-side: logout link must exist.
    await expect(page.locator('a[href="/logout"]')).toBeVisible();

    // Role-specific assertion — proves the session is NOT admin.
    // Creating a location is Admin-only (src/routes/locations.rs); a librarian
    // session must be rejected. Any non-2xx status is acceptable (403/401/303).
    const adminOnly = await page.request.post("/locations", {
      form: { name: "librarian-smoke-denied", parent_id: "", dewey: "" },
      maxRedirects: 0,
      failOnStatusCode: false,
    });
    expect(
      adminOnly.status(),
      "librarian must not be able to create a location (admin-only)",
    ).toBeGreaterThanOrEqual(300);
    expect(adminOnly.status()).toBeLessThan(500);
  });
});
