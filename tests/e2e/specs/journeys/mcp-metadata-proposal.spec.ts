/**
 * Story 11-13 E2E — the MCP metadata proposal tool (FR132).
 *
 * Real journey from a blank browser: loginAs(admin) → scan an ISBN in
 * /catalog (the mock provider fills its publisher) → Admin → API keys →
 * create a `propose` key → as a separate agent client: `tools/list`
 * lists `propose_title_metadata` → `get_title` gives the version →
 * `propose_title_metadata` files a pending proposal and reports the
 * publisher as unchanged → invalid fields are refused field by field →
 * the title page still shows none of the proposed values → the key is
 * revoked.
 *
 * Spec ID "MD" for ISBNs; unique key label; assertions only on rows this
 * test creates. Creates no series, so it does not depend on file order
 * (#528).
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { MCP_TOOLS, mcpCallTool, mcpPost } from "../../helpers/mcp";

const SOURCE = "https://example.org/notice-bibliographique";
const PROPOSED_DEWEY = "843.914";

test.describe("Story 11-13 — MCP metadata proposal tool", () => {
  test("an agent proposes a title's missing information", async ({ page, baseURL }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(specIsbn("MD", 1));
    await scanField.press("Enter");
    const titleId = Number(await titleIdFromSkeleton(page));
    expect(titleId).toBeGreaterThan(0);

    const propose = await createApiKey(page, "propose", "mcp-metadata-proposal");
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
      const tools = (await list.json()).result.tools;
      expect(tools.map((t: { name: string }) => t.name).sort()).toEqual(MCP_TOOLS);
      const tool = tools.find((t: { name: string }) => t.name === "propose_title_metadata");
      expect(tool.annotations.readOnlyHint).toBe(false);

      // Wait for the background metadata fetch (it bumps the version).
      let title: { publisher: string | null; version: number } = { publisher: null, version: 0 };
      await expect
        .poll(
          async () => {
            title = (await mcpCallTool(agent, key, "get_title", { id: titleId })).structuredContent;
            return title.publisher;
          },
          { timeout: 15000 },
        )
        .toBe("Test Publisher");

      const filed = await mcpCallTool(agent, key, "propose_title_metadata", {
        title_id: titleId,
        version: title.version,
        fields: {
          publisher: "Test Publisher",
          dewey_code: PROPOSED_DEWEY,
          publication_year: 1999,
          contributors: [{ name: "MD Contributeur", role: "Auteur" }],
        },
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      expect(filed.structuredContent.status).toBe("pending");
      expect(filed.structuredContent.kind).toBe("metadata");
      expect(filed.structuredContent.item_count).toBe(3);
      expect(filed.structuredContent.unchanged_fields).toEqual(["publisher"]);

      // Invalid fields: every one named, nothing filed.
      const refused = await mcpCallTool(agent, key, "propose_title_metadata", {
        title_id: titleId,
        fields: { isbn: "9782070360246", dewey_code: "abc", subtitle: "" },
        sources: [SOURCE],
      });
      expect(refused.isError).toBe(true);
      const text: string = refused.content[0].text;
      expect(text).toContain("3 fields were refused, nothing was filed");
      expect(text).toContain("`isbn` cannot be proposed");
      expect(text).toContain("`dewey_code` must be a Dewey class");
      expect(text).toContain("`subtitle` is empty");
    } finally {
      await agent.dispose();
    }

    // Nothing was written to the catalog.
    await page.goto(`/title/${titleId}`);
    await expect(page.locator("h1")).toBeVisible();
    await expect(page.getByText(PROPOSED_DEWEY)).toHaveCount(0);
    await expect(page.getByText("MD Contributeur")).toHaveCount(0);

    await page.goto("/admin?tab=api_keys");
    await revokeApiKey(page, propose.label);
  });
});
