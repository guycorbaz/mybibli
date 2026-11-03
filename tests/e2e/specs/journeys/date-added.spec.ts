/**
 * Story 13-7 E2E (#586) — the date a title was added: pages, browse and
 * the agent.
 *
 * Real journey from a blank browser: loginAs(admin) → scan an ISBN in
 * /catalog (creates a title) → the title page shows "Added on" →
 * the home browse "added since <today>" form lists the title (sorted by
 * date added, newest first) → as a separate MCP agent client:
 * `search_titles` with `added_since` today returns it, with tomorrow
 * does not, and an unreadable date is a tool error.
 *
 * Spec ID "DA" for ISBNs; unique key label. Dates are UTC days, like the
 * database's timestamps.
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { mcpCallTool } from "../../helpers/mcp";

function utcDay(offsetDays: number): string {
  const d = new Date();
  d.setUTCDate(d.getUTCDate() + offsetDays);
  return d.toISOString().slice(0, 10);
}

test.describe("Story 13-7 — date added", () => {
  test("a new title is dated, found by the browse filter and by the agent", async ({
    page,
    baseURL,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    const isbn = specIsbn("DA", 1);
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(isbn);
    await scanField.press("Enter");
    const titleId = Number(await titleIdFromSkeleton(page));
    expect(titleId).toBeGreaterThan(0);
    const today = utcDay(0);

    // AC1 — the title page says when the title was added and changed.
    await page.goto(`/title/${titleId}`);
    const dates = page.locator("#record-dates");
    await expect(dates).toContainText(/Added on|Ajouté le/i);
    await expect(dates).toContainText(/Modified on|Modifié le/i);

    // AC3 — the browse "added since" form keeps the title. Apply the
    // date, then sort by date added (newest first) so the title sits on
    // the first page whatever else the suite created today.
    await page.goto("/");
    await page.locator("#browse-added-since").fill(today);
    await page
      .locator("#browse-added-since-form")
      .getByRole("button", { name: /Apply|Appliquer/i })
      .click();
    await expect(page).toHaveURL(new RegExp(`added_since=${today}`));
    await expect(page.locator("#browse-added-since")).toHaveValue(today);
    await expect(page.locator("#browse-added-since-clear")).toBeVisible();
    await page
      .locator("#browse-results thead a", { hasText: /Added|Ajouté/i })
      .click();
    await expect(page).toHaveURL(/sort=date_added/);
    await expect(page).toHaveURL(new RegExp(`added_since=${today}`));
    await expect(
      page.locator(`#browse-results a[href="/title/${titleId}"]`).first(),
    ).toBeAttached();

    // AC5 — the agent filters on the date too.
    const propose = await createApiKey(page, "propose", "date-added");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      const key = propose.plaintext;
      const found = await mcpCallTool(agent, key, "search_titles", {
        query: isbn,
        added_since: today,
      });
      expect(found.isError).toBe(false);
      const items = found.structuredContent.items;
      expect(items.map((i: { id: number }) => i.id)).toEqual([titleId]);
      expect(items[0].created_at).toMatch(new RegExp(`^${today}T\\d{2}:\\d{2}:\\d{2}Z$`));

      const later = await mcpCallTool(agent, key, "search_titles", {
        query: isbn,
        added_since: utcDay(1),
      });
      expect(later.isError).toBe(false);
      expect(later.structuredContent.items).toEqual([]);

      const doc = await mcpCallTool(agent, key, "get_title", { id: titleId });
      expect(doc.isError).toBe(false);
      expect(doc.structuredContent.created_at).toMatch(new RegExp(`^${today}T`));
      expect(typeof doc.structuredContent.updated_at).toBe("string");

      const bad = await mcpCallTool(agent, key, "search_titles", {
        added_since: "last Saturday",
      });
      expect(bad.isError).toBe(true);
      expect(bad.content[0].text).toContain("YYYY-MM-DD");
    } finally {
      await agent.dispose();
      await page.goto("/admin?tab=api_keys");
      await revokeApiKey(page, propose.label);
    }
  });
});
