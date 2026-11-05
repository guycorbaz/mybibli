/**
 * Story 15-6 E2E (#564 part 2, FR166) — the value coverage on
 * `/stats/value`, and the agent's list of volumes to value.
 *
 * Real journey from a blank browser: loginAs(admin) creates a `propose`
 * key → loginAs(librarian) → catalogue a title and a volume by scan → the
 * agent puts the title in a new series (`propose_series_create`, accepted
 * by the librarian) → `/stats/value` shows the series' coverage at 0 %
 * (axe clean) → `list_volumes_to_value` filtered on the series lists the
 * volume, never valued → the librarian enters a current value on the
 * volume's edit form → the series' coverage reads 1 of 1, 100 % → the
 * agent reads the dated value.
 *
 * The DB is shared with parallel specs: every assertion is on the series
 * this test creates (unique name), never on library-wide counts. Spec ID
 * "VC" for ISBNs (random sequence); V-code band V7500–V7999 (unused by any
 * other spec).
 */
import { test, expect, request as playwrightRequest, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";
import { volumeIdByLabel } from "../../helpers/catalog";
import { checkAccessibility } from "../../helpers/accessibility";
import { mcpCallTool } from "../../helpers/mcp";
import { acceptFromDetail, newProposeKey, revokeProposeKey } from "../../helpers/proposals";

const SOURCE = "https://example.org/serie-vc";
const SHARE_0 = /^0 ?%$/;
const SHARE_100 = /^100 ?%$/;

function coverageRow(page: Page, series: string) {
  return page.locator(`#stats-value-coverage-series tr[data-coverage-row="${series}"]`);
}

test.describe("Story 15-6 — value coverage", () => {
  test.setTimeout(120000);

  test("the coverage grows when a volume is valued", async ({ page, baseURL }) => {
    const key = await newProposeKey(page, "value-coverage");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");

      const isbn = specIsbn("VC", 1 + Math.floor(Math.random() * 89999));
      const vcode = `V${7500 + Math.floor(Math.random() * 500)}`;
      await scanTitleAndVolume(page, isbn, vcode);
      await page.goto(`/?q=${isbn}`);
      await page
        .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
        .first()
        .click();
      await expect(page).toHaveURL(/\/title\/\d+/);
      const titleId = Number(/\/title\/(\d+)/.exec(page.url())![1]);
      const volumeId = Number(await volumeIdByLabel(page, String(titleId), vcode));
      // The background metadata fetch bumps the title's version: wait for
      // it, or the series proposal filed now would go stale.
      await expect
        .poll(
          async () =>
            (await mcpCallTool(agent, key.plaintext, "get_title", { id: titleId }))
              .structuredContent.publisher,
          { timeout: 15000 },
        )
        .toBe("Test Publisher");

      // A series of our own, holding the title.
      const series = `VC-Série-${Date.now()}`;
      const created = await mcpCallTool(agent, key.plaintext, "propose_series_create", {
        name: series,
        series_type: "open",
        titles: [{ title_id: titleId, position: 1 }],
        sources: [SOURCE],
      });
      expect(created.isError, JSON.stringify(created.content)).toBe(false);
      await page.goto(`/proposals/${created.structuredContent.proposal_id}`);
      await acceptFromDetail(page);
      await page.locator("#proposal-detail-body").locator('a[href^="/series/"]').first().click();
      await page.waitForURL(/\/series\/\d+$/);
      const seriesId = Number(new URL(page.url()).pathname.split("/").pop());

      // The coverage of the series: one volume, none valued.
      await page.goto("/stats/value");
      await expect(page.locator("#stats-value-coverage-heading")).toBeVisible();
      const row = coverageRow(page, series);
      await expect(row.locator("[data-coverage-volumes]")).toHaveText("1");
      await expect(row.locator("[data-coverage-valued]")).toHaveText("0");
      await expect(row.locator("[data-coverage-share]")).toHaveText(SHARE_0);
      await checkAccessibility(page);

      // The agent finds the volume, never valued.
      const before = (
        await mcpCallTool(agent, key.plaintext, "list_volumes_to_value", { series_id: seriesId })
      ).structuredContent;
      expect(before.total_items).toBe(1);
      expect(before.items[0].volume_id).toBe(volumeId);
      expect(before.items[0].current_value).toBeNull();

      // The librarian enters a value by hand.
      await page.goto(`/volume/${volumeId}/edit`);
      await page.locator("#current_value").fill("33.50");
      await page.locator('form[action$="/update"] button[type="submit"]').click();
      await expect(page).toHaveURL(new RegExp(`/volume/${volumeId}$`));

      await page.goto("/stats/value");
      await expect(row.locator("[data-coverage-valued]")).toHaveText("1");
      await expect(row.locator("[data-coverage-share]")).toHaveText(SHARE_100);
      await expect(row.locator("[data-coverage-old]")).toHaveText("0");
      await checkAccessibility(page);

      const after = (
        await mcpCallTool(agent, key.plaintext, "list_volumes_to_value", { series_id: seriesId })
      ).structuredContent;
      expect(after.items[0].current_value.amount).toBe(33.5);
      expect(after.items[0].current_value.updated_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    } finally {
      await agent.dispose();
      await revokeProposeKey(page, key.label);
    }
  });
});
