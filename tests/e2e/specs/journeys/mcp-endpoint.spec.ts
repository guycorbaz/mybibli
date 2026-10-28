/**
 * Story 11-6 E2E — the MCP endpoint `/api/mcp` (FR127, NFR42, NFR44).
 *
 * Real journey from a blank browser: loginAs(admin) → scan an ISBN in
 * /catalog (creates a title) → Admin → API keys → create a `propose` key
 * → an MCP client session over plain HTTP: `initialize` (2025-11-25) →
 * `tools/list` lists `get_title` → `tools/call get_title` returns the
 * scanned title; a session-less 2026-07-28 call works too → a `read` key
 * is refused (403) → both keys are revoked.
 *
 * Spec ID "MP" for ISBNs; unique key labels; no shared-DB assertion
 * beyond the rows this test creates.
 */
import { test, expect, request as playwrightRequest, APIRequestContext } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";

const MCP_HEADERS = {
  "Content-Type": "application/json",
  Accept: "application/json, text/event-stream",
};

async function mcpPost(
  request: APIRequestContext,
  key: string,
  body: object,
  extra: Record<string, string> = {},
) {
  return request.post("/api/mcp", {
    headers: { ...MCP_HEADERS, Authorization: `Bearer ${key}`, ...extra },
    data: body,
  });
}

test.describe("Story 11-6 — MCP endpoint", () => {
  test("an agent with a propose key connects and reads a title", async ({ page, baseURL }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    // A title to read, created through the real scan flow.
    const isbn = specIsbn("MP", 1);
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(isbn);
    await scanField.press("Enter");
    const titleId = Number(await titleIdFromSkeleton(page));
    expect(titleId).toBeGreaterThan(0);

    const propose = await createApiKey(page, "propose", "mcp");
    const read = await createApiKey(page, "read", "mcp-ro");

    // The agent is a separate client: a fresh request context without the
    // admin's cookies, like Claude Code.
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {

      // 2025-11-25: initialize, then stateless calls (no session id).
      const init = await mcpPost(agent, propose.plaintext, {
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: {
          protocolVersion: "2025-11-25",
          capabilities: {},
          clientInfo: { name: "mybibli-e2e", version: "1.0.0" },
        },
      });
      expect(init.status()).toBe(200);
      const initBody = await init.json();
      expect(initBody.result.protocolVersion).toBe("2025-11-25");
      expect(initBody.result.serverInfo.name).toBe("mybibli");
      expect(init.headers()["mcp-session-id"]).toBeUndefined();

      const legacy = { "MCP-Protocol-Version": "2025-11-25" };
      const list = await mcpPost(
        agent,
        propose.plaintext,
        { jsonrpc: "2.0", id: 2, method: "tools/list" },
        legacy,
      );
      expect(list.status()).toBe(200);
      const names = (await list.json()).result.tools.map((t: { name: string }) => t.name);
      expect(names).toContain("get_title");

      const call = await mcpPost(
        agent,
        propose.plaintext,
        {
          jsonrpc: "2.0",
          id: 3,
          method: "tools/call",
          params: { name: "get_title", arguments: { id: titleId } },
        },
        legacy,
      );
      expect(call.status()).toBe(200);
      const callBody = await call.json();
      expect(callBody.result.isError).toBe(false);
      expect(callBody.result.structuredContent.id).toBe(titleId);
      expect(callBody.result.structuredContent.isbn).toBe(isbn);

      // 2026-07-28: one session-less call, no initialize.
      const modern = await mcpPost(
        agent,
        propose.plaintext,
        {
          jsonrpc: "2.0",
          id: 4,
          method: "tools/call",
          params: {
            name: "get_title",
            arguments: { id: titleId },
            _meta: {
              "io.modelcontextprotocol/protocolVersion": "2026-07-28",
              "io.modelcontextprotocol/clientInfo": { name: "mybibli-e2e", version: "1.0.0" },
              "io.modelcontextprotocol/clientCapabilities": {},
            },
          },
        },
        { "MCP-Protocol-Version": "2026-07-28", "Mcp-Method": "tools/call", "Mcp-Name": "get_title" },
      );
      expect(modern.status()).toBe(200);
      expect((await modern.json()).result.structuredContent.isbn).toBe(isbn);

      // A read key never reaches the agent surface (NFR44).
      const refused = await mcpPost(agent, read.plaintext, {
        jsonrpc: "2.0",
        id: 5,
        method: "tools/list",
      });
      expect(refused.status()).toBe(403);
      expect((await refused.json()).scope_required).toBe("propose");
    } finally {
      // Clean up even on failure: no active key left behind.
      await page.goto("/admin?tab=api_keys");
      await revokeApiKey(page, propose.label);
      await revokeApiKey(page, read.label);
    }
    const afterRevoke = await mcpPost(agent, propose.plaintext, {
      jsonrpc: "2.0",
      id: 6,
      method: "tools/list",
    });
    expect(afterRevoke.status()).toBe(401);
    await agent.dispose();
  });
});
