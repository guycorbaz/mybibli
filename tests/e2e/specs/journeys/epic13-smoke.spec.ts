/**
 * Story 13-12 — Epic 13 smoke test (Foundation Rule #7): the epic's core
 * journey, end to end, from a blank browser. If this test fails, epic 13
 * is not done.
 *
 * Blank context → loginAs(librarian) → a two-level location tree (13-6) →
 * on /catalog an ISBN whose provider title carries a tome marker, then its
 * V-code read twice in a row by the "scanner" (13-1: one volume, no error),
 * then the leaf's L-code (shelved) → the title page shows the title without
 * the marker and "Tome 3 (detected)" (13-10), "Added on" (13-7) and the
 * volume's full location path (13-6) → the home "added since today" filter
 * lists the title (13-7) → a second ISBN; the volume is moved to it from
 * its volume page through the modal (13-8) → loginAs(admin): a label and a
 * `propose` key; a scripted MCP client lists the labels and proposes the
 * label on the volume (13-9) → loginAs(librarian): the proposal is opened
 * from the proposals list and accepted; the volume page carries the label
 * and the label page lists the volume → the key is revoked (afterEach,
 * fresh context).
 *
 * Spec ID "E3"; every code carries a per-run stamp so a re-run on a
 * non-reset stack meets none of an earlier run's rows:
 * - the marker ISBN is `specIsbn("TN", 91000–99999)` — the 13-10 mock case
 *   (seq ≥ 1000), in a band tome-number.spec.ts (1000–90999) never draws;
 * - V-code band V0360–V0439 (no other spec hardcodes or derives it, apart
 *   from the suite-wide random generators of loans/home specs);
 * - L-code bucket 8 (`specLcode(8, …)`, used by no other spec).
 * No mock case was added; the HTML description (13-4) is not covered here
 * (its mock case is a fixed Google Books ISBN, shared with its own spec).
 */
import { test, expect, request as playwrightRequest, type Page } from "@playwright/test";
import { revokeApiKey, createApiKey } from "../../helpers/api-keys";
import { loginAs } from "../../helpers/auth";
import { specIsbn, specLcode } from "../../helpers/isbn";
import { titleIdFromSkeleton } from "../../helpers/catalog";
import { createLocation, createChildLocation } from "../../helpers/locations";
import { createLabel } from "../../helpers/labels";
import { mcpCallTool } from "../../helpers/mcp";
import { acceptFromDetail } from "../../helpers/proposals";
import { simulateScan } from "../../helpers/scanner";

const SPEC = "E3";
// Seconds stamp, kept small enough for specIsbn's five-digit sequence.
const RUN = Math.floor(Date.now() / 1000) % 90000;
const SOURCE = "https://example.org/epic13-smoke";
const CLEAN_TITLE = "Le Mystère des tomes";
const TOME_3 = /(Tome|Volume|Band|Tomo)\s*3\b/;
const DETECTED = /\((détecté|detected|erkannt|rilevato)\)/;
const ERROR = '.feedback-entry[data-feedback-variant="error"]';
const DIALOG = "#modal-slot dialog[open]";
const PROPOSAL_LINK = (id: number) =>
  new RegExp(`(proposal #|proposition n° |Vorschlag Nr\\. |proposta n\\. )${id}$`);

function utcToday(): string {
  return new Date().toISOString().slice(0, 10);
}

async function scan(page: Page, code: string) {
  const field = page.locator("#scan-field");
  await field.fill(code);
  await field.press("Enter");
}

test.describe("Epic 13 smoke — catalogue corrections, end to end", () => {
  test.setTimeout(150000);

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

  test("tome marker + double scan → full path + added since → move → agent label accepted", async ({
    page,
    baseURL,
  }) => {
    // A blank browser: no cookie, nothing injected.
    await page.context().clearCookies();
    expect(await page.context().cookies()).toEqual([]);
    await loginAs(page, "librarian");

    const markerIsbn = specIsbn("TN", 91000 + (RUN % 9000));
    const otherIsbn = specIsbn(SPEC, RUN);
    const vcode = `V0${360 + (RUN % 80)}`;
    const lseq = (RUN % 499) * 2;
    const rootCode = specLcode(8, lseq);
    const shelfCode = specLcode(8, lseq + 1);
    const root = `E3-House-${RUN}`;
    const shelf = `E3-Shelf-${RUN}`;
    const labelName = `Smoke doubtful ${RUN}-${crypto.randomUUID().slice(0, 6)}`;

    // 13-6 — a two-level location tree.
    await createLocation(page, root, rootCode);
    await createChildLocation(page, root, shelf, shelfCode);

    // 13-1 — the marker ISBN, then its label read twice by the scanner.
    await page.goto("/catalog");
    await scan(page, markerIsbn);
    const titleId = Number(await titleIdFromSkeleton(page));
    expect(titleId).toBeGreaterThan(0);
    await expect(page.locator("#context-banner")).toContainText(markerIsbn, {
      timeout: 10000,
    });
    // Two submits of the same label, back to back (well under 200 ms).
    await scan(page, vcode);
    await scan(page, vcode);
    // The leaf's L-code shelves the volume; once its answer is in, any
    // second label post would have been answered too.
    await scan(page, shelfCode);
    await expect(
      page.locator(".feedback-entry").filter({ hasText: shelf }).first(),
    ).toBeVisible({ timeout: 10000 });
    // One "attached" entry and one "shelved" entry — no "already
    // recorded", no error.
    await expect(
      page.locator(".feedback-entry").filter({ hasText: new RegExp(`\\b${vcode}\\b`) }),
    ).toHaveCount(2);
    await expect(page.locator(ERROR)).toHaveCount(0);
    await expect(page.locator("#context-banner")).toContainText(/\b1 vol/i);

    // 13-10 / 13-7 / 13-6 — the title page, once the metadata has landed.
    const h1 = page.locator("#title-metadata h1");
    await expect(async () => {
      await page.goto(`/title/${titleId}`);
      await expect(h1).toHaveText(CLEAN_TITLE, { timeout: 2000 });
    }).toPass({ timeout: 30000 });
    const tome = page.locator("#title-tome");
    await expect(tome).toContainText(TOME_3);
    await expect(tome).toContainText(DETECTED);
    await expect(page.locator("#record-dates")).toContainText(
      /Added on|Ajouté le|Hinzugefügt am|Aggiunto il/i,
    );
    const volumeRows = page.locator("#title-volumes table tbody tr");
    await expect(volumeRows).toHaveCount(1);
    await expect(volumeRows.first()).toContainText(vcode);
    await expect(volumeRows.first()).toContainText(`${root} → ${shelf}`);
    await expect(volumeRows.first()).toContainText(`(${shelfCode})`);

    // 13-7 — the home "added since today" filter lists the title (sorted
    // by date added, newest first, so it sits on the first page).
    const today = utcToday();
    await page.goto("/");
    await page.locator("#browse-added-since").fill(today);
    await page
      .locator("#browse-added-since-form")
      .getByRole("button", { name: /Apply|Appliquer|Anwenden|Applica/i })
      .click();
    await expect(page).toHaveURL(new RegExp(`added_since=${today}`));
    await page
      .locator("#browse-results thead a", { hasText: /Added|Ajouté|Hinzugefügt|Aggiunto/i })
      .click();
    await expect(page).toHaveURL(/sort=date_added/);
    await expect(page).toHaveURL(new RegExp(`added_since=${today}`));
    await expect(
      page.locator(`#browse-results a[href="/title/${titleId}"]`).first(),
    ).toBeAttached();

    // 13-8 — a second title; the volume moves to it through the modal.
    await page.goto("/catalog");
    await scan(page, otherIsbn);
    const otherTitleId = Number(await titleIdFromSkeleton(page));
    expect(otherTitleId).toBeGreaterThan(0);
    expect(otherTitleId).not.toBe(titleId);
    await expect(page.locator("#context-banner")).toContainText(otherIsbn, {
      timeout: 10000,
    });

    await page.goto(`/title/${titleId}`);
    await page
      .locator('#title-volumes table a[href^="/volume/"]')
      .filter({ hasText: vcode })
      .click();
    await expect(page).toHaveURL(/\/volume\/\d+$/);
    const volumeId = Number(/\/volume\/(\d+)$/.exec(page.url())![1]);
    await page
      .getByRole("button", {
        name: /Move to another title|Déplacer vers un autre titre|Anderem Titel zuordnen|Sposta su un altro titolo/i,
      })
      .click();
    const dialog = page.locator(DIALOG);
    await expect(dialog).toBeVisible();
    await expect(page.locator("#volume-move-search")).toBeFocused();
    await simulateScan(page, "#volume-move-search", otherIsbn);
    const candidate = page.locator("#volume-move-candidates [data-move-candidate]");
    await expect(candidate).toHaveCount(1, { timeout: 10000 });
    await expect(candidate).toContainText(otherIsbn);
    await candidate.click();
    await expect(page.locator("[data-move-confirmation]")).toContainText(vcode);
    await dialog.locator("[data-modal-confirm]").click();
    await expect(page.locator(DIALOG)).toHaveCount(0);
    await expect(page.locator("#volume-title-row a")).toHaveAttribute(
      "href",
      `/title/${otherTitleId}`,
    );
    await page.goto(`/title/${otherTitleId}`);
    await expect(page.locator("#title-volumes")).toContainText(vcode);

    // 13-9 — as admin: a label and a `propose` key.
    await page.context().clearCookies();
    await loginAs(page, "admin");
    await createLabel(page, labelName);
    const key = await createApiKey(page, "propose", "smoke13");
    keyLabel = key.label;

    // A scripted MCP client lists the labels and proposes one.
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    let proposalId = 0;
    let labelId = 0;
    try {
      const labels = (await mcpCallTool(agent, key.plaintext, "list_labels", {}))
        .structuredContent.items;
      const label = labels.find((l: { name: string }) => l.name === labelName);
      expect(label, JSON.stringify(labels)).toBeTruthy();
      labelId = label.id;
      const filed = await mcpCallTool(agent, key.plaintext, "propose_label", {
        target: { volume_id: volumeId },
        label_id: labelId,
        sources: [SOURCE],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      expect(filed.structuredContent.kind).toBe("label_attach");
      proposalId = filed.structuredContent.proposal_id;
    } finally {
      await agent.dispose();
    }

    // Back as librarian: open it from the list and accept.
    await page.context().clearCookies();
    await loginAs(page, "librarian");
    await page.goto("/proposals?status=pending");
    await page
      .locator(`tr[data-proposal-id="${proposalId}"]`)
      .getByRole("link", { name: PROPOSAL_LINK(proposalId) })
      .click();
    await expect(page).toHaveURL(new RegExp(`/proposals/${proposalId}$`));
    await expect(
      page.locator('#proposal-items li[data-line="label"] [data-label-target]'),
    ).toContainText(vcode);
    await acceptFromDetail(page);

    // The volume carries the label; the label page lists the volume.
    await page.goto(`/volume/${volumeId}`);
    await expect(
      page.locator(`#entity-labels [data-label-chip="${labelName}"]`),
    ).toBeVisible();
    await page.goto(`/labels/${labelId}`);
    await expect(
      page.locator("#label-volumes-table").getByRole("link", { name: vcode }),
    ).toBeVisible();
  });
});
