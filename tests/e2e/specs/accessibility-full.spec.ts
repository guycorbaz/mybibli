/**
 * Story 9-22 + 10-5 — WCAG 2.2 AA full coverage via axe-core.
 *
 * Iterates over the top-level URL list and runs axe-core with the
 * WCAG 2.0 A, WCAG 2.0 AA, and WCAG 2.2 AA tag sets. Asserts zero
 * violations per page.
 *
 * Discovered violations during initial run are documented in
 * `docs/accessibility-audit.md` and filed as `type:bug` GH issues.
 *
 * Story 10-5 (closes #162) added coverage for the entity-detail
 * routes: `/title/:id`, `/borrower/:id`, `/contributor/:id`,
 * `/series/:id`. Seeding happens in a single `beforeAll` that creates
 * one of each entity and exposes the IDs to the per-route tests.
 * `/setup` is covered in `setup-wizard.spec.ts` instead, because its
 * gate predicate requires an empty `users` table that only the
 * wizard CI lane provides.
 *
 * Spec ID "AX" — entity seed uses AX-prefixed ISBNs (story 10-5).
 */
import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { loginAs } from "../helpers/auth";
import { specIsbn } from "../helpers/isbn";
import { createBorrower } from "../helpers/loans";

const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag22aa"];

interface UrlSpec {
  url: string;
  role: "anonymous" | "librarian" | "admin";
  label: string;
}

const URLS: UrlSpec[] = [
  { url: "/", role: "anonymous", label: "home" },
  { url: "/catalog", role: "anonymous", label: "catalog (anonymous)" },
  { url: "/catalog", role: "librarian", label: "catalog (librarian)" },
  { url: "/loans", role: "librarian", label: "loans" },
  { url: "/borrowers", role: "librarian", label: "borrowers" },
  { url: "/series", role: "anonymous", label: "series" },
  { url: "/locations", role: "anonymous", label: "locations" },
  { url: "/login", role: "anonymous", label: "login" },
  { url: "/admin?tab=health", role: "admin", label: "admin → health" },
  { url: "/admin?tab=users", role: "admin", label: "admin → users" },
  { url: "/admin?tab=reference_data", role: "admin", label: "admin → reference data" },
  { url: "/admin?tab=trash", role: "admin", label: "admin → trash" },
  { url: "/admin?tab=system", role: "admin", label: "admin → system" },
];

function summariseViolations(
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  violations: any[],
): unknown[] {
  return violations.map((v) => ({
    rule: v.id,
    impact: v.impact,
    targets: v.nodes
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      .slice(0, 3)
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      .map((n: any) => n.target.join(" ")),
  }));
}

test.describe("Story 9-22 — WCAG 2.2 AA full coverage", () => {
  for (const { url, role, label } of URLS) {
    test(`${label} (${url}) passes WCAG 2.2 AA`, async ({ page, context }) => {
      await context.clearCookies();
      if (role !== "anonymous") {
        await loginAs(page, role);
      }
      await page.goto(url);
      await page.waitForLoadState("domcontentloaded");

      const results = await new AxeBuilder({ page })
        .withTags(WCAG_TAGS)
        .analyze();

      expect(
        results.violations,
        `${label}: ${results.violations.length} WCAG violation(s):\n${JSON.stringify(summariseViolations(results.violations), null, 2)}`,
      ).toEqual([]);
    });
  }
});

// ─── Story 10-5 — entity-detail routes ─────────────────────────────────
//
// Seeds one title (via ISBN scan, which auto-creates the primary-author
// contributor via the async metadata-fetch chain), one borrower, and one
// series, then asserts axe-clean on each `:id` route. The contributor id
// is read from the resulting `/title/:id` page once the auto-attach
// lands, which is the same path the production UI exposes.
test.describe("Story 10-5 — entity-detail routes WCAG 2.2 AA", () => {
  // Serial mode: the 4 tests share IDs seeded once in `beforeAll`. Under
  // the default `fullyParallel: true`, Playwright would dispatch each
  // test to its own worker — every worker would re-run `beforeAll` and
  // try to scan the same ISBN, which only the first worker sees as a
  // "new title" (others fall through the `!is_new` path that returns
  // an info-feedback element instead of a `.feedback-skeleton`).
  test.describe.configure({ mode: "serial" });

  const ids: {
    title?: number;
    borrower?: number;
    contributor?: number;
    series?: number;
  } = {};

  test.beforeAll(async ({ browser }) => {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    try {
      await loginAs(page, "admin");

      // 1. Title via ISBN scan. The skeleton feedback carries
      //    `id="feedback-entry-{title_id}"`. Sequence is randomised so
      //    re-running the spec against a non-fresh DB still creates a
      //    NEW title (a re-used ISBN would hit the "title exists" path
      //    and return info-feedback instead of a `.feedback-skeleton`).
      //    `e2e-reset.sh` is the cleanup mechanism for accumulated data.
      const seq = Math.floor(Math.random() * 99999);
      const isbn = specIsbn("AX", seq);
      await page.goto("/catalog");
      const scanField = page.locator("#scan-field");
      await scanField.fill(isbn);
      await scanField.press("Enter");
      const skeleton = page
        .locator(".feedback-skeleton[id^='feedback-entry-']")
        .first();
      await expect(skeleton).toBeVisible({ timeout: 10000 });
      const titleIdAttr = await skeleton.getAttribute("id");
      ids.title = parseInt(titleIdAttr!.replace("feedback-entry-", ""), 10);

      // 2. Contributor — auto-attached by the metadata-fetch task once
      //    the mock-metadata SRU response resolves. Poll the title-detail
      //    page until the `/contributor/:id` anchor appears.
      await expect(async () => {
        await page.goto(`/title/${ids.title}`);
        const link = page.locator('a[href^="/contributor/"]').first();
        await expect(link).toBeVisible({ timeout: 1000 });
        const href = await link.getAttribute("href");
        ids.contributor = parseInt(href!.split("/").pop()!, 10);
      }).toPass({ timeout: 15000 });

      // 3. Borrower — DRY via existing helper, then read the id from the
      //    /borrowers anchor href (same pattern as helpers/loans.ts).
      //    Name is suffixed with the same random seq as the ISBN so a
      //    re-run against a non-fresh DB doesn't pick a borrower from a
      //    prior run (`getBorrowerIdByName`-style helpers fail loud on
      //    duplicate names).
      const borrowerName = `AX-A11y Borrower ${seq}`;
      await createBorrower(page, borrowerName);
      const borrowerLink = page
        .locator('tbody a[href^="/borrower/"]')
        .filter({ hasText: new RegExp(`^\\s*AX-A11y Borrower ${seq}\\s*$`) })
        .first();
      const borrowerHref = await borrowerLink.getAttribute("href");
      ids.borrower = parseInt(borrowerHref!.split("/").pop()!, 10);

      // 4. Series — POST /series, follow the 303 redirect to /series/:id
      //    to capture the new id.
      const csrf = await page
        .locator('meta[name="csrf-token"]')
        .getAttribute("content");
      const seriesResp = await page.request.post("/series", {
        form: {
          _csrf_token: csrf ?? "",
          name: `AX A11y Series ${seq}`,
          series_type: "open",
        },
        maxRedirects: 0,
      });
      if (seriesResp.status() !== 303) {
        throw new Error(
          `Series seed: expected 303, got ${seriesResp.status()} — ${(await seriesResp.text()).slice(0, 200)}`,
        );
      }
      const location = seriesResp.headers()["location"] ?? "";
      const m = location.match(/\/series\/(\d+)/);
      if (!m) {
        throw new Error(`Series seed: could not parse id from Location "${location}"`);
      }
      ids.series = parseInt(m[1]!, 10);
    } finally {
      await ctx.close();
    }
  });

  test.afterAll(async ({ browser }) => {
    // Soft-delete the seeded entities so sibling specs that assume an
    // empty list (empty-states.spec.ts on /series in particular) don't
    // flake when they happen to run after this describe. The cleanup is
    // best-effort — parallel sibling tests can still race against the
    // beforeAll window. The empty-states spec docstring already
    // acknowledges this isolation fragility.
    if (Object.keys(ids).length === 0) return;
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    try {
      await loginAs(page, "admin");
      const csrf = await page
        .locator('meta[name="csrf-token"]')
        .getAttribute("content");
      const headers = { "X-CSRF-Token": csrf ?? "" };
      const drops: Array<[string, number | undefined]> = [
        [`/series/${ids.series}`, ids.series],
        [`/borrower/${ids.borrower}`, ids.borrower],
        [`/catalog/contributors/${ids.contributor}`, ids.contributor],
        [`/catalog/title/${ids.title}`, ids.title],
      ];
      for (const [path, id] of drops) {
        if (id !== undefined) {
          await page.request.delete(path, { headers });
        }
      }
    } finally {
      await ctx.close();
    }
  });

  // All entity-detail routes require an authenticated role — anonymous
  // hits the auth middleware and redirects. Role narrowed to the typed
  // union accepted by `loginAs` so a typo fails `tsc --noEmit` in CI.
  const ROUTES: Array<{
    key: "title" | "borrower" | "contributor" | "series";
    role: "admin" | "librarian";
    label: string;
  }> = [
    { key: "title", role: "admin", label: "title detail" },
    { key: "borrower", role: "librarian", label: "borrower detail" },
    { key: "contributor", role: "admin", label: "contributor detail" },
    { key: "series", role: "admin", label: "series detail" },
  ];

  for (const { key, role, label } of ROUTES) {
    test(`${label} (/${key}/:id) passes WCAG 2.2 AA`, async ({
      page,
      context,
    }) => {
      const id = ids[key];
      expect(
        id,
        `${label}: beforeAll did not seed an id for ${key}`,
      ).toBeDefined();

      await context.clearCookies();
      await loginAs(page, role);
      await page.goto(`/${key}/${id}`);
      await page.waitForLoadState("domcontentloaded");

      const results = await new AxeBuilder({ page })
        .withTags(WCAG_TAGS)
        .analyze();

      expect(
        results.violations,
        `${label}: ${results.violations.length} WCAG violation(s):\n${JSON.stringify(summariseViolations(results.violations), null, 2)}`,
      ).toEqual([]);
    });
  }
});
