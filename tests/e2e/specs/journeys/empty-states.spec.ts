/**
 * Story 9-15 — StatusMessage empty-state E2E.
 *
 * The search no-results empty state, role-aware (anonymous vs librarian)
 * and in French through the `lang` cookie. The query is the spec's own
 * (no title can match it), so the case holds whatever other specs create
 * in parallel (Rule #23).
 *
 * The empty `/series` list (anonymous without CTA, librarian with the CTA
 * to `/series/new`, French and English copy) depends on a library with no
 * series at all, which a shared E2E database cannot promise: it is covered
 * on a fresh database by `tests/series_empty_state.rs` (story 19-2, #528).
 *
 * Spec ID "ES" — no ISBNs generated (this spec creates no catalog rows).
 */
import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";

const NO_MATCH_QUERY = "ZZZZNonExistentTitleQuery9999";

test.describe("Story 9-15 — empty-state component", () => {
  test("search-no-results role-aware CTA", async ({ page }) => {
    // Librarian sees "Add this title" CTA.
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto(`/?q=${NO_MATCH_QUERY}`);

    const librarianEmpty = page.locator("[data-status-message][data-variant='empty']");
    await expect(librarianEmpty).toBeVisible();
    const librarianCta = librarianEmpty.locator("a");
    await expect(librarianCta).toBeVisible();
    await expect(librarianCta).toHaveAttribute(
      "href",
      new RegExp(`/catalog/title/new\\?title=${NO_MATCH_QUERY}`),
    );

    // Anonymous sees the empty state but NO CTA.
    await page.context().clearCookies();
    await page.goto(`/?q=${NO_MATCH_QUERY}`);

    const anonEmpty = page.locator("[data-status-message][data-variant='empty']");
    await expect(anonEmpty).toBeVisible();
    await expect(anonEmpty.locator("a")).toHaveCount(0);
  });

  test("i18n — FR locale renders the FR search empty copy", async ({ page }) => {
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
    await page.goto(`/?q=${NO_MATCH_QUERY}`);

    const emptyState = page.locator("[data-status-message][data-variant='empty']");
    await expect(emptyState).toBeVisible();
    // FR copy verbatim from locales/fr.yml (`empty.search_heading`,
    // `empty.search_body`).
    await expect(emptyState.locator("h2")).toHaveText(/^\s*Aucun résultat\s*$/);
    await expect(emptyState).toContainText(`Aucun résultat pour « ${NO_MATCH_QUERY} »`);
  });
});
