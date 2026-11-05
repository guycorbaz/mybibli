/**
 * Story 15-7 — Epic 15 smoke test (Foundation Rule #7): the agent's next
 * tasks, end to end, from a blank browser. If this test fails, epic 15 is
 * not done.
 *
 * Journey:
 *   loginAs(admin) → Admin → Skills: write a skill → create a `propose`
 *   key → as a separate MCP client: `prompts/list` lists the skill and
 *   `get_skill` reads it →
 *   loginAs(librarian) → scan a title and a volume → the agent puts the
 *   title in a new open series with one expected tome (accepted) → the
 *   agent proposes, with `propose_series_update`, to close the series at 1
 *   and remove that tome → the librarian accepts → the series page shows
 *   it closed at 1 →
 *   the agent finds the volume with `list_volumes_to_value`, proposes its
 *   value with `propose_volume_value` → the librarian accepts → the value
 *   statistics show the series' coverage at 100 %.
 *
 * Cleanup: the skill is deactivated, the key revoked.
 *
 * Spec ID "E5" for ISBNs (random sequence); V-code band V8000–V8499
 * (unused by any other spec); unique skill and series names; assertions
 * only on rows this test creates.
 */
import { test, expect, request as playwrightRequest, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";
import { volumeIdByLabel } from "../../helpers/catalog";
import { createApiKey } from "../../helpers/api-keys";
import { mcpCallTool, mcpPost } from "../../helpers/mcp";
import { acceptFromDetail, revokeProposeKey } from "../../helpers/proposals";

const SOURCE = "https://example.org/epic15";
const SKILLS_TAB = /^(Skills|Compétences|Skill)$/i;
const INACTIVE = /^(Inactive|Inaktiv|Inattivo)$/i;
const LEGACY = { "MCP-Protocol-Version": "2025-11-25" };
const CLOSED = /^(Closed|Fermée|Abgeschlossen|Chiusa)$/;
const TOTAL_1 = /^(Total volumes|Nombre total de volumes|Gesamtzahl der Exemplare|Totale esemplari): 1$/;
const SHARE_100 = /^100 ?%$/;

async function openSkillsTab(page: Page) {
  await page.goto("/admin", { waitUntil: "domcontentloaded" });
  await page.getByRole("tab", { name: SKILLS_TAB }).click();
  await expect(page.locator("#admin-skills-editor")).toBeVisible({ timeout: 10000 });
}

const skillRow = (page: Page, name: string) =>
  page.locator("#admin-skills-list tbody tr").filter({
    has: page.getByText(name, { exact: true }),
  });

async function openProposal(page: Page, id: number) {
  await page.goto(`/proposals/${id}`);
  await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
    "data-proposal-status",
    "pending",
  );
}

test.describe("Epic 15 smoke — the agent's next tasks", () => {
  test.setTimeout(180000);

  test("skill, series modification and volume value, from a blank browser", async ({
    page,
    baseURL,
  }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    // 1. The admin writes a skill.
    await openSkillsTab(page);
    const skill = `0e5-${crypto.randomUUID().slice(0, 8)}`;
    const description = `Valuing conventions ${skill}`;
    const body = `# Values\n\nUse completed sales only (${skill}).`;
    const editor = page.locator("#admin-skills-editor");
    await editor.locator("#admin-skills-name").fill(skill);
    await editor.locator("#admin-skills-description").fill(description);
    await editor.locator("#admin-skills-body").fill(body);
    await editor.locator("#admin-skills-submit").click();
    await expect(skillRow(page, skill)).toBeVisible({ timeout: 10000 });

    const key = await createApiKey(page, "propose", "epic15-smoke");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      // 2. The agent lists the skill as a prompt and reads it.
      const list = await mcpPost(
        agent,
        key.plaintext,
        { jsonrpc: "2.0", id: 1, method: "prompts/list" },
        LEGACY,
      );
      expect(list.status()).toBe(200);
      const prompts: { name: string; description: string }[] = (await list.json()).result
        .prompts;
      expect(prompts.find((p) => p.name === skill)).toEqual({ name: skill, description });
      const read = await mcpCallTool(agent, key.plaintext, "get_skill", { name: skill });
      expect(read.isError).toBe(false);
      expect(read.structuredContent.body).toBe(body);

      // 3. A title and its volume, catalogued by scan.
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const isbn = specIsbn("E5", 1 + Math.floor(Math.random() * 89999));
      const vcode = `V${8000 + Math.floor(Math.random() * 500)}`;
      await scanTitleAndVolume(page, isbn, vcode);
      await page.goto(`/?q=${isbn}`);
      await page
        .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
        .first()
        .click();
      await expect(page).toHaveURL(/\/title\/\d+/);
      const titleId = Number(/\/title\/(\d+)/.exec(page.url())![1]);
      const volumeId = Number(await volumeIdByLabel(page, String(titleId), vcode));
      await expect
        .poll(
          async () =>
            (await mcpCallTool(agent, key.plaintext, "get_title", { id: titleId }))
              .structuredContent.publisher,
          { timeout: 15000 },
        )
        .toBe("Test Publisher");

      // 4. The agent creates a series holding the title, with an expected tome.
      const seriesName = `E5-Série-${Date.now()}`;
      const created = await mcpCallTool(agent, key.plaintext, "propose_series_create", {
        name: seriesName,
        series_type: "open",
        titles: [{ title_id: titleId, position: 1 }],
        expected_tomes: [{ position: 2, title: "E5 tome deux" }],
        sources: [SOURCE],
      });
      expect(created.isError, JSON.stringify(created.content)).toBe(false);
      await openProposal(page, created.structuredContent.proposal_id);
      await acceptFromDetail(page);
      await page.locator("#proposal-detail-body").locator('a[href^="/series/"]').first().click();
      await page.waitForURL(/\/series\/\d+$/);
      const seriesId = Number(new URL(page.url()).pathname.split("/").pop());

      // 5. The agent closes the series at 1, removing the expected tome.
      const series = (await mcpCallTool(agent, key.plaintext, "get_series", { id: seriesId }))
        .structuredContent;
      const tome = (series.expected_tomes as { id: number; position: number }[]).find(
        (t) => t.position === 2,
      )!;
      const update = await mcpCallTool(agent, key.plaintext, "propose_series_update", {
        series_id: seriesId,
        series_type: "closed",
        total_volume_count: 1,
        remove_expected_tomes: [tome.id],
        sources: [SOURCE],
      });
      expect(update.isError, JSON.stringify(update.content)).toBe(false);
      await openProposal(page, update.structuredContent.proposal_id);
      await expect(page.locator("table[data-series-update]")).toBeVisible();
      await acceptFromDetail(page);
      await page.goto(`/series/${seriesId}`);
      await expect(page.getByText(CLOSED)).toBeVisible();
      await expect(page.getByText(TOTAL_1)).toBeVisible();
      await expect(page.getByText("E5 tome deux")).toHaveCount(0);

      // 6. The agent values the volume of the series.
      const toValue = (
        await mcpCallTool(agent, key.plaintext, "list_volumes_to_value", { series_id: seriesId })
      ).structuredContent;
      expect(toValue.total_items).toBe(1);
      expect(toValue.items[0].volume_id).toBe(volumeId);
      const currency = (await mcpCallTool(agent, key.plaintext, "get_title", { id: titleId }))
        .structuredContent.default_currency;
      const valued = await mcpCallTool(agent, key.plaintext, "propose_volume_value", {
        volume_id: volumeId,
        current_value: { amount: 64, currency },
        note: "Completed sales, good condition.",
        sources: [SOURCE],
      });
      expect(valued.isError, JSON.stringify(valued.content)).toBe(false);
      await openProposal(page, valued.structuredContent.proposal_id);
      await expect(page.locator("table[data-volume-value]")).toBeVisible();
      await acceptFromDetail(page);

      // 7. The value statistics: the series is fully covered.
      await page.goto("/stats/value");
      const row = page.locator(
        `#stats-value-coverage-series tr[data-coverage-row="${seriesName}"]`,
      );
      await expect(row.locator("[data-coverage-volumes]")).toHaveText("1");
      await expect(row.locator("[data-coverage-valued]")).toHaveText("1");
      await expect(row.locator("[data-coverage-share]")).toHaveText(SHARE_100);
    } finally {
      await agent.dispose();
      await page.context().clearCookies();
      await loginAs(page, "admin");
      await openSkillsTab(page);
      await skillRow(page, skill).locator('button[id^="admin-skill-toggle-"]').click();
      await expect(skillRow(page, skill).getByText(INACTIVE)).toBeVisible({ timeout: 10000 });
      await revokeProposeKey(page, key.label);
    }
  });
});
