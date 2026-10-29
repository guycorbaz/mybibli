/**
 * Story 11-21 E2E — access to the proposals page (FR143, UX-DR32,
 * UX-DR33): the "Proposals" nav entry with its pending-count badge and
 * the "Pending proposals" dashboard indicator appear with a pending
 * proposal and go away once it is decided.
 *
 * From a blank browser: an admin creates a `propose` key → cookies
 * cleared → loginAs(librarian) → scan an ISBN → the agent files a
 * metadata proposal → the badge (strip and mobile panel) and the
 * dashboard pill show the count → the nav entry leads to /proposals →
 * reject → on the next navigation the count is lower (zero: badge and
 * pill gone) → the admin revokes the key.
 *
 * The pending count is library-wide and other specs file proposals in
 * parallel, so the assertions are relative ("at least one", "lower than
 * before the decision") and always cross-check the badge against the
 * pill rendered by the SAME response.
 *
 * Spec ID "NB" for ISBNs; the proposal targets this spec's own title, so
 * its rejection blocks nothing elsewhere. Creates no series (#528).
 */
import { test, expect, request as playwrightRequest, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { mcpCallTool } from "../../helpers/mcp";
import { newProposeKey, revokeProposeKey, scanSettledTitle } from "../../helpers/proposals";

const NAV = 'nav[aria-label="Main navigation"]';
const REJECT_BUTTON = /^(Reject|Refuser|Ablehnen|Rifiuta)$/;
const PILL_LABEL = /Pending proposals|Propositions en attente|Offene Vorschläge|Proposte in attesa/;

/**
 * The pending count shown on the current page: the strip entry's
 * accessible name ("Propositions, 3 en attente"), 0 when it carries none.
 * Checks the badge text matches (capped at 99+) and, on the home page,
 * that the dashboard pill shows the same number.
 */
async function shownCount(page: Page): Promise<number> {
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

test.describe("Story 11-21 — access to the proposals page", () => {
  // Two admin logins around the librarian's session plus a metadata fetch.
  test.setTimeout(90000);

  test("the badge and the dashboard indicator follow a pending proposal", async ({
    page,
    baseURL,
  }) => {
    const key = await newProposeKey(page, "badge");
    expect(baseURL, "baseURL must be configured").toBeTruthy();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      // A retry gets a fresh title: the first attempt's rejection blocks an
      // equivalent re-filing on the same one library-wide (11-15).
      const titleId = await scanSettledTitle(
        page,
        agent,
        key.plaintext,
        "NB",
        1 + test.info().retry,
      );
      const version = (await mcpCallTool(agent, key.plaintext, "get_title", { id: titleId }))
        .structuredContent.version;
      const filed = await mcpCallTool(agent, key.plaintext, "propose_title_metadata", {
        title_id: titleId,
        version,
        // Unique per run: a local re-run on a non-reset stack would
        // otherwise be refused by the previous run's rejection (11-15).
        fields: { subtitle: `Badge ${Date.now()}` },
        sources: ["https://example.org/badge"],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const proposalId = filed.structuredContent.proposal_id as number;

      // The next navigation shows it: badge in the bar, pill on the dashboard.
      await page.goto("/");
      const afterFiling = await shownCount(page);
      expect(afterFiling).toBeGreaterThanOrEqual(1);
      await expect(page.locator("#filter-tag-pending-proposals")).toBeVisible();

      // The mobile panel carries the same entry and badge.
      await page.setViewportSize({ width: 360, height: 740 });
      await page.locator("#mobile-menu-toggle").click();
      const panelEntry = page.locator('#mobile-nav a[href="/proposals"]');
      await expect(panelEntry).toBeVisible();
      await expect(panelEntry.locator("[data-proposals-badge]")).toBeVisible();
      await expect(panelEntry).toHaveAttribute("aria-label", /\d+/);
      await page.setViewportSize({ width: 1280, height: 720 });

      // The entry leads to /proposals and is marked current there.
      await page.locator(`${NAV} > div a[href="/proposals"]`).click();
      await expect(page).toHaveURL(/\/proposals$/);
      await expect(page.locator(`${NAV} > div a[href="/proposals"]`)).toHaveAttribute(
        "aria-current",
        "page",
      );

      // Decide it. The baseline is the count rendered by the detail page
      // itself, one request before the decision, so a sibling spec filing
      // in between can hardly hide the drop.
      await page.goto(`/proposals/${proposalId}`);
      const beforeDecision = await shownCount(page);
      expect(beforeDecision).toBeGreaterThanOrEqual(1);
      await page.getByRole("button", { name: REJECT_BUTTON }).click();
      const dialog = page.locator("#modal-slot dialog");
      await expect(dialog).toBeVisible();
      await dialog.locator("[data-modal-confirm]").click();
      await expect(dialog).toBeHidden({ timeout: 10000 });
      await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
        "data-proposal-status",
        "rejected",
      );

      // Next navigation: the count dropped (polled: a sibling spec may file
      // and decide its own proposal meanwhile); at zero the badge and the
      // pill are gone (checked inside shownCount). The exact drop to zero
      // is proven deterministically by tests/proposals_access.rs.
      await expect
        .poll(
          async () => {
            await page.goto("/");
            return shownCount(page);
          },
          { timeout: 20000 },
        )
        .toBeLessThan(beforeDecision);

      // #545 — the list (this decided proposal at least) fits a phone:
      // no horizontal page scroll, the row and its Open link visible.
      await page.setViewportSize({ width: 360, height: 740 });
      await page.goto("/proposals?status=all");
      const row = page.locator(`tr[data-proposal-id="${proposalId}"]`);
      await expect(row).toBeVisible();
      await expect(row.locator(`a[href="/proposals/${proposalId}"]`)).toBeInViewport();
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      );
      expect(overflow, "/proposals at 360 px scrolls sideways").toBeLessThanOrEqual(1);
    } finally {
      await agent.dispose();
      await revokeProposeKey(page, key.label);
    }
  });

  test("anonymous visitors see neither the entry, the badge nor the indicator", async ({
    page,
  }) => {
    await page.context().clearCookies();
    await page.goto("/");
    await expect(page.locator('a[href="/proposals"]')).toHaveCount(0);
    await expect(page.locator("[data-proposals-badge]")).toHaveCount(0);
    await expect(page.locator("#filter-tag-pending-proposals")).toHaveCount(0);
  });
});
