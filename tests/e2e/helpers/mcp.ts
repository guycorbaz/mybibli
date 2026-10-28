/**
 * MCP client helpers for the `/api/mcp` specs (story 11-7, extracted
 * from the 11-6 spec). Calls go through a request context of the
 * caller's choice — use a fresh one (no admin cookies) to play the agent.
 */
import { APIRequestContext, expect } from "@playwright/test";

export const MCP_HEADERS = {
  "Content-Type": "application/json",
  Accept: "application/json, text/event-stream",
};

/** Every tool `/api/mcp` serves, sorted by name (stories 11-6..11-8, 11-12). */
export const MCP_TOOLS = [
  "get_series",
  "get_title",
  "list_series",
  "list_titles_with_incomplete_metadata",
  "list_titles_without_cover",
  "propose_series_attach",
  "propose_series_create",
  "search_contributors",
  "search_titles",
];

/** POST one JSON-RPC message to `/api/mcp` with a bearer key. */
export async function mcpPost(
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

/**
 * `tools/call` over the 2025-11-25 revision (stateless, header only);
 * returns the JSON-RPC `result` (`isError`, `structuredContent`, …).
 */
export async function mcpCallTool(
  request: APIRequestContext,
  key: string,
  name: string,
  args: object,
) {
  const res = await mcpPost(
    request,
    key,
    { jsonrpc: "2.0", id: 1, method: "tools/call", params: { name, arguments: args } },
    { "MCP-Protocol-Version": "2025-11-25" },
  );
  expect(res.status()).toBe(200);
  const body = await res.json();
  expect(body.error, JSON.stringify(body)).toBeUndefined();
  return body.result;
}
