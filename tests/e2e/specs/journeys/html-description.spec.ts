import { test, expect } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";

// Story 13-4 (#578) — the mock Google Books answers this ISBN with an HTML
// description (BnF is told to miss it, so the chain falls through to Google
// Books, which returns HTML like the real API).
const HTML_DESCRIPTION_ISBN = specIsbn("HD", 1);

test.describe("Descriptions as plain text (story 13-4, #578)", () => {
  test("an HTML description from a provider reads as plain paragraphs", async ({
    page,
  }) => {
    // Blank context, real login (no cookie injection).
    await loginAs(page);

    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(HTML_DESCRIPTION_ISBN);
    await scanField.press("Enter");
    await expect(
      page
        .locator("#feedback-list .feedback-skeleton, #feedback-list .feedback-entry")
        .first(),
    ).toBeVisible({ timeout: 5000 });

    // The metadata lands in the background (see provider-chain.spec.ts,
    // #467): re-query until the title carries its fetched name.
    const titleLink = page.locator(
      "#browse-results table.browse-table tbody tr td a[href^='/title/']",
      { hasText: "HTML Description Book" },
    );
    await expect(async () => {
      await page.goto(`/?q=${HTML_DESCRIPTION_ISBN}`);
      await expect(titleLink.first()).toBeVisible({ timeout: 2000 });
    }).toPass({ timeout: 30000 });

    await titleLink.first().click();
    await expect(page).toHaveURL(/\/title\/\d+/);

    const description = page.locator("#title-description");
    const paragraphs = description.locator("p");
    await expect(paragraphs).toHaveCount(2);
    await expect(paragraphs.nth(0)).toHaveText("Grandis un peu, Titeuf !");
    await expect(paragraphs.nth(1)).toHaveText("La vie de Titeuf évolue & change.");
    // No markup or entity shows through as text.
    await expect(description).not.toContainText("<p>");
    await expect(description).not.toContainText("<b>");
    await expect(description).not.toContainText("&eacute;");
    await expect(description).not.toContainText("&amp;");
  });
});
