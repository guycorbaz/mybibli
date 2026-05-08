/**
 * Story 9-15 — StatusMessage empty-state E2E.
 *
 * Covers AC10 — 4 scenarios:
 *   1. anonymous on /series sees empty state WITHOUT CTA (public read)
 *   2. librarian on /series sees empty state WITH CTA → /series/new
 *   3. search-no-results role-aware (anonymous vs librarian)
 *   4. i18n round-trip (EN + FR via lang cookie)
 *
 * Spec ID "ES" — no ISBNs generated (this spec creates no catalog rows).
 *
 * NB on isolation: tests run against the e2e Docker stack which is reset
 * via `./scripts/e2e-reset.sh` before this spec. The /series list is
 * empty when the seeded DB has no series rows. If a sibling spec (e.g.
 * series.spec.ts) runs in parallel and creates series rows, this spec's
 * "empty list" assertion may flake. Reset before running locally.
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

test.describe("Story 9-15 — empty-state component", () => {
  test("anonymous on /series sees empty state WITHOUT CTA", async ({
    page,
  }) => {
    await page.context().clearCookies();
    await page.goto("/series");

    // Component stable selectors (per AC1 — data-attributes).
    const emptyState = page.locator("[data-status-message][data-variant='empty']");
    await expect(emptyState).toBeVisible();
    // Heading + body present
    await expect(emptyState.locator("h2")).toContainText(/No series|Aucune série/i);
    // No CTA for anonymous (librarian-gated)
    await expect(emptyState.locator("a")).toHaveCount(0);
  });

  test("librarian on /series sees empty state WITH CTA → /series/new", async ({
    page,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto("/series");

    const emptyState = page.locator("[data-status-message][data-variant='empty']");
    await expect(emptyState).toBeVisible();
    // CTA visible for librarian
    const cta = emptyState.locator("a");
    await expect(cta).toBeVisible();
    await expect(cta).toHaveAttribute("href", "/series/new");
    await expect(cta).toContainText(/Create a series|Créer une série/i);

    // Click → navigate to /series/new
    await cta.click();
    await expect(page).toHaveURL(/\/series\/new$/);
  });

  test("search-no-results role-aware CTA", async ({ page }) => {
    // Librarian sees "Add this title" CTA.
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto("/?q=ZZZZNonExistentTitleQuery9999");

    const librarianEmpty = page.locator("[data-status-message][data-variant='empty']");
    await expect(librarianEmpty).toBeVisible();
    const librarianCta = librarianEmpty.locator("a");
    await expect(librarianCta).toBeVisible();
    await expect(librarianCta).toHaveAttribute(
      "href",
      /\/catalog\/title\/new\?title=ZZZZNonExistentTitleQuery9999/,
    );

    // Anonymous sees the empty state but NO CTA.
    await page.context().clearCookies();
    await page.goto("/?q=ZZZZNonExistentTitleQuery9999");

    const anonEmpty = page.locator("[data-status-message][data-variant='empty']");
    await expect(anonEmpty).toBeVisible();
    await expect(anonEmpty.locator("a")).toHaveCount(0);
  });

  test("i18n round-trip — FR locale renders FR copy", async ({ page }) => {
    // Set lang=fr cookie BEFORE navigation so the locale middleware picks it up.
    await page.context().clearCookies();
    await page.context().addCookies([
      {
        name: "lang",
        value: "fr",
        domain: "localhost",
        path: "/",
      },
    ]);
    await page.goto("/series");

    const emptyState = page.locator("[data-status-message][data-variant='empty']");
    await expect(emptyState).toBeVisible();
    // FR copy verbatim from locales/fr.yml
    await expect(emptyState.locator("h2")).toContainText(/Aucune série pour l'instant/i);
  });
});
