/**
 * Story 11-20 E2E — the side-by-side view of metadata and cover
 * proposals (FR137, NFR47, UX-DR31).
 *
 * Journey 1, from a blank browser: an admin creates a `propose` key →
 * cookies cleared → loginAs(librarian) → an ISBN scanned → the agent
 * files a metadata proposal (a subtitle the title lacks, a publisher
 * that differs) → the detail compares current and proposed values in
 * tables with "new" / "changed" text badges → the librarian edits the
 * subtitle → the comparison shows the edited value and the agent's
 * original. axe passes; no horizontal scroll at phone width.
 *
 * Journey 2: the agent files a cover for a title without one → current
 * (placeholder) and proposed image side by side, flagged "adds" → the
 * librarian accepts it → a second cover proposal shows the current
 * cover next to the proposed one, flagged "replaces". axe passes; no
 * horizontal scroll at phone width.
 *
 * Spec ID "SB". Creates no series (#528). The key is revoked at the end
 * (its pending proposals become outdated).
 */
import { test, expect, request as playwrightRequest, Page, Locator } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { checkAccessibility } from "../../helpers/accessibility";
import { mcpCallTool } from "../../helpers/mcp";
import {
  acceptFromDetail,
  newProposeKey,
  revokeProposeKey,
  scanSettledTitle,
} from "../../helpers/proposals";

const SOURCE = "https://example.org/notice-sb";
const EDIT = /^(Edit|Modifier|Bearbeiten|Modifica) —/;
const SAVE = /^(Save|Enregistrer|Speichern|Salva)$/;
const UPDATED =
  /Line of proposal #\d+ updated|Ligne de la proposition n° \d+ modifiée|Zeile von Vorschlag Nr\. \d+ geändert|Riga della proposta n\. \d+ modificata/i;
const NEW_BADGE = /^(New|Nouveau|Neu|Nuovo)$/;
const CHANGED_BADGE = /^(Changed|Modifié|Geändert|Modificato)$/;
const CURRENT = /^(Current|Actuel|Aktuell|Attuale)$/;
const PROPOSED = /^(Proposed|Proposé|Vorgeschlagen|Proposto)$/;
const ORIGINAL = /The assistant proposed:|L'assistant proposait :|Der Assistent schlug vor:|L'assistente proponeva:/;
const ADDS = /accepting adds this one|accepter ajoute celle-ci|Annehmen fügt dieses hinzu|accettare aggiunge questa/;
const REPLACES =
  /Accepting replaces the current cover|Accepter remplace la couverture actuelle|Annehmen ersetzt das aktuelle Cover|Accettare sostituisce la copertina attuale/;
/** A valid 1×1 RGBA PNG. */
const PNG_1X1 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

/** At phone width the page does not scroll horizontally. */
async function expectNoHorizontalScroll(page: Page) {
  await page.setViewportSize({ width: 360, height: 740 });
  await expect
    .poll(() =>
      page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      ),
    )
    .toBeLessThanOrEqual(1);
  await page.setViewportSize({ width: 1280, height: 720 });
}

/** An image has loaded (the authenticated route answered a decodable file). */
async function expectLoaded(img: Locator) {
  await expect(img).toBeVisible();
  await expect.poll(() => img.evaluate((el) => (el as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
}

test.describe("Story 11-20 — side-by-side view for metadata and covers", () => {
  test.setTimeout(90000);

  test("metadata is compared side by side and an edited line shows the agent's original", async ({
    page,
    baseURL,
  }) => {
    const key = await newProposeKey(page, "side-meta");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const titleId = await scanSettledTitle(page, agent, key.plaintext, "SB", 1);
      const filed = await mcpCallTool(agent, key.plaintext, "propose_title_metadata", {
        title_id: titleId,
        fields: { subtitle: "Sous-titre <i>SB</i>", publisher: "Éditeur SB" },
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const id = filed.structuredContent.proposal_id as number;

      await page.goto(`/proposals/${id}`);
      const tables = page.locator("table[data-compare]");
      await expect(tables).toHaveCount(2);

      // Subtitle: empty today, so "new"; the agent's markup is text.
      const subtitle = tables.nth(0);
      await expect(subtitle.locator("caption")).toContainText(/Subtitle|Sous-titre|Untertitel|Sottotitolo/);
      await expect(subtitle.locator("[data-change]")).toHaveText(NEW_BADGE);
      await expect(subtitle.locator("th").nth(0)).toHaveText(CURRENT);
      await expect(subtitle.locator("th").nth(1)).toHaveText(PROPOSED);
      await expect(subtitle.locator("[data-proposed]")).toHaveText("Sous-titre <i>SB</i>");
      await expect(subtitle.locator("i")).toHaveCount(0);

      // Publisher: the current value next to the different proposed one.
      const publisher = tables.nth(1);
      await expect(publisher.locator("[data-change]")).toHaveText(CHANGED_BADGE);
      await expect(publisher.locator("[data-current]")).toHaveText("Test Publisher");
      await expect(publisher.locator("[data-proposed]")).toHaveText("Éditeur SB");
      await checkAccessibility(page);

      // The librarian corrects the subtitle: edited value + agent's original.
      const line = page.locator("#proposal-items > li").first();
      await line.getByRole("button", { name: EDIT }).click();
      const form = line.locator("[data-edit-slot] form");
      await expect(form).toBeVisible();
      await form.locator("input[name='value']").fill("Sous-titre corrigé SB");
      await form.getByRole("button", { name: SAVE }).click();
      await expect(
        page.locator("#feedback-list .feedback-entry").filter({ hasText: UPDATED }).last(),
      ).toBeVisible({ timeout: 10000 });
      const edited = page.locator("table[data-compare]").first();
      await expect(edited.locator("[data-proposed]")).toHaveText("Sous-titre corrigé SB");
      await expect(edited.locator("[data-original]")).toContainText(ORIGINAL);
      await expect(edited.locator("[data-original]")).toContainText("Sous-titre <i>SB</i>");
      await expect(page.locator("table[data-compare]").nth(1).locator("[data-original]")).toHaveCount(0);
      await checkAccessibility(page);
      await expectNoHorizontalScroll(page);
    } finally {
      await agent.dispose();
    }
    await revokeProposeKey(page, key.label);
  });

  test("a cover is compared with the placeholder, then with the current cover", async ({
    page,
    baseURL,
  }) => {
    const key = await newProposeKey(page, "side-cover");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const titleId = await scanSettledTitle(page, agent, key.plaintext, "SB", 2);
      const cover = { title_id: titleId, image_base64: PNG_1X1, media_type: "image/png", sources: [SOURCE] };

      const first = await mcpCallTool(agent, key.plaintext, "propose_cover", cover);
      expect(first.isError, JSON.stringify(first.content)).toBe(false);
      const firstId = first.structuredContent.proposal_id as number;
      await page.goto(`/proposals/${firstId}`);
      const compare = page.locator("[data-cover-compare]");
      await expect(compare.locator("#proposal-current-cover-placeholder")).toBeVisible();
      await expect(compare.locator("#proposal-current-cover")).toHaveCount(0);
      const staged = compare.locator("#proposal-staged-cover");
      await expect(staged).toHaveAttribute("src", `/proposals/${firstId}/cover`);
      await expectLoaded(staged);
      await expect(page.locator("[data-cover-flag]")).toHaveText(ADDS);
      await checkAccessibility(page);
      await expectNoHorizontalScroll(page);

      // Accepted: the title has a cover now; a second proposal replaces it.
      await acceptFromDetail(page);
      const second = await mcpCallTool(agent, key.plaintext, "propose_cover", cover);
      expect(second.isError, JSON.stringify(second.content)).toBe(false);
      const secondId = second.structuredContent.proposal_id as number;
      await page.goto(`/proposals/${secondId}`);
      const current = page.locator("[data-cover-compare] #proposal-current-cover");
      await expect(current).toHaveAttribute("src", new RegExp(`^/covers/${titleId}\\.jpg\\?v=\\d+$`));
      await expectLoaded(current);
      await expectLoaded(page.locator("[data-cover-compare] #proposal-staged-cover"));
      await expect(page.locator("#proposal-current-cover-placeholder")).toHaveCount(0);
      await expect(page.locator("[data-cover-flag]")).toHaveText(REPLACES);
      await checkAccessibility(page);
      await expectNoHorizontalScroll(page);
    } finally {
      await agent.dispose();
    }
    await revokeProposeKey(page, key.label);
  });
});
