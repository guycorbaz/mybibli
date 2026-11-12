/**
 * Story 18-6 — Epic 18 smoke test (Foundation Rule #7): maintenance, end to
 * end, from a blank browser. If this test fails, epic 18 is not done.
 *
 * Journey:
 *   loginAs(admin) → a title scanned on /catalog, so the dashboard has data
 *   → the dashboard's genre donut is drawn by the vendored Chart.js 4.5.1
 *   (18-1) → two series created; one deleted from a second tab, then
 *   "Delete" on its stale page shows the server's not-found message in the
 *   page's feedback region, focus kept (18-5) → a read key lists the kept
 *   series through `GET /api/v1/series`, not the deleted one (18-4) → a
 *   propose key's MCP `initialize` is answered by the rmcp 3.9 server
 *   (18-3) → both keys revoked. 18-2 (pinned build images) is the image
 *   this stack runs; it has no user-visible journey.
 *
 * Data: ISBN `specIsbn("E8", random)`; series names carry a per-run token;
 * assertions only on this spec's rows (Rule #23). The file creates series,
 * so its name sorts after the specs that assume none exists (#528).
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { mcpPost } from "../../helpers/mcp";

const NOT_FOUND = /couldn't find what you were looking for|Introuvable|nicht finden|non .*trovat/i;
const GENERIC = /Server error|Erreur serveur|Serverfehler|Errore del server/i;
const DELETE = /^(Delete|Supprimer|Löschen|Elimina)$/i;

async function createSeries(page: import("@playwright/test").Page, name: string): Promise<string> {
  await page.goto("/series/new");
  await page.locator("#series-name").fill(name);
  await page.locator('main button[type="submit"]').last().click();
  await page.waitForURL(/\/series\/\d+$/);
  return page.url();
}

test.describe("Epic 18 smoke — maintenance", () => {
  test("dashboard chart, a visible series error, the series API and MCP", async ({
    page,
    context,
    baseURL,
  }) => {
    test.setTimeout(120000);
    await context.clearCookies();
    await loginAs(page, "admin");

    // A title, so the dashboard has something to chart.
    await page.goto("/catalog");
    const scan = page.locator("#scan-field");
    await scan.fill(specIsbn("E8", Math.floor(Math.random() * 100000)));
    await scan.press("Enter");
    await expect(page.locator(".feedback-entry, .feedback-skeleton").first()).toBeVisible({
      timeout: 10000,
    });

    // 18-1 — the donut is drawn by the vendored Chart.js 4.5.1.
    await page.goto("/");
    const canvas = page.locator("#stats-by-genre-canvas");
    await expect(canvas).toBeVisible({ timeout: 10000 });
    await expect
      .poll(() => page.evaluate(() => (window as unknown as { Chart?: { version: string } }).Chart?.version))
      .toBe("4.5.1");
    const box = await canvas.boundingBox();
    expect(box && box.width > 0 && box.height > 0).toBe(true);

    // 18-5 — a series deleted in another tab: Delete shows the server's message.
    const token = `E8-${Date.now().toString(36)}`;
    const kept = `${token}-kept`;
    const gone = `${token}-gone`;
    await createSeries(page, kept);
    const goneUrl = await createSeries(page, gone);

    const other = await context.newPage();
    await other.goto(goneUrl);
    await other.getByRole("button", { name: DELETE }).click();
    await expect(other.locator("#modal-slot dialog[open]")).toBeVisible();
    await other.locator("[data-modal-confirm]").click();
    await other.waitForURL(/\/series$/);
    await other.close();

    const deleteBtn = page.getByRole("button", { name: DELETE });
    await deleteBtn.click();
    const region = page.locator("#feedback-list");
    await expect(region.locator(".feedback-entry")).toContainText(NOT_FOUND, { timeout: 10000 });
    await expect(region).not.toContainText(GENERIC);
    await expect(deleteBtn).toBeFocused();

    // 18-4 — the series API lists the kept series, not the deleted one.
    const read = await createApiKey(page, "read", "e8-read");
    const propose = await createApiKey(page, "propose", "e8-propose");
    let agent: Awaited<ReturnType<typeof playwrightRequest.newContext>> | undefined;
    try {
      const resp = await page.request.get("/api/v1/series", {
        headers: { Authorization: `Bearer ${read.plaintext}` },
      });
      expect(resp.status()).toBe(200);
      const names = ((await resp.json()) as Array<{ name: string }>).map((s) => s.name);
      expect(names).toContain(kept);
      expect(names).not.toContain(gone);

      // 18-3 — the MCP server (rmcp 3.9) answers `initialize`.
      agent = await playwrightRequest.newContext({ baseURL: baseURL! });
      const init = await mcpPost(agent, propose.plaintext, {
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: {
          protocolVersion: "2025-11-25",
          capabilities: {},
          clientInfo: { name: "e2e-epic18", version: "0" },
        },
      });
      expect(init.status()).toBe(200);
      const result = (await init.json()).result;
      expect(result.protocolVersion).toBe("2025-11-25");
      expect(result.capabilities.tools).toBeDefined();
      expect(result.serverInfo.name).toBeTruthy();
    } finally {
      await agent?.dispose();
      await page.goto("/admin?tab=api_keys");
      await revokeApiKey(page, read.label);
      await revokeApiKey(page, propose.label);
    }
  });
});
