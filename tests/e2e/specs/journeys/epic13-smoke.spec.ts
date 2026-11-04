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
 * Spec ID "E3". Re-runs on a non-reset stack draw fresh codes:
 * - the marker ISBN is `specIsbn("TN", 91000–99999)` — the 13-10 mock case
 *   (seq ≥ 1000), in a band tome-number.spec.ts (1000–90999) never draws;
 *   the seq follows the clock in seconds (`% 9000`), so runs less than
 *   2.5 h apart never share it; the second ISBN, `specIsbn("E3", seconds
 *   % 100000)`, repeats only after ~28 h;
 * - the V-code is in V2000–V3999, which no spec hardcodes nor derives a
 *   band in, following the clock in seconds (`% 2000`: runs less than
 *   33 min apart never share it). The suite-wide per-test generators of
 *   loans.spec.ts / home-search.spec.ts (V0000–V9999) and home.spec.ts
 *   (V1000–V9999) can still draw it, as they can any code: a rare,
 *   transient clash every spec already lives with;
 * - the two L-codes are drawn in bucket 8 (`specLcode(8, …)`, used by no
 *   other spec) among the codes /locations does not already list, so a
 *   re-run never meets an earlier run's locations.
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
const SECONDS = Math.floor(Date.now() / 1000);
const RUN = SECONDS % 100000;
const SOURCE = "https://example.org/epic13-smoke";
const CLEAN_TITLE = "Le Mystère des tomes";
const TOME_3 = /(Tome|Volume|Band|Tomo)\s*3\b/;
const DETECTED = /\((détecté|detected|erkannt|rilevato)\)/;
const ERROR = '.feedback-entry[data-feedback-variant="error"]';
const DIALOG = "#modal-slot dialog[open]";
const PROPOSAL_LINK = (id: number) =>
  new RegExp(`(proposal #|proposition n° |Vorschlag Nr\\. |proposta n\\. )${id}$`);

/**
 * A UTC day, `offsetDays` from today. Since story 15-1 (#602) the web
 * cuts "added since" days in the instance zone (Europe/Zurich by
 * default, UTC while instance-time-zone.spec runs). Presence uses today's
 * UTC date: its start, in either zone, is never after a title created
 * today. Absence uses `utcDay(2)`: `utcDay(1)` is already today in Zurich
 * from 23:00 UTC (22:00 in summer). A run straddling midnight UTC between
 * the scan and the filter would still fail the presence check — the smoke
 * runs in seconds, so that window is accepted.
 */
function utcDay(offsetDays: number): string {
  const d = new Date();
  d.setUTCDate(d.getUTCDate() + offsetDays);
  return d.toISOString().slice(0, 10);
}

/** Two consecutive bucket-8 L-codes /locations does not list yet. */
async function freeLcodePair(page: Page): Promise<[string, string]> {
  await page.goto("/locations");
  const listed = await page.content();
  for (let attempt = 0; attempt < 50; attempt++) {
    const seq = Math.floor(Math.random() * 499) * 2;
    const pair: [string, string] = [specLcode(8, seq), specLcode(8, seq + 1)];
    if (!listed.includes(pair[0]) && !listed.includes(pair[1])) return pair;
  }
  throw new Error("no free L-code pair left in bucket 8");
}

/** Apply the home "added since" form (the current query is kept). */
async function applyAddedSince(page: Page, day: string) {
  await page.locator("#browse-added-since").fill(day);
  await page
    .locator("#browse-added-since-form")
    .getByRole("button", { name: /Apply|Appliquer|Anwenden|Applica/i })
    .click();
  await expect(page).toHaveURL(new RegExp(`added_since=${day}`));
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
      // Nothing to revoke if the creation never reached the server.
      const row = admin.locator("#admin-api-keys-list tr", { hasText: keyLabel });
      if ((await row.count()) > 0) await revokeApiKey(admin, keyLabel);
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

    const markerIsbn = specIsbn("TN", 91000 + (SECONDS % 9000));
    const otherIsbn = specIsbn(SPEC, RUN);
    const vcode = `V${2000 + (SECONDS % 2000)}`;
    const [rootCode, shelfCode] = await freeLcodePair(page);
    const root = `E3-House-${SECONDS}`;
    const shelf = `E3-Shelf-${SECONDS}`;
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
    // Two submits of the same label, back to back (well under 200 ms):
    // the scan field must post the label once.
    const labelPosts: string[] = [];
    page.on("request", (req) => {
      if (req.method() !== "POST" || new URL(req.url()).pathname !== "/catalog/scan") return;
      const code = new URLSearchParams(req.postData() ?? "").get("code");
      if (code === vcode) labelPosts.push(code);
    });
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
    expect(labelPosts).toEqual([vcode]);

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

    // 13-7 — the home "added since" filter, on a full-text search for the
    // title: today keeps it, the day after tomorrow leaves it out. Not on the ISBN: on
    // the web an exact ISBN/V-code hit means "show me this book" and
    // bypasses the browse filters by design (13-7 review, L5; the MCP
    // applies added_since). Sorted by date added, newest first, so the
    // title sits on the first page whatever earlier runs left; the
    // future-date check proves the filter, not the sort.
    await page.goto(`/?q=${encodeURIComponent(CLEAN_TITLE)}&sort=date_added&dir=desc`);
    const titleLink = page.locator(`#browse-results a[href="/title/${titleId}"]`);
    await applyAddedSince(page, utcDay(0));
    await expect(page).toHaveURL(/sort=date_added/);
    await expect(titleLink.first()).toBeAttached();
    // The day after tomorrow (UTC): the web cuts days in the instance
    // zone (Europe/Zurich, or UTC while instance-time-zone.spec runs), so
    // "tomorrow" in UTC can already be today in Zurich after 22:00/23:00
    // UTC; two days ahead is in the future in both zones.
    await applyAddedSince(page, utcDay(2));
    await expect(page.locator("#browse-added-since")).toHaveValue(utcDay(2));
    await expect(titleLink).toHaveCount(0);

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
    await page.goto(`/title/${titleId}`);
    await expect(page.locator("#title-metadata h1")).toHaveText(CLEAN_TITLE);
    await expect(page.locator("main")).not.toContainText(vcode);

    // 13-9 — as admin: a label and a `propose` key.
    await page.context().clearCookies();
    await loginAs(page, "admin");
    await createLabel(page, labelName);
    // The key's label is set before the creation, so afterEach revokes it
    // even if the creation flow fails late.
    keyLabel = `smoke13-${crypto.randomUUID().slice(0, 8)}`;
    const key = await createApiKey(page, "propose", keyLabel, { exactLabel: true });

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
