/**
 * Story 9-22 — WCAG 2.2 AA full coverage via axe-core.
 *
 * Iterates over the top-level URL list and runs axe-core with the
 * WCAG 2.0 A, WCAG 2.0 AA, and WCAG 2.2 AA tag sets. Asserts zero
 * violations per page.
 *
 * Discovered violations during initial run are documented in
 * `docs/accessibility-audit.md` and filed as `type:bug` GH issues.
 *
 * Entity-detail routes (/title/:id, /borrower/:id, /contributor/:id,
 * /series/:id) are excluded — they require fixture plumbing
 * (creating IDs via API or DB seed before each test). Tracked as a
 * follow-up.
 *
 * Spec ID "AX" — no ISBNs generated.
 */
import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { loginAs } from "../helpers/auth";

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

      // Build a readable summary of violations for the failure message.
      // axe's default error format is verbose; this summarises the
      // rule + first failing target so the CI log is actionable.
      const summary = results.violations.map((v) => ({
        rule: v.id,
        impact: v.impact,
        targets: v.nodes.slice(0, 3).map((n) => n.target.join(" ")),
      }));

      expect(
        results.violations,
        `${label}: ${results.violations.length} WCAG violation(s):\n${JSON.stringify(summary, null, 2)}`,
      ).toEqual([]);
    });
  }
});
