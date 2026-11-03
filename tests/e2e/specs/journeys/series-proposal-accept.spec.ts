/**
 * Story 11-17 E2E — accepting a series proposal (FR138, FR140, FR146).
 *
 * Journey 1, from a blank browser: an admin creates a `propose` key →
 * cookies cleared → loginAs(librarian) → two ISBNs scanned → the agent
 * files a `series_create` over MCP (two titles, two expected tomes) →
 * the librarian opens the proposal, accepts it through the UX-DR8 modal
 * → "Accepted", the decider, a success entry → the series page shows
 * both titles and both expected tomes.
 *
 * Journey 2 (stale): the agent files a `series_attach` for a series the
 * librarian then renames → accepting says the proposal is outdated and
 * nothing was added; the series page shows no expected tome.
 *
 * Journey 3 (story 13-11, #588): the librarian declares an expected
 * tome without ISBN at position 2 → the agent proposes the scanned
 * (owned) title at that position — no longer refused → the librarian
 * accepts → the series page shows the title at position 2 and no
 * expected tome left.
 *
 * Spec ID "SA". The file name sorts after `classification-fieldset`,
 * `dewey-code` and `empty-states`, which assume no series exists
 * (D-11-11-6, #528). Series names carry a per-run stamp.
 */
import { test, expect, request as playwrightRequest, Page, APIRequestContext } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { mcpCallTool } from "../../helpers/mcp";

const SOURCE = "https://example.org/serie";
const ACCEPT = /^(Accept|Accepter|Annehmen|Accetta)$/;
const ACCEPTED =
  /Proposal #\d+ accepted|Proposition n° \d+ acceptée|Vorschlag Nr\. \d+ angenommen|Proposta n\. \d+ accettata/i;
const OUTDATED =
  /is now outdated and nothing was added|est désormais périmée et rien n'a été ajouté|ist jetzt veraltet|è ora superata/i;

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

/** Scan a fresh ISBN; wait for its background metadata fetch (which
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
  await scanField.fill(specIsbn("SA", seq));
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
  await expect(dialog.locator("[data-modal-confirm]")).toHaveText(ACCEPT);
  await dialog.locator("[data-modal-confirm]").click();
  await expect(dialog).toBeHidden({ timeout: 10000 });
}

test.describe("Story 11-17 — accept a series proposal", () => {
  test.setTimeout(90000);

  test("a librarian accepts a series created by the agent and finds it on the series page", async ({
    page,
    baseURL,
  }) => {
    const key = await newKey(page, "accept");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const t1 = await scanSettledTitle(page, agent, key.plaintext, 1);
      const t2 = await scanSettledTitle(page, agent, key.plaintext, 2);

      const name = `SA-Created-${Date.now()}`;
      const filed = await mcpCallTool(agent, key.plaintext, "propose_series_create", {
        name,
        series_type: "closed",
        total_volume_count: 4,
        titles: [
          { title_id: t1, position: 1 },
          { title_id: t2, position: 2 },
        ],
        expected_tomes: [
          { position: 3, title: "SA tome trois" },
          { position: 4, title: "SA tome quatre" },
        ],
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const proposalId = filed.structuredContent.proposal_id as number;

      await page.goto(`/proposals/${proposalId}`);
      await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
        "data-proposal-status",
        "pending",
      );
      await page.locator("#proposal-accept-button").click();
      const dialog = page.locator("#modal-slot dialog");
      await expect(dialog).toContainText(name);
      await dialog.getByRole("button", { name: /Cancel|Annuler|Abbrechen|Annulla/i }).click();
      await expect(dialog).toBeHidden();

      await acceptFromDetail(page);
      const body = page.locator("#proposal-detail-body");
      await expect(body).toHaveAttribute("data-proposal-status", "accepted");
      await expect(page.locator("#proposal-decided-by")).toHaveText("librarian");
      await expect(page.locator("#proposal-accept-button")).toHaveCount(0);
      await expect(page.locator("#proposal-reject-button")).toHaveCount(0);
      const entry = page.locator("#feedback-list .feedback-entry").filter({ hasText: ACCEPTED });
      await expect(entry).toBeVisible({ timeout: 10000 });

      // The subject now links the created series.
      await body.locator(`a[href^="/series/"]`).first().click();
      await page.waitForURL(/\/series\/\d+$/);
      await expect(page.locator("h1")).toContainText(name);
      await expect(page.locator(`a[href="/title/${t1}"]`).first()).toBeVisible();
      await expect(page.locator(`a[href="/title/${t2}"]`).first()).toBeVisible();
      await expect(page.getByText("SA tome trois")).toBeVisible();
      await expect(page.getByText("SA tome quatre")).toBeVisible();

      // Reloaded, the proposal keeps its decision.
      await page.goto(`/proposals/${proposalId}`);
      await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
        "data-proposal-status",
        "accepted",
      );
    } finally {
      await agent.dispose();
      await revoke(page, key.label);
    }
  });

  test("a proposal whose series changed since filing is marked outdated and changes nothing", async ({
    page,
    baseURL,
  }) => {
    const key = await newKey(page, "accept-stale");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const name = `SA-Stale-${Date.now()}`;
      await page.goto("/series/new");
      await page.locator("#series-name").fill(name);
      await page.locator("#series-type").selectOption("closed");
      await page.locator("#series-total").fill("3");
      await page.getByRole("button", { name: /^(Save|Enregistrer|Speichern|Salva)$/i }).click();
      await page.waitForURL(/\/series\/\d+$/);
      const seriesId = Number(page.url().match(/\/series\/(\d+)$/)![1]);

      const filed = await mcpCallTool(agent, key.plaintext, "propose_series_attach", {
        series_id: seriesId,
        expected_tomes: [{ position: 2, title: "SA tome jamais ajouté" }],
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const proposalId = filed.structuredContent.proposal_id as number;

      // The librarian renames the series meanwhile.
      await page.goto(`/series/${seriesId}/edit`);
      await page.locator("#series-name").fill(`${name}-renamed`);
      await page.getByRole("button", { name: /^(Save|Enregistrer|Speichern|Salva)$/i }).click();
      await page.waitForURL(/\/series\/\d+$/);

      await page.goto(`/proposals/${proposalId}`);
      await acceptFromDetail(page);
      await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
        "data-proposal-status",
        "stale",
      );
      await expect(
        page.locator("#feedback-list .feedback-entry").filter({ hasText: OUTDATED }),
      ).toBeVisible({ timeout: 10000 });
      await expect(page.locator("#proposal-accept-button")).toHaveCount(0);

      await page.goto(`/series/${seriesId}`);
      await expect(page.locator("h1")).toContainText(`${name}-renamed`);
      await expect(page.getByText("SA tome jamais ajouté")).toHaveCount(0);
    } finally {
      await agent.dispose();
      await revoke(page, key.label);
    }
  });

  test("an owned title proposed at an expected position fills that tome", async ({
    page,
    baseURL,
  }) => {
    const key = await newKey(page, "accept-fill");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const titleId = await scanSettledTitle(page, agent, key.plaintext, 3);

      const name = `SA-Fill-${Date.now()}`;
      await page.goto("/series/new");
      await page.locator("#series-name").fill(name);
      await page.locator("#series-type").selectOption("closed");
      await page.locator("#series-total").fill("3");
      await page.getByRole("button", { name: /^(Save|Enregistrer|Speichern|Salva)$/i }).click();
      await page.waitForURL(/\/series\/\d+$/);
      const seriesId = Number(page.url().match(/\/series\/(\d+)$/)![1]);

      // The librarian declares tome 2 (no ISBN).
      await page.locator("#add-expected-tome").click();
      await page.waitForURL(/\/expected-tomes\/new$/);
      await page.locator("#expected-tome-position").fill("2");
      await page.locator("#expected-tome-title").fill("SA tome deux attendu");
      await page.locator('main button[type="submit"]').last().click();
      await page.waitForURL(/\/series\/\d+$/);
      await expect(
        page.locator("[data-expected-tome]").filter({ hasText: "SA tome deux attendu" }),
      ).toBeVisible();

      // The agent proposes the owned title at that position: filed.
      const filed = await mcpCallTool(agent, key.plaintext, "propose_series_attach", {
        series_id: seriesId,
        titles: [{ title_id: titleId, position: 2 }],
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const proposalId = filed.structuredContent.proposal_id as number;

      await page.goto(`/proposals/${proposalId}`);
      await acceptFromDetail(page);
      await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
        "data-proposal-status",
        "accepted",
      );
      await expect(
        page.locator("#feedback-list .feedback-entry").filter({ hasText: ACCEPTED }),
      ).toBeVisible({ timeout: 10000 });

      // The tome is owned: the title holds position 2, no expected tome left.
      await page.goto(`/series/${seriesId}`);
      await expect(page.locator(`a[href="/title/${titleId}"]`).first()).toBeVisible();
      await expect(page.locator("[data-expected-tome]")).toHaveCount(0);
      const detail = await mcpCallTool(agent, key.plaintext, "get_series", { id: seriesId });
      const tomes = detail.structuredContent.expected_tomes as {
        position: number;
        settled_title_id: number | null;
      }[];
      expect(tomes.map((t) => [t.position, t.settled_title_id])).toEqual([[2, titleId]]);
    } finally {
      await agent.dispose();
      await revoke(page, key.label);
    }
  });
});
