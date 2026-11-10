import { test, expect, Page, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { checkAccessibility } from "../../helpers/accessibility";
import { mcpCallTool, mcpPost } from "../../helpers/mcp";

/**
 * Story 17-6 (#656, FR177, UX-DR44) — the MCP skills mybibli ships with.
 *
 * Journey from a blank browser: on the stack's fresh database, startup
 * inserted the four comic skills. An admin finds them in Admin → Skills,
 * inactive, marked "provided by mybibli", their text readable; as a
 * separate agent client, none of them is served (`prompts/list`,
 * `get_skill`). The key is revoked.
 *
 * specId "DS". Read-only on the default rows (Rule #23): the spec never
 * activates, edits or deletes them — other specs may read the same rows.
 * Name taken, deleted then restarted: tests/default_skills.rs (DB).
 */

const SKILLS_TAB = /^(Skills|Compétences|Skill)$/i;
const INACTIVE = /^(Inactive|Inaktiv|Inattivo)$/i;
const PROVIDED = /^(provided by mybibli|fourni par mybibli|von mybibli bereitgestellt|fornito da mybibli)$/;
const LEGACY = { "MCP-Protocol-Version": "2025-11-25" };
const DEFAULTS = [
  "comics-covers",
  "comics-series-attach",
  "comics-series-create",
  "comics-title-spelling",
];

async function openSkillsTab(page: Page) {
  await page.goto("/admin", { waitUntil: "domcontentloaded" });
  await page.getByRole("tab", { name: SKILLS_TAB }).click();
  await expect(page.locator("#admin-skills-editor")).toBeVisible({ timeout: 10000 });
}

const rowOf = (page: Page, name: string) =>
  page.locator("#admin-skills-list tbody tr").filter({
    has: page.getByText(name, { exact: true }),
  });

test.describe("Story 17-6 — default MCP skills", () => {
  test("the four comic skills ship inactive, marked as provided by mybibli, and are not served", async ({
    page,
    baseURL,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");
    await openSkillsTab(page);
    await checkAccessibility(page);

    for (const name of DEFAULTS) {
      const row = rowOf(page, name);
      await expect(row).toHaveCount(1);
      await expect(row.getByText(INACTIVE)).toBeVisible();
      await expect(row.getByText(PROVIDED)).toBeVisible();
    }

    // The text is readable as a model for the admin's own skills.
    const covers = rowOf(page, "comics-covers");
    await covers.locator("summary").click();
    await expect(covers.locator("pre")).toContainText("## Ground rules");
    await expect(covers.locator("pre")).toContainText("list_titles_without_cover");

    let propose: Awaited<ReturnType<typeof createApiKey>> | undefined;
    let agent: Awaited<ReturnType<typeof playwrightRequest.newContext>> | undefined;
    try {
      propose = await createApiKey(page, "propose", "default-skills");
      expect(baseURL, "baseURL must be configured").toBeTruthy();
      agent = await playwrightRequest.newContext({ baseURL: baseURL! });
      const key = propose.plaintext;

      const list = await mcpPost(
        agent,
        key,
        { jsonrpc: "2.0", id: 1, method: "prompts/list" },
        LEGACY,
      );
      expect(list.status()).toBe(200);
      const served: string[] = (await list.json()).result.prompts.map(
        (p: { name: string }) => p.name,
      );
      for (const name of DEFAULTS) {
        expect(served).not.toContain(name);
      }

      const read = await mcpCallTool(agent, key, "get_skill", { name: "comics-covers" });
      expect(read.isError).toBe(true);
      expect(read.content[0].text).toContain('No active skill is named "comics-covers"');
    } finally {
      try {
        await agent?.dispose();
      } catch (e) {
        console.warn("default-skills cleanup step failed:", e);
      }
      if (propose) {
        const label = propose.label;
        try {
          await page.goto("/admin?tab=api_keys");
          await revokeApiKey(page, label);
        } catch (e) {
          console.warn("default-skills cleanup step failed:", e);
        }
      }
    }
  });
});
