/**
 * Shared steps of the proposal journeys (epic 11): a `propose` key
 * created and revoked by an admin, a scanned title whose background
 * metadata fetch has settled, acceptance through the UX-DR8 modal.
 */
import { expect, Page, APIRequestContext } from "@playwright/test";
import { loginAs } from "./auth";
import { specIsbn } from "./isbn";
import { titleIdFromSkeleton } from "./catalog";
import { createApiKey, revokeApiKey } from "./api-keys";
import { mcpCallTool } from "./mcp";

export const ACCEPTED =
  /Proposal #\d+ accepted|Proposition n° \d+ acceptée|Vorschlag Nr\. \d+ angenommen|Proposta n\. \d+ accettata/i;

/** An admin creates a `propose` key (cookies cleared first). */
export async function newProposeKey(page: Page, prefix: string) {
  await page.context().clearCookies();
  await loginAs(page, "admin");
  return createApiKey(page, "propose", prefix);
}

/** An admin revokes the key (its pending proposals become outdated). */
export async function revokeProposeKey(page: Page, label: string) {
  await page.context().clearCookies();
  await loginAs(page, "admin");
  await page.goto("/admin?tab=api_keys");
  await revokeApiKey(page, label);
}

/** Scan `specIsbn(specId, seq)` and wait for its background metadata
 *  fetch (it bumps the title's version, which would make a proposal
 *  filed before it outdated). Returns the title id. */
export async function scanSettledTitle(
  page: Page,
  agent: APIRequestContext,
  key: string,
  specId: string,
  seq: number,
): Promise<number> {
  await page.goto("/catalog");
  const scanField = page.locator("#scan-field");
  await scanField.fill(specIsbn(specId, seq));
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

/** Accept the proposal open on the page through the modal. */
export async function acceptFromDetail(page: Page) {
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

/** The desktop navigation strip (story 11-21). */
export const NAV = 'nav[aria-label="Main navigation"]';
const PILL_LABEL = /Pending proposals|Propositions en attente|Offene Vorschläge|Proposte in attesa/;

/**
 * The pending count shown on the current page (story 11-21): the strip
 * entry's accessible name ("Propositions, 3 en attente"), 0 when it
 * carries none. Checks the badge text matches (capped at 99+) and, on
 * the home page, that the dashboard pill shows the same number. The
 * count is library-wide: callers assert relatively.
 */
export async function shownPendingCount(page: Page): Promise<number> {
  const entry = page.locator(`${NAV} > div a[href="/proposals"]`);
  await expect(entry).toHaveCount(1);
  const aria = await entry.getAttribute("aria-label");
  const badge = entry.locator("[data-proposals-badge]");
  const onHome = new URL(page.url()).pathname === "/";
  const pill = page.locator("#filter-tag-pending-proposals");
  if (aria === null) {
    await expect(badge).toHaveCount(0);
    if (onHome) await expect(pill).toHaveCount(0);
    return 0;
  }
  const n = Number(/(\d+)/.exec(aria)![1]);
  expect(aria).toMatch(/^(Proposals|Propositions|Vorschläge|Proposte), \d+ /);
  await expect(badge).toHaveText(n > 99 ? "99+" : String(n));
  await expect(badge).toHaveAttribute("aria-hidden", "true");
  if (onHome) {
    await expect(pill).toHaveAttribute("href", "/proposals");
    await expect(pill).toHaveAttribute("aria-label", new RegExp(`: ${n}$`));
    await expect(pill).toContainText(PILL_LABEL);
  }
  return n;
}
