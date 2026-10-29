/**
 * Story 11-19 E2E — editing a proposal before accepting it (FR139).
 *
 * Journey 1, from a blank browser: an admin creates a `propose` key →
 * cookies cleared → loginAs(librarian) → two ISBNs scanned → the agent
 * files a `series_create` over MCP (two titles, two expected tomes) →
 * the librarian moves a title to another position with the line's
 * inline form (inline-form.js), removes an expected tome through the
 * UX-DR8 modal, accepts → the series holds the title at its new
 * position, the kept tome, not the removed one; the proposal keeps the
 * trail (edited line, removed line).
 *
 * Journey 2: the agent files a metadata proposal; the librarian
 * corrects the proposed subtitle, accepts → the title page shows the
 * corrected subtitle.
 *
 * Spec ID "PE". The file name sorts after `classification-fieldset`,
 * `dewey-code` and `empty-states`, which assume no series exists
 * (D-11-11-6, #528). Series names carry a per-run stamp.
 */
import { test, expect, request as playwrightRequest, Page, APIRequestContext, Locator } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { mcpCallTool } from "../../helpers/mcp";

const SOURCE = "https://example.org/edit";
const EDIT = /^(Edit|Modifier|Bearbeiten|Modifica)$/;
const REMOVE = /^(Remove|Retirer|Entfernen|Rimuovi)$/;
const SAVE = /^(Save|Enregistrer|Speichern|Salva)$/;
const UPDATED =
  /Line of proposal #\d+ updated|Ligne de la proposition n° \d+ modifiée|Zeile von Vorschlag Nr\. \d+ geändert|Riga della proposta n\. \d+ modificata/i;
const REMOVED =
  /Line removed from proposal|Ligne retirée de la proposition|Zeile aus Vorschlag|Riga rimossa dalla proposta/i;
const EDITED_BY = /Edited by librarian|Modifiée par librarian|Bearbeitet von librarian|Modificata da librarian/;
const ACCEPTED =
  /Proposal #\d+ accepted|Proposition n° \d+ acceptée|Vorschlag Nr\. \d+ angenommen|Proposta n\. \d+ accettata/i;

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

/** Scan a fresh ISBN and wait for its background metadata fetch (it
 *  bumps the title's version) so the proposal is filed on the final
 *  version. Returns the title id. */
async function scanSettledTitle(
  page: Page,
  agent: APIRequestContext,
  key: string,
  seq: number,
): Promise<number> {
  await page.goto("/catalog");
  const scanField = page.locator("#scan-field");
  await scanField.fill(specIsbn("PE", seq));
  await scanField.press("Enter");
  const titleId = Number(await titleIdFromSkeleton(page));
  expect(titleId).toBeGreaterThan(0);
  await expect
    .poll(
      async () =>
        (await mcpCallTool(agent, key, "get_title", { id: titleId })).structuredContent
          .publisher,
      { timeout: 15000 },
    )
    .toBe("Test Publisher");
  return titleId;
}

async function acceptFromDetail(page: Page) {
  await page.locator("#proposal-accept-button").click();
  const dialog = page.locator("#modal-slot dialog");
  await expect(dialog).toBeVisible();
  await dialog.locator("[data-modal-confirm]").click();
  await expect(dialog).toBeHidden({ timeout: 10000 });
  await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
    "data-proposal-status",
    "accepted",
  );
  await expect(
    page.locator("#feedback-list .feedback-entry").filter({ hasText: ACCEPTED }),
  ).toBeVisible({ timeout: 10000 });
}

/** Open a line's inline form, run `fill`, save, wait for the update. */
async function editLine(page: Page, line: Locator, fill: (form: Locator) => Promise<void>) {
  await line.getByRole("button", { name: EDIT }).click();
  const form = line.locator("[data-edit-slot] form");
  await expect(form).toBeVisible();
  await fill(form);
  await form.getByRole("button", { name: SAVE }).click();
  await expect(
    page.locator("#feedback-list .feedback-entry").filter({ hasText: UPDATED }).last(),
  ).toBeVisible({ timeout: 10000 });
}

test.describe("Story 11-19 — edit a proposal before accepting it", () => {
  test.setTimeout(90000);

  test("a librarian moves a title, removes a tome, accepts and finds the series as edited", async ({
    page,
    baseURL,
  }) => {
    const key = await newKey(page, "edit");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const t1 = await scanSettledTitle(page, agent, key.plaintext, 1);
      const t2 = await scanSettledTitle(page, agent, key.plaintext, 2);

      const name = `PE-Edited-${Date.now()}`;
      const filed = await mcpCallTool(agent, key.plaintext, "propose_series_create", {
        name,
        series_type: "closed",
        total_volume_count: 6,
        titles: [
          { title_id: t1, position: 1 },
          { title_id: t2, position: 2 },
        ],
        expected_tomes: [
          { position: 3, title: "PE tome gardé" },
          { position: 4, title: "PE tome retiré" },
        ],
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const proposalId = filed.structuredContent.proposal_id as number;

      await page.goto(`/proposals/${proposalId}`);
      const items = page.locator("#proposal-items > li");

      // Move the first title from position 1 to position 5.
      const titleLine = items.filter({ has: page.locator(`a[href="/title/${t1}"]`) });
      await editLine(page, titleLine, async (form) => {
        await form.getByLabel(/^(Position|Posizione)$/).fill("5");
      });
      const movedLine = items.filter({ has: page.locator(`a[href="/title/${t1}"]`) });
      await expect(movedLine).toContainText(/5/);
      await expect(movedLine.locator("[data-edit-note]")).toContainText(EDITED_BY);

      // Cancel leaves the form closed and unchanged.
      await movedLine.getByRole("button", { name: EDIT }).click();
      await expect(movedLine.locator("[data-edit-slot]")).toBeVisible();
      await movedLine
        .locator("[data-edit-slot]")
        .getByRole("button", { name: /^(Cancel|Annuler|Abbrechen|Annulla)$/ })
        .click();
      await expect(movedLine.locator("[data-edit-slot]")).toBeHidden();

      // Remove the second expected tome through the modal.
      const tomeLine = items.filter({ hasText: "PE tome retiré" });
      await tomeLine.getByRole("button", { name: REMOVE }).click();
      const dialog = page.locator("#modal-slot dialog");
      await expect(dialog).toBeVisible();
      await expect(dialog).toContainText("PE tome retiré");
      await dialog.locator("[data-modal-confirm]").click();
      await expect(dialog).toBeHidden({ timeout: 10000 });
      await expect(
        page.locator("#feedback-list .feedback-entry").filter({ hasText: REMOVED }),
      ).toBeVisible({ timeout: 10000 });
      await expect(items.filter({ hasText: "PE tome retiré" })).toHaveCount(0);
      await expect(page.locator("#proposal-removed")).toContainText("PE tome retiré");

      await acceptFromDetail(page);

      // The series holds what was accepted, as edited.
      await page.locator("#proposal-detail-body").locator(`a[href^="/series/"]`).first().click();
      await page.waitForURL(/\/series\/\d+$/);
      const seriesId = Number(page.url().match(/\/series\/(\d+)$/)![1]);
      await expect(page.locator("h1")).toContainText(name);
      await expect(page.getByText("PE tome gardé")).toBeVisible();
      await expect(page.getByText("PE tome retiré")).toHaveCount(0);
      const detail = (await mcpCallTool(agent, key.plaintext, "get_series", { id: seriesId }))
        .structuredContent;
      const positions = (detail.positions as { title_id: number; position: number }[])
        .map((p): [number, number] => [p.title_id, p.position])
        .sort((a, b) => a[1] - b[1]);
      expect(positions).toEqual([
        [t2, 2],
        [t1, 5],
      ]);
      expect((detail.expected_tomes as { title: string }[]).map((t) => t.title)).toEqual([
        "PE tome gardé",
      ]);

      // The decided proposal keeps the trail.
      await page.goto(`/proposals/${proposalId}`);
      await expect(page.locator("[data-edit-note]").first()).toContainText(EDITED_BY);
      await expect(page.locator("#proposal-removed")).toContainText("PE tome retiré");
      await expect(page.getByRole("button", { name: EDIT })).toHaveCount(0);
    } finally {
      await agent.dispose();
      await revoke(page, key.label);
    }
  });

  test("a librarian corrects a proposed subtitle and the title page shows the correction", async ({
    page,
    baseURL,
  }) => {
    const key = await newKey(page, "edit-meta");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const titleId = await scanSettledTitle(page, agent, key.plaintext, 3);
      const filed = await mcpCallTool(agent, key.plaintext, "propose_title_metadata", {
        title_id: titleId,
        fields: { subtitle: "Sous-titre de l'agent PE" },
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const proposalId = filed.structuredContent.proposal_id as number;

      await page.goto(`/proposals/${proposalId}`);
      const line = page.locator("#proposal-items > li").first();
      await editLine(page, line, async (form) => {
        await form.locator("input[name='value']").fill("Sous-titre corrigé PE");
      });
      await expect(page.locator("#proposal-items > li").first().locator("[data-proposed]")).toHaveText(
        "Sous-titre corrigé PE",
      );
      await expect(page.locator("[data-edit-note]")).toContainText("Sous-titre de l'agent PE");

      await acceptFromDetail(page);
      await page.goto(`/title/${titleId}`);
      await expect(page.locator("main")).toContainText("Sous-titre corrigé PE");
      await expect(page.locator("main")).not.toContainText("Sous-titre de l'agent PE");
    } finally {
      await agent.dispose();
      await revoke(page, key.label);
    }
  });
});
