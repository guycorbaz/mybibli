/**
 * Story 15-5 E2E (#564, FR165, UX-DR41) — an agent proposes what a volume
 * is worth; the librarian accepts it.
 *
 * Real journey from a blank browser: loginAs(admin) → catalogue a title
 * and a volume by scan → create a `propose` key → as a separate agent
 * client: `get_title` gives the volume (no amounts yet) and the default
 * currency → `propose_volume_value` in another currency is refused → with
 * the default currency, a current value and a list price are filed →
 * loginAs(librarian) → the proposal page shows the volume, current vs
 * proposed value, the list price labelled as a stand-in for the purchase
 * price, the note and the sources → accept through the UX-DR8 modal → the
 * volume's edit form holds the amounts → `get_title` reports them dated →
 * the key is revoked.
 *
 * Spec ID "VV" for ISBNs (random sequence, so a rerun against an un-reset
 * DB starts from a fresh title); V-code band V7000–V7499 (unused by any
 * other spec); assertions only on rows this test creates.
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { scanTitleAndVolume } from "../../helpers/loans";
import { volumeIdByLabel } from "../../helpers/catalog";
import { createApiKey } from "../../helpers/api-keys";
import { mcpCallTool } from "../../helpers/mcp";
import { acceptFromDetail, revokeProposeKey } from "../../helpers/proposals";

const SOURCE = "https://example.org/cote-vv";
const NOTE = "Good condition, first printing; comparable completed sales.";
const VALUE = /^(48\.50 CHF|48,50 CHF)$/;
const LIST_PRICE = /^(12\.90 CHF|12,90 CHF)$/;

test.describe("Story 15-5 — the agent proposes a volume value", () => {
  test.setTimeout(90000);

  test("an agent values a volume and the librarian accepts", async ({ page, baseURL }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    const isbn = specIsbn("VV", 1 + Math.floor(Math.random() * 89999));
    const vcode = `V${7000 + Math.floor(Math.random() * 500)}`;
    await scanTitleAndVolume(page, isbn, vcode);
    await page.goto(`/?q=${isbn}`);
    await page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first()
      .click();
    await expect(page).toHaveURL(/\/title\/\d+/);
    const titleId = Number(/\/title\/(\d+)/.exec(page.url())![1]);
    const volumeId = Number(await volumeIdByLabel(page, String(titleId), vcode));

    const propose = await createApiKey(page, "propose", "volume-value");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    let proposalId = 0;
    try {
      const key = propose.plaintext;
      const before = (await mcpCallTool(agent, key, "get_title", { id: titleId }))
        .structuredContent;
      const currency = before.default_currency as string;
      expect(currency).toMatch(/^[A-Z]{3}$/);
      const volume = before.volumes.find((v: { label: string }) => v.label === vcode);
      expect(volume.id).toBe(volumeId);
      expect(volume.purchase_price).toBeNull();
      expect(volume.current_value).toBeNull();

      // No conversion: another currency is refused, the default named.
      const other = currency === "EUR" ? "USD" : "EUR";
      const refused = await mcpCallTool(agent, key, "propose_volume_value", {
        volume_id: volumeId,
        current_value: { amount: 48.5, currency: other },
        sources: [SOURCE],
      });
      expect(refused.isError).toBe(true);
      expect(refused.content[0].text).toContain(`records values in ${currency}`);

      const filed = await mcpCallTool(agent, key, "propose_volume_value", {
        volume_id: volumeId,
        current_value: { amount: 48.5, currency },
        list_price: { amount: 12.9, currency },
        note: NOTE,
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      expect(filed.structuredContent.kind).toBe("volume_value");
      expect(filed.structuredContent.status).toBe("pending");
      proposalId = filed.structuredContent.proposal_id;
    } finally {
      await agent.dispose();
    }

    // The librarian reviews and accepts.
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto(`/proposals/${proposalId}`);
    await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
      "data-proposal-status",
      "pending",
    );
    const valueLine = page.locator('#proposal-items li[data-line="current_value"]');
    await expect(valueLine.locator("[data-value-target]")).toContainText(vcode);
    const table = valueLine.locator("table[data-volume-value]");
    await expect(table.locator("td[data-proposed]")).toHaveText(VALUE);
    await expect(valueLine.locator("[data-value-note]")).toHaveText(NOTE);
    const listLine = page.locator('#proposal-items li[data-line="list_price"]');
    await expect(listLine.locator("[data-list-price]")).toHaveText(LIST_PRICE);
    await expect(listLine.locator("[data-list-price-note]")).toBeVisible();
    await expect(page.locator("#proposal-sources")).toContainText(SOURCE);
    await acceptFromDetail(page);

    // The volume holds the amounts (its edit form, CR #243).
    await page.goto(`/volume/${volumeId}/edit`);
    await expect(page.locator("#current_value")).toHaveValue(/^48\.5/);
    await expect(page.locator("#purchase_price")).toHaveValue(/^12\.9/);

    // The agent reads them back, dated; then the key is revoked.
    const reader = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      const after = (await mcpCallTool(reader, propose.plaintext, "get_title", { id: titleId }))
        .structuredContent;
      const volume = after.volumes.find((v: { id: number }) => v.id === volumeId);
      expect(volume.current_value.amount).toBe(48.5);
      expect(volume.current_value.updated_at).toMatch(/^\d{4}-\d{2}-\d{2}T/);
      expect(volume.purchase_price.amount).toBe(12.9);
    } finally {
      await reader.dispose();
    }
    await revokeProposeKey(page, propose.label);
  });

  // Story 16-3 (#639, FR168) — a purchase price proposed without a value,
  // then a replacement of one that looks wrong.
  test("an agent proposes a purchase price alone and flags a wrong one", async ({ page, baseURL }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");

    const isbn = specIsbn("VV", 1 + Math.floor(Math.random() * 89999));
    const vcode = `V${7000 + Math.floor(Math.random() * 500)}`;
    await scanTitleAndVolume(page, isbn, vcode);
    await page.goto(`/?q=${isbn}`);
    await page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first()
      .click();
    await expect(page).toHaveURL(/\/title\/\d+/);
    const titleId = Number(/\/title\/(\d+)/.exec(page.url())![1]);
    const volumeId = Number(await volumeIdByLabel(page, String(titleId), vcode));

    const propose = await createApiKey(page, "propose", "volume-price");
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    const file = async (amount: number): Promise<number> => {
      const { default_currency: currency } = (
        await mcpCallTool(agent, propose.plaintext, "get_title", { id: titleId })
      ).structuredContent;
      const filed = await mcpCallTool(agent, propose.plaintext, "propose_volume_value", {
        volume_id: volumeId,
        list_price: { amount, currency },
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      return filed.structuredContent.proposal_id;
    };
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");

      // Alone: the volume page shows the price; the value stays unset.
      await page.goto(`/proposals/${await file(24)}`);
      const alone = page.locator('#proposal-items li[data-line="list_price"]');
      await expect(alone.locator("[data-value-target]")).toContainText(vcode);
      await expect(alone.locator("[data-purchase-problem]")).toHaveCount(0);
      await acceptFromDetail(page);
      await page.goto(`/volume/${volumeId}`);
      await expect(page.locator("#volume-purchase-price")).toContainText(/^24[.,]00/);
      await expect(page.locator("#volume-current-value")).toHaveText("—");

      // A price ten times away from the list price is flagged and replaced.
      await page.goto(`/proposals/${await file(2.4)}`);
      const replacing = page.locator('#proposal-items li[data-line="list_price"]');
      await expect(replacing.locator("[data-purchase-replaces]")).toContainText(/24[.,]00/);
      await expect(replacing.locator("[data-purchase-problem]")).toBeVisible();
      await acceptFromDetail(page);
      await page.goto(`/volume/${volumeId}`);
      await expect(page.locator("#volume-purchase-price")).toContainText(/^2[.,]40/);
    } finally {
      await agent.dispose();
    }
    await page.context().clearCookies();
    await loginAs(page, "admin");
    await revokeProposeKey(page, propose.label);
  });

  // Story 16-3 (#639) — the thresholds are in Admin → System. The defaults are
  // saved back as they are: the setting is shared with every parallel spec.
  test("an admin sees and saves the purchase price thresholds", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "admin");
    await page.goto("/admin?tab=system");
    const form = page.locator("#admin-system-purchase-thresholds-form");
    const gap = form.locator('input[name="gap_factor"]');
    await expect(gap).toHaveValue("3");
    await expect(form.locator('input[name="negligible_amount"]')).toHaveValue("1.00");

    // Out of range: refused, the typed value kept.
    await gap.evaluate((el: HTMLInputElement) => el.removeAttribute("min"));
    await gap.fill("1");
    await form.locator('button[type="submit"]').click();
    await expect(form.locator('input[name="gap_factor"]')).toHaveValue("1");
    await expect(page.locator("#feedback-list .feedback-entry").last()).toContainText(/2.*100/);

    await form.locator('input[name="gap_factor"]').fill("3");
    await form.locator('button[type="submit"]').click();
    await expect(page.locator("#feedback-list .feedback-entry").last()).toContainText(
      /saved|enregistr/i,
    );
    await expect(form.locator('input[name="gap_factor"]')).toHaveValue("3");
  });
});
