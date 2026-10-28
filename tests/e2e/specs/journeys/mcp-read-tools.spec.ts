/**
 * Story 11-7 E2E — the MCP catalog read tools (FR128).
 *
 * Real journey from a blank browser: loginAs(admin) → scan an ISBN in
 * /catalog (creates a title; the mock provider adds a synthetic author)
 * → Admin → API keys → create a `propose` key → as a separate agent
 * client: `tools/list` lists the read tools → `search_titles`
 * finds the scanned ISBN → `get_title` returns the enriched record →
 * `search_contributors` finds its author → an invalid argument is a
 * tool error → the key is revoked.
 *
 * Spec ID "MR" for ISBNs; unique key label; assertions only on rows
 * this test creates (the synthetic author is shared, so it is matched
 * by id, not by count).
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { MCP_TOOLS, mcpCallTool, mcpPost } from "../../helpers/mcp";

test.describe("Story 11-7 — MCP catalog read tools", () => {
  test("an agent searches and reads the catalog", async ({ page, baseURL }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    const isbn = specIsbn("MR", 1);
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(isbn);
    await scanField.press("Enter");
    const titleId = Number(await titleIdFromSkeleton(page));
    expect(titleId).toBeGreaterThan(0);

    const propose = await createApiKey(page, "propose", "mcp-read");
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

      // search_titles: exact ISBN lookup.
      const found = await mcpCallTool(agent, key, "search_titles", { query: isbn });
      expect(found.isError).toBe(false);
      expect(found.structuredContent.items.map((i: { id: number }) => i.id)).toEqual([titleId]);
      expect(found.structuredContent.page_size).toBe(25);

      // get_title: enriched record. The author arrives with the async
      // metadata fetch — poll until it is there.
      let author: { id: number; name: string } | undefined;
      await expect
        .poll(
          async () => {
            const res = await mcpCallTool(agent, key, "get_title", { id: titleId });
            expect(res.isError).toBe(false);
            const doc = res.structuredContent;
            expect(doc.isbn).toBe(isbn);
            expect(typeof doc.has_cover).toBe("boolean");
            expect(Array.isArray(doc.manually_edited_fields)).toBe(true);
            author = doc.contributors[0];
            return doc.contributors.length;
          },
          { timeout: 15000 },
        )
        .toBeGreaterThan(0);

      // search_contributors: the author is listed with at least this title.
      await expect
        .poll(async () => {
          for (let p = 1; p <= 10; p++) {
            const res = await mcpCallTool(agent, key, "search_contributors", {
              query: author!.name,
              page: p,
            });
            expect(res.isError).toBe(false);
            const hit = res.structuredContent.items.find((c: { id: number }) => c.id === author!.id);
            if (hit) return hit.title_count;
            if (p >= res.structuredContent.total_pages) break;
          }
          return 0;
        })
        .toBeGreaterThan(0);

      // A bad argument is a tool error the agent can read, not a failure.
      const bad = await mcpCallTool(agent, key, "search_titles", { media_type: "vinyl" });
      expect(bad.isError).toBe(true);
      expect(bad.content[0].text).toContain("book, bd, cd, dvd, magazine, report");
    } finally {
      await agent.dispose();
      await page.goto("/admin?tab=api_keys");
      await revokeApiKey(page, propose.label);
    }
  });
});
