/**
 * Story 11-16 E2E — the proposals page: list, detail, reject (FR137,
 * FR138, FR146, UX-DR31) and the resubmission delay seen from the agent
 * (#536, FR136).
 *
 * Journey 1, from a blank browser: an admin creates a `propose` key →
 * cookies cleared → loginAs(librarian) → scan an ISBN in /catalog → the
 * agent files a metadata proposal → /proposals lists it → open it →
 * sources open in a new tab, the agent's text is escaped → Reject
 * through the UX-DR8 modal with a reason → the page shows "rejected",
 * the decider and the reason → the agent's equivalent re-filing is
 * refused with the reopening date → `list_my_proposals` shows the reason
 * and `resubmittable_from` → the admin revokes the key.
 *
 * Journey 2: a cover proposal shows its staged image to the librarian;
 * rejecting it without a reason removes the image (the staged file is
 * discarded).
 *
 * Spec ID "PR" for ISBNs; every proposal targets this spec's own title,
 * so its rejections block nothing elsewhere. Creates no series (#528).
 * The resubmission delay is stack-global: this spec only needs it to be
 * non-zero (11-15's spec moves it between 45 and 90).
 */
import { test, expect, request as playwrightRequest, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { mcpCallTool } from "../../helpers/mcp";
import { checkAccessibility } from "../../helpers/accessibility";

const SOURCE = "https://example.org/revue?x=1";
const REJECTED =
  /Proposal #\d+ rejected|Proposition n° \d+ refusée|Vorschlag Nr\. \d+ abgelehnt|Proposta n\. \d+ rifiutata/i;
const REJECT_BUTTON = /^(Reject|Refuser|Ablehnen|Rifiuta)$/;
const PNG_1X1 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

/** Story 12-4 (#561): the subject link, named "<subject> — proposal #N". */
function openName(id: number): RegExp {
  return new RegExp(`(proposal #|proposition n° |Vorschlag Nr\\. |proposta n\\. )${id}$`);
}

async function newKey(page: Page, prefix: string) {
  await page.context().clearCookies();
  await loginAs(page, "admin");
  return createApiKey(page, "propose", prefix);
}

async function revoke(page: Page, label: string) {
  await page.context().clearCookies();
  await loginAs(page, "admin");
  await page.goto("/admin?tab=api_keys");
  await revokeApiKey(page, label);
}

/** Scan a fresh ISBN as the logged-in user and return the title id. */
async function scanTitle(page: Page, seq: number): Promise<number> {
  await page.goto("/catalog");
  const scanField = page.locator("#scan-field");
  await scanField.fill(specIsbn("PR", seq));
  await scanField.press("Enter");
  const titleId = Number(await titleIdFromSkeleton(page));
  expect(titleId).toBeGreaterThan(0);
  return titleId;
}

async function rejectFromDetail(page: Page, reason: string) {
  await page.getByRole("button", { name: REJECT_BUTTON }).click();
  const dialog = page.locator("#modal-slot dialog");
  await expect(dialog).toBeVisible();
  if (reason) {
    await dialog.locator("textarea[name=reason]").fill(reason);
  }
  await dialog.locator("[data-modal-confirm]").click();
  await expect(dialog).toBeHidden({ timeout: 10000 });
  await expect(page.locator("#feedback-list").getByText(REJECTED)).toBeVisible({
    timeout: 10000,
  });
  await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
    "data-proposal-status",
    "rejected",
  );
}

test.describe("Story 11-16 — proposals page", () => {
  // Each journey creates and revokes a key as an admin around the
  // librarian's session: three logins plus a metadata fetch.
  test.setTimeout(90000);

  test("a librarian rejects a proposal with a reason; the agent cannot re-file it", async ({
    page,
    baseURL,
  }) => {
    const key = await newKey(page, "review");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      // The journey itself starts from a blank browser, as a librarian.
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const titleId = await scanTitle(page, 1);

      // Wait for the background metadata fetch (it bumps the version).
      let version = 0;
      await expect
        .poll(
          async () => {
            const t = (await mcpCallTool(agent, key.plaintext, "get_title", { id: titleId }))
              .structuredContent;
            version = t.version;
            return t.publisher;
          },
          { timeout: 15000 },
        )
        .toBe("Test Publisher");
      const filed = await mcpCallTool(agent, key.plaintext, "propose_title_metadata", {
        title_id: titleId,
        version,
        fields: { subtitle: "Édition <b>révisée</b>" },
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const proposalId = filed.structuredContent.proposal_id as number;

      // The list shows it among the pending proposals.
      await page.goto("/proposals");
      const row = page.locator(`tr[data-proposal-id="${proposalId}"]`);
      await expect(row).toBeVisible();
      await expect(row).toContainText(key.label);
      await expect(row.locator("[data-proposal-status]")).toHaveAttribute(
        "data-proposal-status",
        "pending",
      );
      await row.getByRole("link", { name: openName(proposalId) }).click();

      // The detail: escaped value, source in a new tab.
      await expect(page.locator("h1")).toContainText(`${proposalId}`);
      await expect(page.locator("[data-proposed]").first()).toHaveText("Édition <b>révisée</b>");
      const source = page.locator("#proposal-sources a");
      await expect(source).toHaveAttribute("href", SOURCE);
      await expect(source).toHaveAttribute("target", "_blank");
      await expect(source).toHaveAttribute("rel", "noopener noreferrer");

      await rejectFromDetail(page, "Pas cette édition");
      await expect(page.locator("#proposal-reason")).toHaveText("Pas cette édition");
      await expect(page.locator("#proposal-decided-by")).toContainText("librarian");
      await expect(page.getByRole("button", { name: REJECT_BUTTON })).toHaveCount(0);

      // A reload shows the same decision (it is stored, not only swapped in).
      await page.reload();
      await expect(page.locator("#proposal-reason")).toHaveText("Pas cette édition");

      // #536 — the agent's equivalent re-filing is refused inside the delay.
      const refused = await mcpCallTool(agent, key.plaintext, "propose_title_metadata", {
        title_id: titleId,
        fields: { subtitle: "Autre essai" },
        sources: [SOURCE],
      });
      expect(refused.isError).toBe(true);
      const text = refused.content[0].text as string;
      expect(text).toContain("Pas cette édition");
      expect(text).toMatch(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z/);

      const history = await mcpCallTool(agent, key.plaintext, "list_my_proposals", {
        status: "rejected",
      });
      const entry = (history.structuredContent.items as Array<Record<string, unknown>>).find(
        (i) => i.proposal_id === proposalId,
      )!;
      expect(entry.rejection_reason).toBe("Pas cette édition");
      expect(String(entry.resubmittable_from)).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/);
    } finally {
      await agent.dispose();
      await revoke(page, key.label);
    }
  });

  test("a cover proposal shows its staged image until it is rejected", async ({
    page,
    baseURL,
  }) => {
    const key = await newKey(page, "review-cover");
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const titleId = await scanTitle(page, 2);
      const filed = await mcpCallTool(agent, key.plaintext, "propose_cover", {
        title_id: titleId,
        image_base64: PNG_1X1,
        media_type: "image/png",
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const proposalId = filed.structuredContent.proposal_id as number;

      await page.goto(`/proposals?status=pending&kind=cover`);
      const coverRow = page.locator(`tr[data-proposal-id="${proposalId}"]`);
      // Story 12-4: a pending cover row shows its thumbnail.
      const thumb = coverRow.locator(`img[src="/proposals/${proposalId}/cover"]`);
      await expect(thumb).toBeVisible();
      await expect
        .poll(() => thumb.evaluate((el) => (el as HTMLImageElement).naturalWidth))
        .toBeGreaterThan(0);
      // Story 12-4 (AC4): the list with its new rows passes axe.
      await checkAccessibility(page);
      await coverRow.getByRole("link", { name: openName(proposalId) }).click();
      const img = page.locator("#proposal-staged-cover");
      await expect(img).toBeVisible();
      await expect
        .poll(() => img.evaluate((el) => (el as HTMLImageElement).naturalWidth))
        .toBeGreaterThan(0);

      await rejectFromDetail(page, "");
      await expect(page.locator("#proposal-staged-cover")).toHaveCount(0);
      const res = await page.request.get(`/proposals/${proposalId}/cover`);
      expect(res.status()).toBe(404);
    } finally {
      await agent.dispose();
      await revoke(page, key.label);
    }
  });
});
