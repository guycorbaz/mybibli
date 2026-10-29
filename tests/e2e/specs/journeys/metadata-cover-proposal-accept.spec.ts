/**
 * Story 11-18 E2E — accepting a metadata or cover proposal (FR138,
 * FR140, FR144, FR146).
 *
 * Journey 1, from a blank browser: an admin creates a `propose` key →
 * cookies cleared → loginAs(librarian) → an ISBN scanned → the agent
 * files a metadata proposal (subtitle, Dewey code, a contributor) over
 * MCP → the librarian accepts it through the UX-DR8 modal → the agent
 * files a cover proposal → the librarian accepts it → "Accepted",
 * success entries → the title page shows the new subtitle, Dewey code,
 * contributor and the proposed cover; the accepted cover proposal shows
 * the cover now installed. Journey 3: #531 on the title page.
 *
 * Journey 2 (stale): the agent files two cover proposals and a metadata
 * proposal for one title → the librarian accepts the first cover → the
 * second cover and the metadata proposal are now outdated: accepting
 * them says so and changes nothing.
 *
 * Spec ID "MA". Creates no series (#528); every proposal targets this
 * spec's own titles. The key is revoked at the end.
 */
import { test, expect, request as playwrightRequest, Page, APIRequestContext } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { selectFirstRealOption, titleIdFromSkeleton } from "../../helpers/catalog";
import { createApiKey, revokeApiKey } from "../../helpers/api-keys";
import { mcpCallTool } from "../../helpers/mcp";

const SOURCE = "https://example.org/notice-ma";
const ACCEPT = /^(Accept|Accepter|Annehmen|Accetta)$/;
const ACCEPTED =
  /Proposal #\d+ accepted|Proposition n° \d+ acceptée|Vorschlag Nr\. \d+ angenommen|Proposta n\. \d+ accettata/i;
const COVER_SHOWN =
  /now shows the proposed cover|affiche maintenant la couverture proposée|zeigt jetzt das vorgeschlagene Cover|mostra ora la copertina proposta/i;
const OUTDATED =
  /is now outdated and nothing was changed|est désormais périmée et rien n'a été modifié|ist jetzt veraltet|è ora superata/i;
/** A valid 1×1 RGBA PNG. */
const PNG_1X1 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

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
 *  bumps the title's version). Returns the title id. */
async function scanSettledTitle(
  page: Page,
  agent: APIRequestContext,
  key: string,
  seq: number,
): Promise<number> {
  await page.goto("/catalog");
  const scanField = page.locator("#scan-field");
  await scanField.fill(specIsbn("MA", seq));
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

async function fileOk(
  agent: APIRequestContext,
  key: string,
  tool: string,
  args: Record<string, unknown>,
): Promise<number> {
  const filed = await mcpCallTool(agent, key, tool, { ...args, sources: [SOURCE] });
  expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
  return filed.structuredContent.proposal_id as number;
}

/** Open a proposal and accept it through the modal. */
async function acceptProposal(page: Page, id: number) {
  await page.goto(`/proposals/${id}`);
  await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
    "data-proposal-status",
    "pending",
  );
  await page.locator("#proposal-accept-button").click();
  const dialog = page.locator("#modal-slot dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.locator("[data-modal-confirm]")).toHaveText(ACCEPT);
  await dialog.locator("[data-modal-confirm]").click();
  await expect(dialog).toBeHidden({ timeout: 10000 });
}

test.describe("Story 11-18 — accept a metadata or cover proposal", () => {
  test.setTimeout(90000);

  test("a librarian accepts the agent's metadata and cover and sees them on the title page", async ({
    page,
    baseURL,
  }) => {
    const key = await newKey(page, "accept-meta");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const titleId = await scanSettledTitle(page, agent, key.plaintext, 1);

      const contributor = `Illustratrice MA ${Date.now()}`;
      const metaId = await fileOk(agent, key.plaintext, "propose_title_metadata", {
        title_id: titleId,
        fields: {
          subtitle: "Édition revue MA",
          dewey_code: "741.5",
          contributors: [{ name: contributor, role: "Illustrateur" }],
        },
      });

      // Metadata: the modal names the fields, then the decision.
      await page.goto(`/proposals/${metaId}`);
      await page.locator("#proposal-accept-button").click();
      const dialog = page.locator("#modal-slot dialog");
      await expect(dialog).toContainText(/Subtitle|Sous-titre|Untertitel|Sottotitolo/);
      await dialog.getByRole("button", { name: /Cancel|Annuler|Abbrechen|Annulla/i }).click();
      await expect(dialog).toBeHidden();
      await acceptProposal(page, metaId);
      await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
        "data-proposal-status",
        "accepted",
      );
      await expect(page.locator("#proposal-decided-by")).toHaveText("librarian");
      await expect(
        page.locator("#feedback-list .feedback-entry").filter({ hasText: ACCEPTED }),
      ).toBeVisible({ timeout: 10000 });

      // Cover: filed once the metadata is in (accepting a proposal bumps
      // the title's version, which makes proposals filed before it
      // outdated — journey 2), accepted, and the detail shows the cover.
      const coverId = await fileOk(agent, key.plaintext, "propose_cover", {
        title_id: titleId,
        image_base64: PNG_1X1,
        media_type: "image/png",
      });
      await acceptProposal(page, coverId);
      await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
        "data-proposal-status",
        "accepted",
      );
      await expect(
        page.locator("#feedback-list .feedback-entry").filter({ hasText: COVER_SHOWN }),
      ).toBeVisible({ timeout: 10000 });
      await expect(page.locator("#proposal-accepted-cover")).toHaveAttribute(
        "src",
        new RegExp(`^/covers/${titleId}\\.jpg\\?v=\\d+$`),
      );

      // The title page shows everything.
      await page.goto(`/title/${titleId}`);
      await expect(page.locator("main")).toContainText("Édition revue MA");
      await expect(page.locator("main")).toContainText("741.5");
      await expect(page.locator("#contributor-list")).toContainText(contributor);
      const cover = page.locator(`img[src^="/covers/${titleId}.jpg?v="]`);
      await expect(cover).toBeVisible();
      const res = await page.request.get((await cover.getAttribute("src"))!);
      expect(res.status()).toBe(200);
      expect(res.headers()["content-type"]).toContain("image/jpeg");
    } finally {
      await agent.dispose();
    }
    await revoke(page, key.label);
  });

  test("proposals made outdated by an accepted cover change nothing", async ({ page, baseURL }) => {
    const key = await newKey(page, "accept-stale");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const titleId = await scanSettledTitle(page, agent, key.plaintext, 2);
      const cover = { title_id: titleId, image_base64: PNG_1X1, media_type: "image/png" };
      const first = await fileOk(agent, key.plaintext, "propose_cover", cover);
      const second = await fileOk(agent, key.plaintext, "propose_cover", cover);
      const meta = await fileOk(agent, key.plaintext, "propose_title_metadata", {
        title_id: titleId,
        fields: { subtitle: "Jamais appliqué MA" },
      });

      await acceptProposal(page, first);
      await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
        "data-proposal-status",
        "accepted",
      );

      for (const id of [second, meta]) {
        await acceptProposal(page, id);
        await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
          "data-proposal-status",
          "stale",
        );
        await expect(
          page.locator("#feedback-list .feedback-entry").filter({ hasText: OUTDATED }),
        ).toBeVisible({ timeout: 10000 });
        await expect(page.locator("#proposal-accept-button")).toHaveCount(0);
      }

      await page.goto(`/title/${titleId}`);
      await expect(page.locator("main")).not.toContainText("Jamais appliqué MA");
    } finally {
      await agent.dispose();
    }
    await revoke(page, key.label);
  });

  test("#531 — a contributor removed from a title can be added back", async ({ page }) => {
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto("/catalog");
    const scanField = page.locator("#scan-field");
    await scanField.fill(specIsbn("MA", 3));
    await scanField.press("Enter");
    const titleId = Number(await titleIdFromSkeleton(page));
    expect(titleId).toBeGreaterThan(0);
    await page.goto(`/title/${titleId}`);

    const name = `Retour MA ${Date.now()}`;
    const list = page.locator("#contributor-list");
    const add = async () => {
      await page.locator(`button[hx-get="/title/${titleId}/contributor-form"]`).click();
      const form = page.locator("#contributor-form-slot form");
      await expect(form).toBeVisible();
      await form.locator("#contributor-name-input").fill(name);
      await selectFirstRealOption(form.locator("#contributor-role-select"));
      await form.locator('button[type="submit"]').click();
      await expect(list).toContainText(name, { timeout: 10000 });
    };

    await add();
    await list.locator(`button[aria-label$="${name}"]`).click();
    await expect(list).not.toContainText(name, { timeout: 10000 });

    // Added back: no "already has this role" error, the credit is shown.
    await add();
    await expect(page.locator('.feedback-entry[data-feedback-variant="error"]')).toHaveCount(0);
    await page.reload();
    await expect(page.locator("#contributor-list")).toContainText(name);
  });
});
