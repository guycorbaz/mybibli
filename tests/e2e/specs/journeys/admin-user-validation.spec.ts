/**
 * #55 MEDIUM #2 — admin user creation: username length validation.
 *
 * The create handler now rejects usernames shorter than 3 or longer than 255
 * characters with a localized BadRequest, instead of only checking emptiness
 * (and letting an over-long name surface as a raw DB truncation 500). This
 * spec drives the real POST /admin/users handler with the page's CSRF token,
 * mirroring the seeding pattern in admin-smoke.spec.ts.
 *
 * Spec ID "UV" — creates no persisted rows on the happy path (both cases are
 * rejected before insert).
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

test.describe("#55 — admin username length validation", () => {
  test.beforeEach(async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");
  });

  async function csrfToken(page: import("@playwright/test").Page): Promise<string> {
    await page.goto("/admin?tab=users");
    return page.evaluate(
      () =>
        document.querySelector<HTMLMetaElement>('meta[name="csrf-token"]')
          ?.content || "",
    );
  }

  test("rejects a too-short username with localized copy", async ({ page }) => {
    const csrf = await csrfToken(page);
    const resp = await page.request.post("/admin/users", {
      form: { username: "ab", password: "valid-password-123", role: "librarian", _csrf_token: csrf },
    });
    expect(resp.status()).toBe(400);
    expect(await resp.text()).toMatch(/at least 3 characters|au moins 3 caractères/i);
  });

  test("rejects an over-long username with localized copy", async ({ page }) => {
    const csrf = await csrfToken(page);
    const resp = await page.request.post("/admin/users", {
      form: {
        username: "x".repeat(256),
        password: "valid-password-123",
        role: "librarian",
        _csrf_token: csrf,
      },
    });
    expect(resp.status()).toBe(400);
    expect(await resp.text()).toMatch(/at most 255 characters|au plus 255 caractères/i);
  });

  // Story 19-4 (Rule #9) — a taken username says what happened and what
  // to do; on the creation form it showed the model's raw marker before.
  test("refuses a taken username with what to do", async ({ page }) => {
    const csrf = await csrfToken(page);
    const resp = await page.request.post("/admin/users", {
      form: { username: "librarian", password: "valid-password-123", role: "librarian", _csrf_token: csrf },
    });
    expect(resp.status()).toBe(409);
    const text = await resp.text();
    expect(text).toMatch(
      /The account was not saved: the username “librarian” is already taken\. Choose another username\.|Le compte n(?:'|&#x27;)a pas été enregistré : le nom d(?:'|&#x27;)utilisateur « librarian » est déjà pris\. Choisissez-en un autre\./,
    );
    expect(text).not.toContain(">username_taken<");
  });
});
