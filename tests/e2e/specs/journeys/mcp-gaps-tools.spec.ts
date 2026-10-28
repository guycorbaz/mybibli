/**
 * Story 11-8 E2E — the MCP series and gaps read tools (FR129).
 *
 * Real journey from a blank browser: loginAs(admin) → create a closed
 * series of 3 → scan an ISBN in /catalog and assign the title at
 * position 2 → Admin → API keys → create a `propose` key → as a
 * separate agent client: `tools/list` lists the seven read tools →
 * `get_series` reports positions 1 and 3 missing → `list_series` with
 * `gaps_only` lists the series → the two title lists answer → the key
 * is revoked.
 *
 * Spec ID "MG" for ISBNs; unique series name and key label; assertions
 * only on rows this test creates.
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { MCP_TOOLS, mcpCallTool, mcpPost } from "../../helpers/mcp";

test.describe("Story 11-8 — MCP series and gaps read tools", () => {
  test("an agent reads a series' gaps and the incomplete titles", async ({ page, baseURL }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    // A closed series of 3 tomes.
    const seriesName = `MG-Series-${Date.now()}`;
    await page.goto("/series/new");
    await page.locator("#series-name").fill(seriesName);
    await page.locator("#series-type").selectOption("closed");
    await page.locator("#series-total").fill("3");
    await page.locator('main button[type="submit"]').last().click();
    await page.waitForURL(/\/series\/\d+$/);
    const seriesId = Number(page.url().match(/\/series\/(\d+)$/)![1]);

    // Scan a title and put it at position 2.
    const isbn = specIsbn("MG", 1);
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(isbn);
    await scanField.press("Enter");
    const titleId = Number(await titleIdFromSkeleton(page));
    expect(titleId).toBeGreaterThan(0);
    await page.goto(`/title/${titleId}`);
    await page.locator("#assign-series").selectOption({ label: seriesName });
    await page.locator("#assign-position").fill("2");
    await page.locator("#assign-series-submit").click();
    await page.waitForURL(/\/title\/\d+/);

    const propose = await createApiKey(page, "propose", "mcp-gaps");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      const key = propose.plaintext;

      const list = await mcpPost(
        agent,
        key,
        { jsonrpc: "2.0", id: 1, method: "tools/list" },
        { "MCP-Protocol-Version": "2025-11-25" },
      );
      expect(list.status()).toBe(200);
      const names = (await list.json()).result.tools.map((t: { name: string }) => t.name).sort();
      expect(names).toEqual(MCP_TOOLS);

      // get_series: the title holds position 2; 1 and 3 are missing.
      await expect
        .poll(async () => {
          const res = await mcpCallTool(agent, key, "get_series", { id: seriesId });
          expect(res.isError).toBe(false);
          return res.structuredContent.positions.map((p: { title_id: number }) => p.title_id);
        })
        .toEqual([titleId]);
      const series = (await mcpCallTool(agent, key, "get_series", { id: seriesId }))
        .structuredContent;
      expect(series.name).toBe(seriesName);
      expect(series.series_type).toBe("closed");
      expect(series.positions[0].position).toBe(2);
      expect(series.gap_count).toBe(2);
      expect(series.missing_positions).toEqual([1, 3]);

      // list_series: the series is among the ones with gaps.
      const gaps = await mcpCallTool(agent, key, "list_series", {
        query: seriesName,
        gaps_only: true,
      });
      expect(gaps.isError).toBe(false);
      expect(gaps.structuredContent.items).toEqual([
        {
          id: seriesId,
          name: seriesName,
          series_type: "closed",
          total_volume_count: 3,
          owned_positions: 1,
          gap_count: 2,
        },
      ]);

      // The two title lists answer with the paged shape.
      for (const tool of ["list_titles_without_cover", "list_titles_with_incomplete_metadata"]) {
        const res = await mcpCallTool(agent, key, tool, { media_type: "book" });
        expect(res.isError, tool).toBe(false);
        expect(res.structuredContent.page_size, tool).toBe(25);
        expect(Array.isArray(res.structuredContent.items), tool).toBe(true);
      }

      // A bad argument is a tool error the agent can read.
      const bad = await mcpCallTool(agent, key, "list_titles_with_incomplete_metadata", {
        missing: "isbn",
      });
      expect(bad.isError).toBe(true);
      expect(bad.content[0].text).toContain("description, publisher, contributor, publication_year");
    } finally {
      await agent.dispose();
      await page.goto("/admin?tab=api_keys");
      await revokeApiKey(page, propose.label);
    }
  });
});
