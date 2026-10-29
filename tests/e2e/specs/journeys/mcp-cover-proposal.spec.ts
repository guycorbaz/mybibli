/**
 * Story 11-14 E2E — the MCP cover proposal tool (FR133, NFR45, NFR48).
 *
 * Real journey from a blank browser: loginAs(admin) → scan an ISBN in
 * /catalog → Admin → API keys → create a `propose` key → as a separate
 * agent client: `tools/list` lists `propose_cover` → `get_title` gives
 * the version → `propose_cover` files a pending cover proposal → a
 * non-image is refused → the title is unchanged (same cover state, same
 * version) → the staging area is not served under /covers → the key is
 * revoked and the agent is turned away.
 *
 * Spec ID "PC" for ISBNs; unique key label; assertions only on rows this
 * test creates. Creates no series, so it does not depend on file order
 * (#528).
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { MCP_TOOLS, mcpCallTool, mcpPost } from "../../helpers/mcp";

const SOURCE = "https://example.org/couverture";
/** A valid 1×1 RGBA PNG. */
const PNG_1X1 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

test.describe("Story 11-14 — MCP cover proposal tool", () => {
  test("an agent proposes a cover without touching the catalog", async ({ page, baseURL }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(specIsbn("PC", 1));
    await scanField.press("Enter");
    const titleId = Number(await titleIdFromSkeleton(page));
    expect(titleId).toBeGreaterThan(0);

    const propose = await createApiKey(page, "propose", "mcp-cover-proposal");
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
      const tool = tools.find((t: { name: string }) => t.name === "propose_cover");
      expect(tool.annotations.readOnlyHint).toBe(false);
      expect(tool.inputSchema.required).toEqual(
        expect.arrayContaining(["title_id", "image_base64", "media_type", "sources"]),
      );

      // Wait for the background metadata fetch (it bumps the version).
      let title: { publisher: string | null; version: number; has_cover: boolean } = {
        publisher: null,
        version: 0,
        has_cover: false,
      };
      await expect
        .poll(
          async () => {
            title = (await mcpCallTool(agent, key, "get_title", { id: titleId })).structuredContent;
            return title.publisher;
          },
          { timeout: 15000 },
        )
        .toBe("Test Publisher");

      const filed = await mcpCallTool(agent, key, "propose_cover", {
        title_id: titleId,
        version: title.version,
        image_base64: PNG_1X1,
        media_type: "image/png",
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      expect(filed.structuredContent.status).toBe("pending");
      expect(filed.structuredContent.kind).toBe("cover");
      expect(filed.structuredContent.item_count).toBe(1);
      expect(filed.structuredContent.replaces_existing).toBe(title.has_cover);

      // Not an image: refused, nothing filed.
      const refused = await mcpCallTool(agent, key, "propose_cover", {
        title_id: titleId,
        image_base64: Buffer.from("definitely not an image").toString("base64"),
        media_type: "image/png",
        sources: [SOURCE],
      });
      expect(refused.isError).toBe(true);
      expect(refused.content[0].text).toContain("not a JPEG, PNG, WebP or GIF");

      // The catalog did not change.
      const after = (await mcpCallTool(agent, key, "get_title", { id: titleId })).structuredContent;
      expect(after.version).toBe(title.version);
      expect(after.has_cover).toBe(title.has_cover);
    } finally {
      await agent.dispose();
    }

    // The staging area is never served.
    const staging = await page.request.get("/covers/.staging/");
    expect(staging.status()).toBe(404);

    await page.goto(`/title/${titleId}`);
    await expect(page.locator("h1")).toBeVisible();
    await expect(page.locator('img[src*=".staging"]')).toHaveCount(0);

    await page.goto("/admin?tab=api_keys");
    await revokeApiKey(page, propose.label);

    const turnedAway = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      const res = await mcpPost(
        turnedAway,
        propose.plaintext,
        { jsonrpc: "2.0", id: 1, method: "tools/list" },
        { "MCP-Protocol-Version": "2025-11-25" },
      );
      expect(res.status()).toBe(401);
    } finally {
      await turnedAway.dispose();
    }
  });
});
