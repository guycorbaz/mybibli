/**
 * Story 11-23 — Epic 11 smoke test (Foundation Rule #7): the epic's core
 * journey, end to end, from a blank browser. If this test fails, epic 11
 * is not done.
 *
 * Blank context → loginAs(admin) → a `propose` key created in Admin → API
 * keys → cookies cleared → loginAs(librarian) → two ISBNs scanned → a
 * scripted MCP client (fresh request context, plain JSON-RPC) opens with
 * `initialize` (2025-11-25), lists the tools, finds both titles with
 * `search_titles` (one call per protocol revision: 2025-11-25 and the
 * session-less 2026-07-28) and files a series proposal (two titles, one
 * expected tome) → the nav badge and the dashboard pill show it waiting →
 * the nav entry leads to /proposals → the librarian opens the proposal,
 * moves a title to another position, accepts → the series page shows both
 * titles at their positions and the expected tome → the tome goes to the
 * wish list → its ISBN is scanned → it shows as owned → the key is
 * revoked (finally).
 *
 * The pending count is library-wide and parallel specs file and decide
 * proposals, so the badge assertion is relative (≥ 1, badge = aria =
 * pill on the same response), not the epic's literal "shows 1".
 *
 * Spec ID "EK". ISBN sequences and the series name carry a per-run stamp,
 * so a re-run on a non-reset stack meets none of an earlier run's titles,
 * tomes or wish-list items; nothing is rejected, so the 90-day
 * resubmission delay (11-15) never applies. The file name starts with
 * "series-" so it sorts well after `classification-fieldset`,
 * `dewey-code` and `empty-states`, which assume no series exists
 * (D-11-11-6, #528). The key is revoked in `afterEach` from a fresh
 * browser context, so it is revoked even when the test times out.
 */
import { test, expect, request as playwrightRequest } from "@playwright/test";
import { revokeApiKey } from "../../helpers/api-keys";
import { loginAs } from "../../helpers/auth";
import { specIsbn } from "../../helpers/isbn";
import { MCP_TOOLS, mcpCallTool, mcpPost } from "../../helpers/mcp";
import {
  NAV,
  acceptFromDetail,
  newProposeKey,
  scanSettledTitle,
  shownPendingCount,
} from "../../helpers/proposals";

const SPEC = "EK";
// Seconds stamp, kept small enough for specIsbn's five-digit sequence.
const RUN = Math.floor(Date.now() / 1000) % 24000;
const seq = (k: number) => RUN * 4 + k;

const SOURCE = "https://example.org/epic11-smoke";
const EDIT = /^(Edit|Modifier|Bearbeiten|Modifica) —/;
const SAVE = /^(Save|Enregistrer|Speichern|Salva)$/;
const UPDATED =
  /Line of proposal #\d+ updated|Ligne de la proposition n° \d+ modifiée|Zeile von Vorschlag Nr\. \d+ geändert|Riga della proposta n\. \d+ modificata/i;
const WISH_BUTTON =
  /Add to wish list|Ajouter à la wish list|Zur Wunschliste hinzufügen|Aggiungi alla lista dei desideri/i;
const WISHED =
  /added to your wish list|ajouté à votre wish list|Wunschliste hinzugefügt|aggiunto alla tua lista dei desideri/i;
const SETTLED =
  /Expected tome found|Tome attendu trouvé|Erwarteter Band gefunden|Volume atteso trovato/i;

/** `_meta` and headers of a session-less call on revision 2026-07-28. */
function modernCall(id: number, name: string, args: object) {
  return {
    body: {
      jsonrpc: "2.0",
      id,
      method: "tools/call",
      params: {
        name,
        arguments: args,
        _meta: {
          "io.modelcontextprotocol/protocolVersion": "2026-07-28",
          "io.modelcontextprotocol/clientInfo": { name: "mybibli-smoke", version: "1.0.0" },
          "io.modelcontextprotocol/clientCapabilities": {},
        },
      },
    },
    headers: { "MCP-Protocol-Version": "2026-07-28", "Mcp-Method": "tools/call", "Mcp-Name": name },
  };
}

test.describe("Epic 11 smoke — an agent proposes, a librarian decides", () => {
  // Two admin logins, a librarian session, two metadata fetches, a scan.
  test.setTimeout(120000);

  // The key created by the test, revoked after it whatever happened.
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

  test("propose key → MCP client files a series → badge → edit, accept → series → wish → scan", async ({
    page,
    baseURL,
  }) => {
    // A blank browser: no cookie, nothing injected.
    expect(await page.context().cookies()).toEqual([]);
    const key = await newProposeKey(page, "smoke");
    keyLabel = key.label;
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    // The agent is a separate client: a fresh request context, no cookies.
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const isbn1 = specIsbn(SPEC, seq(0));
      const isbn2 = specIsbn(SPEC, seq(1));
      const tomeIsbn = specIsbn(SPEC, seq(2));
      const t1 = await scanSettledTitle(page, agent, key.plaintext, SPEC, seq(0));
      const t2 = await scanSettledTitle(page, agent, key.plaintext, SPEC, seq(1));

      // --- The agent: 2025-11-25 opens with initialize, then tools/list.
      const init = await mcpPost(agent, key.plaintext, {
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: {
          protocolVersion: "2025-11-25",
          capabilities: {},
          clientInfo: { name: "mybibli-smoke", version: "1.0.0" },
        },
      });
      expect(init.status()).toBe(200);
      expect((await init.json()).result.protocolVersion).toBe("2025-11-25");
      const list = await mcpPost(
        agent,
        key.plaintext,
        { jsonrpc: "2.0", id: 2, method: "tools/list" },
        { "MCP-Protocol-Version": "2025-11-25" },
      );
      expect(list.status()).toBe(200);
      const names = (await list.json()).result.tools.map((t: { name: string }) => t.name).sort();
      expect(names).toEqual(MCP_TOOLS);

      // Reads the catalog: one title per protocol revision.
      const found1 = await mcpCallTool(agent, key.plaintext, "search_titles", { query: isbn1 });
      expect(found1.isError).toBe(false);
      expect(found1.structuredContent.items.map((i: { id: number }) => i.id)).toEqual([t1]);
      const modern = modernCall(3, "search_titles", { query: isbn2 });
      const res2 = await mcpPost(agent, key.plaintext, modern.body, modern.headers);
      expect(res2.status()).toBe(200);
      const found2 = (await res2.json()).result;
      expect(found2.isError).toBe(false);
      expect(found2.structuredContent.items.map((i: { id: number }) => i.id)).toEqual([t2]);

      // Files a closed series: both titles, one expected tome with an ISBN.
      const name = `EK-Smoke-${RUN}`;
      const tomeTitle = `EK tome attendu ${RUN}`;
      const filed = await mcpCallTool(agent, key.plaintext, "propose_series_create", {
        name,
        series_type: "closed",
        total_volume_count: 4,
        titles: [
          { title_id: t1, position: 1 },
          { title_id: t2, position: 2 },
        ],
        expected_tomes: [{ position: 3, title: tomeTitle, isbn: tomeIsbn }],
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const proposalId = filed.structuredContent.proposal_id as number;

      // --- The librarian sees it waiting: badge in the bar, pill on the
      // dashboard, same count on the same response.
      await page.goto("/");
      expect(await shownPendingCount(page)).toBeGreaterThanOrEqual(1);
      await expect(page.locator("#filter-tag-pending-proposals")).toBeVisible();

      // The nav entry leads to the list, which holds this proposal.
      await page.locator(`${NAV} > div a[href="/proposals"]`).click();
      await expect(page).toHaveURL(/\/proposals$/);
      await page.locator(`a[href="/proposals/${proposalId}"]`).click();
      await expect(page).toHaveURL(new RegExp(`/proposals/${proposalId}$`));
      await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
        "data-proposal-status",
        "pending",
      );

      // Moves the second title from position 2 to position 4.
      const items = page.locator("#proposal-items > li");
      const line = items.filter({ has: page.locator(`a[href="/title/${t2}"]`) });
      await line.getByRole("button", { name: EDIT }).click();
      const form = line.locator("[data-edit-slot] form");
      await expect(form).toBeVisible();
      await form.getByLabel(/^(Position|Posizione)$/).fill("4");
      await form.getByRole("button", { name: SAVE }).click();
      await expect(
        page.locator("#feedback-list .feedback-entry").filter({ hasText: UPDATED }).last(),
      ).toBeVisible({ timeout: 10000 });
      await expect(items.filter({ has: page.locator(`a[href="/title/${t2}"]`) })).toContainText(
        /\b(Position|Posizione) 4\b/,
      );

      await acceptFromDetail(page);

      // --- The catalog shows the result: the series as accepted.
      await page.locator("#proposal-detail-body").locator(`a[href^="/series/"]`).first().click();
      await page.waitForURL(/\/series\/\d+$/);
      const seriesPath = new URL(page.url()).pathname;
      const seriesId = Number(seriesPath.match(/\/series\/(\d+)$/)![1]);
      await expect(page.locator("h1")).toContainText(name);
      const cell = (titleId: number) =>
        page.locator(`a[role="gridcell"][href="/title/${titleId}"]`);
      await expect(cell(t1)).toHaveAttribute("aria-label", / 1 — /);
      await expect(cell(t2)).toHaveAttribute("aria-label", / 4 — /);
      const tome = page.locator("[data-expected-tome]").filter({ hasText: tomeTitle });
      await expect(tome).toHaveAttribute("data-position", "3");
      await expect(tome).toContainText(tomeIsbn);
      const detail = (await mcpCallTool(agent, key.plaintext, "get_series", { id: seriesId }))
        .structuredContent;
      const positions = (detail.positions as { title_id: number; position: number }[])
        .map((p): [number, number] => [p.title_id, p.position])
        .sort((a, b) => a[1] - b[1]);
      expect(positions).toEqual([
        [t1, 1],
        [t2, 4],
      ]);
      expect(
        (detail.expected_tomes as { position: number; title: string; isbn: string | null }[]).map(
          (t) => [t.position, t.title, t.isbn],
        ),
      ).toEqual([[3, tomeTitle, tomeIsbn]]);
      expect(detail.missing_positions).toEqual([2, 3]);

      // The expected tome goes to the wish list.
      await tome.getByRole("button", { name: WISH_BUTTON }).click();
      await expect(page.locator("#series-feedback")).toContainText(WISHED);

      // Scanning its ISBN settles it at position 3, linked to the wish.
      await page.goto("/catalog");
      const scanField = page.locator("#scan-field");
      await scanField.fill(tomeIsbn);
      await scanField.press("Enter");
      await expect(page.locator("#feedback-list")).toContainText(SETTLED, { timeout: 10000 });
      await expect(page.locator("#feedback-list [data-wishlist-autolink]").first()).toBeVisible();

      // Owned now: three titles in the grid, position 3 among them, no tome.
      await page.goto(seriesPath);
      await expect(page.locator("[data-expected-tome]")).toHaveCount(0);
      const owned = page.locator('a[role="gridcell"][href^="/title/"]');
      await expect(owned).toHaveCount(3);
      await expect(page.locator('a[role="gridcell"][aria-label*=" 3 — "]')).toHaveCount(1);
      const tomeTitleId = (
        await mcpCallTool(agent, key.plaintext, "search_titles", { query: tomeIsbn })
      ).structuredContent.items[0].id as number;
      const settled = (await mcpCallTool(agent, key.plaintext, "get_series", { id: seriesId }))
        .structuredContent;
      expect(
        (settled.positions as { title_id: number; position: number }[]).find(
          (p) => p.position === 3,
        )?.title_id,
      ).toBe(tomeTitleId);
      // The tome is kept as settled by the scanned title, none left open.
      expect(
        (settled.expected_tomes as { title: string; settled_title_id: number | null }[]).map(
          (t) => [t.title, t.settled_title_id],
        ),
      ).toEqual([[tomeTitle, tomeTitleId]]);
      expect(settled.missing_positions).toEqual([2]);
    } finally {
      await agent.dispose();
    }
  });
});
