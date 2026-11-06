/**
 * Story 16-7 — Epic 16 smoke test (Foundation Rule #7): what real use showed,
 * end to end, from a blank browser. If this test fails, epic 16 is not done.
 *
 * Journey:
 *   loginAs(admin) → create a `propose` key → /catalog: an irregular code shows
 *   the choice with its warning → the librarian confirms → a volume is scanned
 *   on that title →
 *   as a separate agent client: upload a cover file, check its receipt, file
 *   `propose_cover` by token; propose a purchase price alone →
 *   loginAs(librarian) → accept both → the title shows the cover, the volume
 *   page shows the purchase price and no value.
 *
 * Cleanup: the key is revoked.
 *
 * Random 10-digit code; V-code band V9000–V9499 (home.spec.ts draws at random
 * from V1000–V9999, so a collision is rare but possible);
 * assertions only on rows this test creates (Rule #23).
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { createHash } from "node:crypto";
import { loginAs } from "../../helpers/auth";
import { volumeIdByLabel } from "../../helpers/catalog";
import { createApiKey } from "../../helpers/api-keys";
import { mcpCallTool } from "../../helpers/mcp";
import { acceptFromDetail, revokeProposeKey } from "../../helpers/proposals";

const SOURCE = "https://example.org/epic16";
/** A valid 1×1 RGBA PNG. */
const PNG_1X1 = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

/** 10 digits that fail as an ISBN-10 (a valid one would be read as an ISBN). */
function irregularCode(): string {
  let body = "5";
  while (body.length < 9) body += Math.floor(Math.random() * 10);
  const sum = [...body].reduce((acc, c, i) => acc + Number(c) * (10 - i), 0);
  const valid = (11 - (sum % 11)) % 11;
  return body + String((valid + 1) % 10);
}

test.describe("Epic 16 smoke — what real use showed", () => {
  test("a warned code becomes a title that the agent completes", async ({ page, baseURL }) => {
    test.setTimeout(90000);
    await page.context().clearCookies();
    await loginAs(page, "admin");
    const propose = await createApiKey(page, "propose", "epic16-smoke");

    // A code of no known shape asks first.
    await page.goto("/catalog");
    const field = page.locator("#scan-field");
    const code = irregularCode();
    await field.fill(code);
    await field.press("Enter");
    await expect(page.locator("[data-code-warning]").last()).toBeVisible();
    await page.locator(`button[role="radio"][hx-vals*="${code}"]`, { hasText: /Book|Livre/ }).click();
    await expect(page.locator(".feedback-skeleton").last()).toBeVisible();

    const vcode = `V${9000 + Math.floor(Math.random() * 500)}`;
    await field.fill(vcode);
    await field.press("Enter");
    await expect(page.locator(".feedback-entry").first()).toContainText(new RegExp(vcode, "i"), {
      timeout: 10000,
    });

    await page.goto(`/?q=${code}`);
    await page
      .locator('#browse-results table.browse-table tbody tr td a[href^="/title/"]')
      .first()
      .click();
    await expect(page).toHaveURL(/\/title\/\d+/);
    const titleId = Number(/\/title\/(\d+)/.exec(page.url())![1]!);
    const volumeId = Number(await volumeIdByLabel(page, String(titleId), vcode));

    // The agent sends the cover as a file, and a purchase price alone.
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    let coverProposal = 0;
    let priceProposal = 0;
    try {
      const uploaded = await agent.post("/api/v1/cover-uploads", {
        headers: { Authorization: `Bearer ${propose.plaintext}` },
        multipart: { file: { name: "cover.png", mimeType: "image/png", buffer: PNG_1X1 } },
      });
      expect(uploaded.status()).toBe(201);
      const receipt = await uploaded.json();
      expect(receipt.sha256).toBe(createHash("sha256").update(PNG_1X1).digest("hex"));
      const cover = await mcpCallTool(agent, propose.plaintext, "propose_cover", {
        title_id: titleId,
        upload_token: receipt.upload_token,
        sources: [SOURCE],
      });
      expect(cover.isError, JSON.stringify(cover.content)).toBe(false);
      coverProposal = cover.structuredContent.proposal_id;

      const doc = (await mcpCallTool(agent, propose.plaintext, "get_title", { id: titleId }))
        .structuredContent;
      const price = await mcpCallTool(agent, propose.plaintext, "propose_volume_value", {
        volume_id: volumeId,
        list_price: { amount: 24.9, currency: doc.default_currency },
        sources: [SOURCE],
      });
      expect(price.isError, JSON.stringify(price.content)).toBe(false);
      priceProposal = price.structuredContent.proposal_id;
    } finally {
      await agent.dispose();
    }

    // The librarian decides.
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto(`/proposals/${coverProposal}`);
    await acceptFromDetail(page);
    await page.goto(`/proposals/${priceProposal}`);
    await expect(page.locator('#proposal-items li[data-line="list_price"]')).toContainText(vcode);
    await acceptFromDetail(page);

    await page.goto(`/title/${titleId}`);
    await expect(page.locator(`img[src*="/covers/${titleId}.jpg"]`).first()).toBeVisible();
    await page.goto(`/volume/${volumeId}`);
    await expect(page.locator("#volume-purchase-price")).toContainText(/^24[.,]90/);
    await expect(page.locator("#volume-current-value")).toHaveText("—");

    await revokeProposeKey(page, propose.label);
  });
});
