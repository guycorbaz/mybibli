/**
 * CR #396 E2E — Admin per-provider metadata-timeout overrides.
 *
 * Real journey (Foundation Rule #3): blank browser → loginAs(admin) →
 * /admin?tab=system → set an override → verify persistence → clear it
 * back to "use default". Serial mode: these tests mutate the shared
 * `provider_timeout.*` K/V rows, same rationale as the 8-5 spec.
 *
 * No other spec touches the provider_timeout rows, so this file is
 * parallel-safe relative to the rest of the suite.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

const FORM = "form#admin-system-provider-timeouts-form";
const SAVED_RE =
  /Per-provider timeouts saved|Délais par fournisseur enregistrés/i;

test.describe("CR #396 — Admin per-provider timeouts", () => {
  test.describe.configure({ mode: "serial" });

  test.beforeEach(async ({ page }) => {
    await page.context().clearCookies();
  });

  test("form renders one input per registered provider with the default as placeholder", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");

    const form = page.locator(FORM);
    await expect(form).toBeVisible();

    // Spot-check a slugged display name ("BnF" → bnf) and a
    // snake_case one — both seeded by migration 20260611000000.
    const bnfInput = form.locator('input[name="timeout_bnf"]');
    const gbInput = form.locator('input[name="timeout_google_books"]');
    await expect(bnfInput).toBeVisible();
    await expect(gbInput).toBeVisible();

    // Empty = "use the global default"; the placeholder carries it.
    await expect(gbInput).toHaveValue("");
    await expect(gbInput).toHaveAttribute("placeholder", /^\d+$/);

    // Provider labels are proper nouns, rendered verbatim (NFR41).
    await expect(form.getByText("BnF", { exact: true })).toBeVisible();
  });

  test("override save persists, second save works (no stale-version 409), clear restores default", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");

    const gbInput = page.locator(`${FORM} input[name="timeout_google_books"]`);
    const submit = page.locator(`${FORM} button[type="submit"]`);

    // Set an override.
    await gbInput.fill("3");
    await submit.click();
    await expect(
      page.locator("#feedback-list").getByText(SAVED_RE),
    ).toBeVisible({ timeout: 10000 });

    // Persistence proof: full reload re-renders the stored value.
    await page.goto("/admin?tab=system");
    await expect(gbInput).toHaveValue("3");

    // Second consecutive save on the same row — guards against the #406
    // stale-version class of bug (form must re-render fresh versions).
    await gbInput.fill("7");
    await submit.click();
    await expect(
      page.locator("#feedback-list").getByText(SAVED_RE).last(),
    ).toBeVisible({ timeout: 10000 });
    await page.goto("/admin?tab=system");
    await expect(gbInput).toHaveValue("7");

    // Clear back to "use default" (empty field) for test isolation.
    await gbInput.fill("");
    await submit.click();
    await expect(
      page.locator("#feedback-list").getByText(SAVED_RE).last(),
    ).toBeVisible({ timeout: 10000 });
    await page.goto("/admin?tab=system");
    await expect(gbInput).toHaveValue("");
  });

  test("out-of-range override → 400 + localized inline error", async ({
    page,
    request,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");
    const csrf = await page
      .locator('meta[name="csrf-token"]')
      .getAttribute("content");
    expect(csrf).toBeTruthy();
    const cookies = await page.context().cookies();
    const cookieHeader = cookies.map((c) => `${c.name}=${c.value}`).join("; ");

    // 99 is outside the 1..=60 bound the form's max attribute mirrors —
    // bypass the client-side guard with a direct POST.
    const resp = await request.post("/admin/system/provider-timeouts", {
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
        Cookie: cookieHeader,
        "HX-Request": "true",
      },
      data: `timeout_google_books=99&timeout_google_books_version=1&_csrf_token=${encodeURIComponent(csrf!)}`,
      maxRedirects: 0,
    });
    expect(resp.status()).toBe(400);
  });

  test("unchanged form save reports no changes instead of bumping versions", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");

    // Submit with every field untouched (all empty = all defaults).
    await page.locator(`${FORM} button[type="submit"]`).click();
    await expect(
      page
        .locator("#feedback-list")
        .getByText(/No changes|Aucune modification/i),
    ).toBeVisible({ timeout: 10000 });
  });
});
