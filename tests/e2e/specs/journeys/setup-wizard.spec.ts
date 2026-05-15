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
import AxeBuilder from "@axe-core/playwright";

const SETUP_E2E_GATED = process.env.MYBIBLI_SETUP_E2E !== "1";

// WCAG tag sets shared with `accessibility-full.spec.ts`. Story 10-5
// (closes #162) adds a `/setup` axe check on Step 1, since the gate
// predicate requires an empty users table that only the wizard CI lane
// (this spec's lane) provides.
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag22aa"];

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

    // Story 10-5 — axe-clean assertion on the live wizard. Runs before
    // any form fills so the test exercises the as-rendered admin step.
    {
      const results = await new AxeBuilder({ page })
        .withTags(WCAG_TAGS)
        .analyze();
      const summary = results.violations.map((v) => ({
        rule: v.id,
        impact: v.impact,
        targets: v.nodes.slice(0, 3).map((n) => n.target.join(" ")),
      }));
      expect(
        results.violations,
        `/setup (Step 1): ${results.violations.length} WCAG violation(s):\n${JSON.stringify(summary, null, 2)}`,
      ).toEqual([]);
    }

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

  // Note: the original "resume after browser close" test was removed
  // post-merge of story 8-8 review pass-1. It fundamentally cannot
  // coexist in the same Playwright spec as the smoke test above —
  // both want a pristine `users` table to start, but the smoke test
  // completes the wizard and locks `/setup` into 404 single-use mode.
  // Running them in parallel races on shared DB state; running them
  // serially leaves the second test with no clean baseline to reset
  // to. The CI's `e2e-wizard` lane wipes users/sessions ONCE before
  // the spec, not between tests, so multi-test isolation requires
  // either a TEST_MODE-gated wipe endpoint (out of scope) or
  // dropping a test.
  //
  // **Coverage-equivalent**: the server-side resume property is
  // covered by `tests/setup_wizard.rs::full_happy_path_step_1_through_login`,
  // which walks Step 1 → 2 → 3 → /setup/complete → /setup is 404 →
  // POST /login works. The resume test's specific assertion ("a
  // fresh browser context with NO cookies still resolves to the
  // correct step") is a property of the resolver itself, not of any
  // client-side state — the resolver in `services::setup::resolve_step`
  // never reads cookies, so the integration test's GET /setup
  // (with new admin's cookie OR with no cookie at all) exercises
  // the same code path.
});
