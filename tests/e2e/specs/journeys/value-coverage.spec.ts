/**
 * Story 15-6 E2E (#564 part 2, FR166) — the value coverage on
 * `/stats/value`, and the agent's list of volumes to value.
 *
 * Real journey from a blank browser: loginAs(librarian) → catalogue a
 * title and a volume by scan → `/stats/value` shows the coverage section
 * (axe clean) → an agent's `list_volumes_to_value` lists the new volume
 * (never valued) → the librarian enters a current value on the volume's
 * edit form → the coverage counts one more valued volume → the volume is
 * now listed after every never-valued one.
 *
 * Spec ID "VC" for ISBNs (random sequence, so a rerun against an un-reset
 * DB starts from a fresh title); V-code band V7500–V7999 (unused by any
 * other spec). The DB is shared with parallel specs: counts are compared
 * relatively (values are only added by specs, never removed).
 */
import { test, expect, request as playwrightRequest, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";
import { volumeIdByLabel } from "../../helpers/catalog";
import { checkAccessibility } from "../../helpers/accessibility";
import { mcpCallTool } from "../../helpers/mcp";
import { newProposeKey, revokeProposeKey } from "../../helpers/proposals";

async function overallValued(page: Page): Promise<number> {
  await page.goto("/stats/value");
  const row = page.locator("#stats-value-coverage-overall tbody tr").first();
  await expect(row).toBeVisible();
  return Number(await row.locator("[data-coverage-valued]").innerText());
}

test.describe("Story 15-6 — value coverage", () => {
  test.setTimeout(90000);

  test("the coverage grows when a volume is valued", async ({ page, baseURL }) => {
    const key = await newProposeKey(page, "value-coverage");
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

    const before = await overallValued(page);
    await expect(page.locator("#stats-value-coverage-heading")).toBeVisible();
    await checkAccessibility(page);

    // The agent finds the volume among the never-valued ones.
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    const findVolume = async () => {
      for (let p = 1; p <= 400; p++) {
        const res = (await mcpCallTool(agent, key.plaintext, "list_volumes_to_value", { page: p }))
          .structuredContent;
        const hit = res.items.find((i: { volume_id: number }) => i.volume_id === volumeId);
        if (hit || p >= res.total_pages) return hit;
      }
      return undefined;
    };
    try {
      const listed = await findVolume();
      expect(listed, "the new volume is listed").toBeTruthy();
      expect(listed.label).toBe(vcode);
      expect(listed.current_value).toBeNull();

      // The librarian enters a value by hand.
      await page.goto(`/volume/${volumeId}/edit`);
      await page.locator("#current_value").fill("33.50");
      await page.locator('form[action$="/update"] button[type="submit"]').click();
      await expect(page).toHaveURL(new RegExp(`/volume/${volumeId}$`));

      const after = await overallValued(page);
      expect(after).toBeGreaterThanOrEqual(before + 1);
      await checkAccessibility(page);

      const valued = await findVolume();
      expect(valued.current_value.amount).toBe(33.5);
      expect(valued.current_value.updated_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
    } finally {
      await agent.dispose();
    }
    await revokeProposeKey(page, key.label);
  });
});
