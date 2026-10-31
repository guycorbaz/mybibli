/**
 * Story 12-6 — Epic 12 smoke test (Foundation Rule #7): the epic's core
 * journey, end to end, from a blank browser. If this test fails, epic 12
 * is not done.
 *
 * Blank context → loginAs(admin) → a `propose` key created in Admin → API
 * keys → cookies cleared → loginAs(librarian) → an ISBN **typed with
 * hyphens** on /catalog creates the title under its bare ISBN-13 (12.1) →
 * a scripted MCP client (fresh request context) opens with `initialize`:
 * the instructions name the sources mybibli already queries (12.3) → it
 * files a metadata proposal correcting the **title** and adding a
 * description (12.5), and a cover proposal → on /proposals the cover row
 * shows its thumbnail and the metadata row its proposed fields; the
 * subject opens the proposal (12.4) → the librarian accepts the cover,
 * then the metadata proposal, which is **not outdated** (12.2) → the title
 * page shows the corrected title and the installed cover → the key is
 * revoked (afterEach, fresh context).
 *
 * Spec ID "EZ"; the ISBN sequence carries a per-run stamp, so a re-run on
 * a non-reset stack meets none of an earlier run's titles. Creates no
 * series (#528); nothing is rejected (no resubmission delay applies).
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { revokeApiKey } from "../../helpers/api-keys";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { mcpCallTool, mcpPost } from "../../helpers/mcp";
import { acceptFromDetail, newProposeKey } from "../../helpers/proposals";

const SPEC = "EZ";
// Seconds stamp, kept small enough for specIsbn's five-digit sequence.
const RUN = Math.floor(Date.now() / 1000) % 90000;
const SOURCE = "https://example.org/epic12-smoke";
/** A valid 1×1 RGBA PNG. */
const PNG_1X1 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
const PROPOSED_FIELDS =
  /(Proposed|Proposé|Vorgeschlagen|Proposto) ?: .*(Title|Titre|Titel|Titolo).*(Description|Beschreibung|Descrizione)/;
const PROPOSAL_LINK = (id: number) =>
  new RegExp(`(proposal #|proposition n° |Vorschlag Nr\\. |proposta n\\. )${id}$`);

/** `9782205001211` → `978-2-205-00121-1`. */
function hyphenated(isbn13: string): string {
  return [isbn13.slice(0, 3), isbn13.slice(3, 4), isbn13.slice(4, 7), isbn13.slice(7, 12), isbn13.slice(12)].join("-");
}

test.describe("Epic 12 smoke — proposals, after a first real use", () => {
  test.setTimeout(120000);

  let keyLabel: string | null = null;
  test.afterEach(async ({ browser }) => {
    if (keyLabel === null) return;
    const context = await browser.newContext();
    try {
      const admin = await context.newPage();
      await loginAs(admin, "admin");
      await admin.goto("/admin?tab=api_keys");
      await revokeApiKey(admin, keyLabel);
    } finally {
      keyLabel = null;
      await context.close();
    }
  });

  test("hyphenated ISBN → agent instructions → title + cover proposals → readable list → both accepted", async ({
    page,
    baseURL,
  }) => {
    // A blank browser: no cookie, nothing injected.
    expect(await page.context().cookies()).toEqual([]);
    const key = await newProposeKey(page, "smoke12");
    keyLabel = key.label;
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");

      // 12.1 — an ISBN typed as printed, with hyphens.
      const isbn13 = specIsbn(SPEC, RUN);
      await page.goto("/catalog");
      const scanField = page.locator("#scan-field");
      await scanField.fill(hyphenated(isbn13));
      await scanField.press("Enter");
      const titleId = Number(await titleIdFromSkeleton(page));
      expect(titleId).toBeGreaterThan(0);
      // Settled background fetch (it would otherwise race the proposals).
      let version = 0;
      await expect
        .poll(
          async () => {
            const t = (await mcpCallTool(agent, key.plaintext, "get_title", { id: titleId }))
              .structuredContent;
            version = t.version;
            expect(t.isbn).toBe(isbn13);
            return t.publisher;
          },
          { timeout: 15000 },
        )
        .toBe("Test Publisher");

      // 12.3 — the agent is told where mybibli already searched.
      const init = await mcpPost(agent, key.plaintext, {
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: {
          protocolVersion: "2025-11-25",
          capabilities: {},
          clientInfo: { name: "mybibli-smoke12", version: "1.0.0" },
        },
      });
      expect(init.status()).toBe(200);
      const instructions: string = (await init.json()).result.instructions;
      expect(instructions).toMatch(/^- book: BnF, /m);
      expect(instructions).toContain("Search other sources first");

      // 12.5 — a title correction (plus a description), and a cover.
      const corrected = `Titre corrigé EZ ${RUN}`;
      const meta = await mcpCallTool(agent, key.plaintext, "propose_title_metadata", {
        title_id: titleId,
        version,
        fields: { title: corrected, description: "Une description trouvée ailleurs." },
        sources: [SOURCE],
      });
      expect(meta.isError, JSON.stringify(meta.content)).toBe(false);
      const metaId = meta.structuredContent.proposal_id as number;
      const cover = await mcpCallTool(agent, key.plaintext, "propose_cover", {
        title_id: titleId,
        version,
        image_base64: PNG_1X1,
        media_type: "image/png",
        sources: [SOURCE],
      });
      expect(cover.isError, JSON.stringify(cover.content)).toBe(false);
      const coverId = cover.structuredContent.proposal_id as number;

      // 12.4 — the list shows what each proposes; the subject opens it.
      await page.goto("/proposals?status=pending");
      const coverRow = page.locator(`tr[data-proposal-id="${coverId}"]`);
      const thumb = coverRow.locator(`img[src="/proposals/${coverId}/cover"]`);
      await expect(thumb).toBeVisible();
      await expect
        .poll(() => thumb.evaluate((el) => (el as HTMLImageElement).naturalWidth))
        .toBeGreaterThan(0);
      const metaRow = page.locator(`tr[data-proposal-id="${metaId}"]`);
      await expect(metaRow.locator("[data-proposed-fields]")).toHaveText(PROPOSED_FIELDS);
      await coverRow.getByRole("link", { name: PROPOSAL_LINK(coverId) }).click();
      await expect(page).toHaveURL(new RegExp(`/proposals/${coverId}$`));
      await acceptFromDetail(page);

      // 12.2 — the accepted cover does not outdate the metadata proposal.
      await page.goto("/proposals?status=pending");
      await page
        .locator(`tr[data-proposal-id="${metaId}"]`)
        .getByRole("link", { name: PROPOSAL_LINK(metaId) })
        .click();
      await expect(page).toHaveURL(new RegExp(`/proposals/${metaId}$`));
      await expect(page.locator("#proposal-detail-body")).toContainText(corrected);
      await acceptFromDetail(page);

      // Both applied: the corrected title and the installed cover.
      await page.goto(`/title/${titleId}`);
      await expect(page.locator("h1")).toContainText(corrected);
      await expect(page.locator(`img[src^="/covers/${titleId}.jpg?v="]`).first()).toBeVisible({
        timeout: 10000,
      });
    } finally {
      await agent.dispose();
    }
  });
});
