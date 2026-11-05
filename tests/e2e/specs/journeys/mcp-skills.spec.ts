import { test, expect, Page, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { MCP_TOOLS, mcpCallTool, mcpPost } from "../../helpers/mcp";

/**
 * Story 15-3 (#614, FR163) — the library's skills served to the agent.
 *
 * Journey from a blank browser: an admin writes a skill in Admin → Skills
 * and creates a `propose` key; as a separate agent client, the
 * `initialize` instructions list the skill, `prompts/list` lists it,
 * `prompts/get` returns its body as one user message, and the read-only
 * `get_skill` tool reads it. The admin deactivates it: the agent can no
 * longer read it. The key is revoked.
 *
 * specId "SS": the skill name is `0ss-<random>` — unique across workers
 * and runs, and sorted before the `sk-` skills of admin-skills.spec.ts so
 * it stays within the instructions' bounded list.
 */

const SKILLS_TAB = /^(Skills|Compétences|Skill)$/i;
const INACTIVE = /^(Inactive|Inaktiv|Inattivo)$/i;
const LEGACY = { "MCP-Protocol-Version": "2025-11-25" };

async function openSkillsTab(page: Page) {
  await page.goto("/admin", { waitUntil: "domcontentloaded" });
  await page.getByRole("tab", { name: SKILLS_TAB }).click();
  await expect(page.locator("#admin-skills-editor")).toBeVisible({ timeout: 10000 });
}

const rowOf = (page: Page, name: string) =>
  page.locator("#admin-skills-list tbody tr").filter({
    has: page.getByText(name, { exact: true }),
  });

test.describe("Story 15-3 — MCP skills served to the agent", () => {
  test("an admin writes a skill; the agent sees it as a prompt and reads it with get_skill", async ({
    page,
    baseURL,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");
    await openSkillsTab(page);

    const name = `0ss-${crypto.randomUUID().slice(0, 8)}`;
    const description = `Series conventions ${name}`;
    const body = `# Series\n\nAttach tomes in order (${name}).\n<b>kept as typed</b>`;
    const editor = page.locator("#admin-skills-editor");
    await editor.locator("#admin-skills-name").fill(name);
    await editor.locator("#admin-skills-description").fill(description);
    await editor.locator("#admin-skills-body").fill(body);
    await editor.locator("#admin-skills-submit").click();
    await expect(rowOf(page, name)).toBeVisible({ timeout: 10000 });

    const propose = await createApiKey(page, "propose", "mcp-skills");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      const key = propose.plaintext;

      // initialize: the prompts capability and the "Library skills" section.
      const init = await mcpPost(agent, key, {
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: {
          protocolVersion: "2025-11-25",
          capabilities: {},
          clientInfo: { name: "e2e-skills", version: "0" },
        },
      });
      expect(init.status()).toBe(200);
      const result = (await init.json()).result;
      expect(result.capabilities.prompts).toBeDefined();
      expect(result.instructions).toContain("Library skills:");
      expect(result.instructions).toContain(`\n- ${name}: ${description}`);

      // prompts/list → the skill, no arguments.
      const list = await mcpPost(
        agent,
        key,
        { jsonrpc: "2.0", id: 2, method: "prompts/list" },
        LEGACY,
      );
      expect(list.status()).toBe(200);
      const prompts: { name: string; description: string; arguments?: unknown }[] = (
        await list.json()
      ).result.prompts;
      const prompt = prompts.find((p) => p.name === name);
      expect(prompt).toEqual({ name, description });

      // prompts/get → the body as one user message.
      const got = await mcpPost(
        agent,
        key,
        { jsonrpc: "2.0", id: 3, method: "prompts/get", params: { name } },
        LEGACY,
      );
      expect(got.status()).toBe(200);
      expect((await got.json()).result.messages).toEqual([
        { role: "user", content: { type: "text", text: body } },
      ]);

      // get_skill is a served tool and reads the same skill.
      const tools = await mcpPost(
        agent,
        key,
        { jsonrpc: "2.0", id: 4, method: "tools/list" },
        LEGACY,
      );
      const names = (await tools.json()).result.tools.map((t: { name: string }) => t.name).sort();
      expect(names).toEqual(MCP_TOOLS);
      const read = await mcpCallTool(agent, key, "get_skill", { name });
      expect(read.isError).toBe(false);
      expect(read.structuredContent.body).toBe(body);
      expect(read.structuredContent.description).toBe(description);
      expect(read.structuredContent.updated_at).toMatch(/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$/);

      // Deactivated in the tab: the agent can no longer read it.
      await openSkillsTab(page);
      await rowOf(page, name).locator('button[id^="admin-skill-toggle-"]').click();
      await expect(rowOf(page, name).getByText(INACTIVE)).toBeVisible({ timeout: 10000 });
      const gone = await mcpCallTool(agent, key, "get_skill", { name });
      expect(gone.isError).toBe(true);
      expect(gone.content[0].text).toContain(`No active skill is named "${name}"`);
      const refused = await mcpPost(
        agent,
        key,
        { jsonrpc: "2.0", id: 5, method: "prompts/get", params: { name } },
        LEGACY,
      );
      const refusedBody = await refused.json();
      expect(refusedBody.error.code).toBe(-32602);
      expect(JSON.stringify(refusedBody)).not.toContain("Attach tomes in order");
    } finally {
      await agent.dispose();
      await page.goto("/admin?tab=api_keys");
      await revokeApiKey(page, propose.label);
    }
  });
});
