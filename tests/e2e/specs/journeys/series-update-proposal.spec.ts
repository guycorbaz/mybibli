/**
 * Story 15-4 E2E — a series modification proposed by the agent (FR164,
 * UX-DR41).
 *
 * From a blank browser: an admin creates a `propose` key → cookies
 * cleared → loginAs(librarian) → the agent files a `series_create` (open,
 * three expected tomes) that the librarian accepts → the agent reads the
 * series over MCP and proposes, with `propose_series_update`, to close it
 * at 2 tomes — first without removing tome 3 (refused, the message names
 * position 3), then with tome 3 removed → the librarian opens the
 * proposal: current vs proposed type and total, the removed tome → accepts
 * through the UX-DR8 modal → the series page shows it closed, total 2,
 * and no longer the removed tome.
 *
 * Spec ID "SU". Series names carry a per-run stamp.
 */
import { test, expect, request as playwrightRequest, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { mcpCallTool } from "../../helpers/mcp";

const SOURCE = "https://example.org/serie";
const ACCEPT = /^(Accept|Accepter|Annehmen|Accetta)$/;
const ACCEPTED =
  /Proposal #\d+ accepted|Proposition n° \d+ acceptée|Vorschlag Nr\. \d+ angenommen|Proposta n\. \d+ accettata/i;
const CLOSED = /^(Closed|Fermée|Abgeschlossen|Chiusa)$/;
/** The series page's "total" figure (`series.total_count`), at 2. */
const TOTAL_2 = /^(Total volumes|Nombre total de volumes|Gesamtzahl der Exemplare|Totale esemplari): 2$/;

async function acceptFromDetail(page: Page) {
  await page.locator("#proposal-accept-button").click();
  const dialog = page.locator("#modal-slot dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.locator("[data-modal-confirm]")).toHaveText(ACCEPT);
  await dialog.locator("[data-modal-confirm]").click();
  await expect(dialog).toBeHidden({ timeout: 10000 });
  await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
    "data-proposal-status",
    "accepted",
  );
}

test.describe("Story 15-4 — series modification proposals", () => {
  test.setTimeout(90000);

  test("the agent closes a series at its real total and the librarian accepts it", async ({
    page,
    baseURL,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");
    const key = await createApiKey(page, "propose", "series-update");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");

      // A series the library already has: created through an accepted
      // agent proposal, open, with three expected tomes.
      const name = `SU-Neptune-${Date.now()}`;
      const created = await mcpCallTool(agent, key.plaintext, "propose_series_create", {
        name,
        series_type: "open",
        expected_tomes: [
          { position: 1, title: "SU tome un" },
          { position: 2, title: "SU tome deux" },
          { position: 3, title: "SU tome trois" },
        ],
        sources: [SOURCE],
      });
      expect(created.isError, JSON.stringify(created.content)).toBe(false);
      await page.goto(`/proposals/${created.structuredContent.proposal_id}`);
      await acceptFromDetail(page);
      await page
        .locator("#proposal-detail-body")
        .locator(`a[href^="/series/"]`)
        .first()
        .click();
      await page.waitForURL(/\/series\/\d+$/);
      const seriesId = Number(new URL(page.url()).pathname.split("/").pop());
      await expect(page.getByText("SU tome trois")).toBeVisible();

      // The agent reads the series and its expected tomes.
      const series = await mcpCallTool(agent, key.plaintext, "get_series", { id: seriesId });
      const tomes = series.structuredContent.expected_tomes as { id: number; position: number }[];
      const third = tomes.find((t) => t.position === 3)!.id;

      // Closing at 2 while tome 3 is expected: refused, position named.
      const refused = await mcpCallTool(agent, key.plaintext, "propose_series_update", {
        series_id: seriesId,
        series_type: "closed",
        total_volume_count: 2,
        sources: [SOURCE],
      });
      expect(refused.isError).toBe(true);
      expect(JSON.stringify(refused.content)).toContain("position(s) 3");

      const filed = await mcpCallTool(agent, key.plaintext, "propose_series_update", {
        series_id: seriesId,
        series_type: "closed",
        total_volume_count: 2,
        remove_expected_tomes: [third],
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const proposalId = filed.structuredContent.proposal_id as number;

      // Review: current vs proposed, the removed tome.
      await page.goto(`/proposals/${proposalId}`);
      const body = page.locator("#proposal-detail-body");
      await expect(body).toHaveAttribute("data-proposal-status", "pending");
      const table = body.locator("table[data-series-update]");
      await expect(table).toBeVisible();
      await expect(table.locator("td[data-proposed]").first()).toHaveText(CLOSED);
      await expect(table.locator("td[data-proposed]").nth(1)).toHaveText("2");
      await expect(body.locator("[data-removed-tome]")).toHaveText("SU tome trois");

      await acceptFromDetail(page);
      const entry = page.locator("#feedback-list .feedback-entry").filter({ hasText: ACCEPTED });
      await expect(entry).toBeVisible({ timeout: 10000 });

      // The series page: closed at 2, the removed tome gone.
      await page.goto(`/series/${seriesId}`);
      await expect(page.locator("h1")).toContainText(name);
      await expect(page.getByText(CLOSED)).toBeVisible();
      await expect(page.getByText(TOTAL_2)).toBeVisible();
      await expect(page.getByText("SU tome deux")).toBeVisible();
      await expect(page.getByText("SU tome trois")).toHaveCount(0);
    } finally {
      await agent.dispose();
      await page.context().clearCookies();
      await loginAs(page, "admin");
      await page.goto("/admin?tab=api_keys");
      await revokeApiKey(page, key.label);
    }
  });
});
