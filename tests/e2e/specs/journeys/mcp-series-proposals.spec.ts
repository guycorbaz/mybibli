/**
 * Story 11-12 E2E — the MCP series proposal tools (FR130, FR131).
 *
 * Real journey from a blank browser: loginAs(admin) → scan an ISBN in
 * /catalog (a title the agent will place) → create a series in the web UI
 * (the one the agent attaches to) → Admin → API keys → create a `propose`
 * key → as a separate agent client: `tools/list` lists the nine tools →
 * `propose_series_create` files a pending proposal → the same proposal
 * again is refused with the pending id → a create for the existing
 * series' name is refused and points to `propose_series_attach` →
 * `get_series` gives the version → `propose_series_attach` files a
 * pending proposal → the key is revoked. Nothing in the catalog changes
 * (the series page still shows no expected tome).
 *
 * Spec ID "SQ" for ISBNs; unique series names and key label; assertions
 * only on rows this test creates. The spec sorts after the specs that
 * assume no series exists (D-11-11-6).
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { MCP_TOOLS, mcpCallTool, mcpPost } from "../../helpers/mcp";

const SOURCE = "https://example.org/series-catalogue";

test.describe("Story 11-12 — MCP series proposal tools", () => {
  test("an agent proposes a new series and titles for an existing one", async ({
    page,
    baseURL,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    // A title the agent will place in the proposed series.
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(specIsbn("SQ", 1));
    await scanField.press("Enter");
    const titleId = Number(await titleIdFromSkeleton(page));
    expect(titleId).toBeGreaterThan(0);

    // An existing closed series of 4.
    const stamp = Date.now();
    const existingName = `SQ-Existing-${stamp}`;
    await page.goto("/series/new");
    await page.locator("#series-name").fill(existingName);
    await page.locator("#series-type").selectOption("closed");
    await page.locator("#series-total").fill("4");
    await page.getByRole("button", { name: /^(Save|Enregistrer|Speichern|Salva)$/i }).click();
    await page.waitForURL(/\/series\/\d+$/);
    const seriesId = Number(page.url().match(/\/series\/(\d+)$/)![1]);

    const propose = await createApiKey(page, "propose", "mcp-series-proposals");
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
      const createTool = tools.find((t: { name: string }) => t.name === "propose_series_create");
      expect(createTool.annotations.readOnlyHint).toBe(false);

      // A new series with the scanned title and one expected tome.
      const newName = `SQ-New-${stamp}`;
      const createArgs = {
        name: newName,
        series_type: "closed",
        total_volume_count: 3,
        titles: [{ title_id: titleId, position: 1 }],
        expected_tomes: [{ position: 2, title: `${newName} tome 2` }],
        sources: [SOURCE],
      };
      const created = await mcpCallTool(agent, key, "propose_series_create", createArgs);
      expect(created.isError, JSON.stringify(created.content)).toBe(false);
      expect(created.structuredContent.status).toBe("pending");
      expect(created.structuredContent.kind).toBe("series_create");
      expect(created.structuredContent.item_count).toBe(3);
      const createdId = created.structuredContent.proposal_id;

      // The same proposal again: refused, with the pending proposal's id.
      const again = await mcpCallTool(agent, key, "propose_series_create", createArgs);
      expect(again.isError).toBe(true);
      expect(again.content[0].text).toContain(`Proposal ${createdId}`);

      // A series that already exists: refused, pointing to attach.
      const exists = await mcpCallTool(agent, key, "propose_series_create", {
        ...createArgs,
        name: existingName,
        titles: [],
      });
      expect(exists.isError).toBe(true);
      expect(exists.content[0].text).toContain(
        `propose_series_attach with \`series_id\` ${seriesId}`,
      );

      // Attach: the title at position 3 and an expected tome at 4,
      // pinned on the version get_series returned.
      const series = await mcpCallTool(agent, key, "get_series", { id: seriesId });
      expect(series.isError).toBe(false);
      const attached = await mcpCallTool(agent, key, "propose_series_attach", {
        series_id: seriesId,
        series_version: series.structuredContent.version,
        titles: [{ title_id: titleId, position: 3 }],
        expected_tomes: [{ position: 4, title: `${existingName} tome 4`, isbn: "2-205-00121-3" }],
        sources: [SOURCE],
      });
      expect(attached.isError, JSON.stringify(attached.content)).toBe(false);
      expect(attached.structuredContent.kind).toBe("series_attach");
      expect(attached.structuredContent.item_count).toBe(3);

      // A position below 1 is refused with an actionable message.
      const bad = await mcpCallTool(agent, key, "propose_series_create", {
        ...createArgs,
        name: `SQ-Bad-${stamp}`,
        titles: [{ title_id: titleId, position: 0 }],
      });
      expect(bad.isError).toBe(true);
      expect(bad.content[0].text).toContain(`The position of title ${titleId} must be between 1`);
    } finally {
      await agent.dispose();
      await page.goto("/admin?tab=api_keys");
      await revokeApiKey(page, propose.label);
    }

    // Nothing was written to the catalog: the series page shows no tome.
    await page.goto(`/series/${seriesId}`);
    await expect(page.locator("h1")).toContainText(existingName);
    await expect(page.getByText(`${existingName} tome 4`)).toHaveCount(0);
  });
});
