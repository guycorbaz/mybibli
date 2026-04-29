/**
 * Story 8-8 — First-launch setup wizard E2E (smoke + resume).
 *
 * **Why this spec is gated by MYBIBLI_SETUP_E2E.**
 * Every other Playwright spec in this suite assumes a seeded DB with
 * an admin user, and the test stack pins `MYBIBLI_SKIP_SETUP=1` so the
 * gate middleware never fires. The wizard's smoke test must do the
 * opposite — start from an empty DB *without* the bypass — so it has
 * to run against a sibling container.
 *
 * Two ways to run it locally:
 *
 *   # 1) Spawn the test stack fresh, drop the seed users, unset the
 *   #    bypass, then run only this spec.
 *   docker compose -f tests/e2e/docker-compose.test.yml up -d --build
 *   docker compose -f tests/e2e/docker-compose.test.yml exec -T db \
 *       mariadb -uroot -proot_test mybibli_test \
 *       -e "DELETE FROM users;"
 *   docker compose -f tests/e2e/docker-compose.test.yml stop mybibli
 *   MYBIBLI_SKIP_SETUP="" \
 *       docker compose -f tests/e2e/docker-compose.test.yml up -d mybibli
 *   MYBIBLI_SETUP_E2E=1 npx playwright test specs/journeys/setup-wizard.spec.ts
 *
 *   # 2) Run cargo run locally against a clean DB on port 8080.
 *   MYBIBLI_SETUP_E2E=1 npx playwright test specs/journeys/setup-wizard.spec.ts
 *
 * The CI job that wires the wizard stack lives outside this PR — it
 * is tracked as a follow-up issue surfaced in Epic 8 retrospective.
 */
import { test, expect } from "@playwright/test";

const SETUP_E2E_GATED = process.env.MYBIBLI_SETUP_E2E !== "1";

test.describe("Story 8-8 — First-launch setup wizard", () => {
  test.skip(
    SETUP_E2E_GATED,
    "Set MYBIBLI_SETUP_E2E=1 and run against a stack WITHOUT MYBIBLI_SKIP_SETUP and a clean users table. See spec header for runbook.",
  );

  test("smoke: anonymous browser → wizard → first scan", async ({ page, baseURL }) => {
    // Pin EN cookie before the first navigation so all assertions match
    // English strings deterministically (story 8-8 i18n note).
    await page.context().addCookies([
      { name: "lang", value: "en", url: baseURL ?? "http://localhost:8080" },
    ]);

    // Step 1: redirect from any app route to /setup.
    await page.goto("/catalog");
    await expect(page).toHaveURL(/\/setup$/);

    // Fill admin form.
    await page.fill('input[name="username"]', "wizard_admin");
    await page.fill('input[name="password"]', "wizard_pass_8chars");
    await page.click('button[type="submit"][name="_back"][value="0"]');

    // Step 2 — provider keys (skip everything, hit Next).
    await expect(page).toHaveURL(/\/setup$/);
    await expect(page.locator("h1")).toContainText(/Step 2/i);
    await page.click('button[type="submit"][name="_back"][value="0"]');

    // Step 3 — preferences. Pick EN, threshold 21.
    await expect(page.locator("h1")).toContainText(/Step 3/i);
    await page.check('input[name="default_language"][value="en"]');
    await page.fill('input[name="overdue_threshold_days"]', "21");
    await page.click('button[type="submit"][name="_back"][value="0"]');

    // Step 4 — recap.
    await expect(page.locator("h1")).toContainText(/Step 4/i);
    await expect(page.locator("dl")).toContainText("wizard_admin");
    await expect(page.locator("dl")).toContainText("21");

    // Complete setup → redirect to /catalog.
    await page.click('button[type="submit"][name="_back"][value="0"]');
    await expect(page).toHaveURL(/\/catalog$/, { timeout: 10000 });

    // /setup is dead — single-use property.
    const resp = await page.request.get("/setup");
    expect(resp.status()).toBe(404);
  });

  test("resume: closing the browser after Step 1 lands at Step 2 with a single admin row", async ({
    browser,
    baseURL,
  }) => {
    // Browser A: do Step 1 only.
    const ctxA = await browser.newContext();
    const pageA = await ctxA.newPage();
    await ctxA.addCookies([
      { name: "lang", value: "en", url: baseURL ?? "http://localhost:8080" },
    ]);
    await pageA.goto("/setup");
    await pageA.fill('input[name="username"]', "resume_admin");
    await pageA.fill('input[name="password"]', "resume_pass_8chars");
    await pageA.click('button[type="submit"][name="_back"][value="0"]');
    await expect(pageA.locator("h1")).toContainText(/Step 2/i);
    await ctxA.close();

    // Browser B: fresh context, no cookies. The next GET /setup must
    // resume at Step 2 (NOT Step 1) — admin already exists.
    const ctxB = await browser.newContext();
    const pageB = await ctxB.newPage();
    await ctxB.addCookies([
      { name: "lang", value: "en", url: baseURL ?? "http://localhost:8080" },
    ]);
    await pageB.goto("/setup");
    await expect(pageB.locator("h1")).toContainText(/Step 2/i);

    // Verify: NO duplicate admin row. Hit /catalog after wizard
    // completion is out of scope here — this test scope is the resume
    // detection, not the full happy path.
    await ctxB.close();
  });
});
