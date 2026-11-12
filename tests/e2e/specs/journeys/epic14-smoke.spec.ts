/**
 * Story 14-3 — Epic 14 smoke test (Foundation Rule #7): the new navigation,
 * end to end, from a blank browser. If this test fails, epic 14 is not
 * done.
 *
 * Setup (its own browser context, so the journey's page stays blank): an
 * admin creates a `propose` key; a librarian scans a title; a scripted MCP
 * client files one metadata proposal on it, so the Proposals entry carries
 * its badge.
 *
 * Journey, in a blank context:
 *   at 1280 px — loginAs(librarian) → from the sidebar, Catalog, Loans,
 *   Proposals (badge visible, accessible name with the count) → collapse
 *   → reload: still collapsed → Series by its icon (accessible name) →
 *   expand;
 *   at 390 px — open the drawer → Locations → the new page shows the
 *   drawer closed → open it again, log out → the anonymous drawer offers
 *   Log in → the login page is shown.
 *
 * Cleanup (setup context): the key is revoked, which turns its pending
 * proposal stale — nothing stays pending, nothing is blocked.
 *
 * Spec ID "E4" — one ISBN, `specIsbn("E4", seconds % 100000)`, so a re-run
 * on a non-reset stack scans a new title (it repeats only after ~28 h).
 */
import { test, expect, request as playwrightRequest, type Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { mcpCallTool } from "../../helpers/mcp";
import { newProposeKey, revokeProposeKey, scanSettledTitle } from "../../helpers/proposals";
import { expectDrawerClosed, openDrawer } from "../../helpers/drawer";

const SPEC = "E4";
const RUN = Math.floor(Date.now() / 1000) % 100000;
const WIDE = { width: 1280, height: 800 };
const PHONE = { width: 390, height: 844 };

const NAV = "Main navigation";
const CATALOG = /^(Catalog|Catalogue|Katalog|Catalogo)$/;
const LOANS = /^(Loans|Prêts|Ausleihen|Prestiti)$/;
const SERIES = /^(Series|Séries|Serien|Serie)$/;
const LOCATIONS = /^(Locations|Emplacements|Standorte|Posizioni|Ubicazioni)$/;
// With a pending proposal the entry's name carries the count (11-21):
// visible label + screen-reader-only count, no aria-label (18-1).
const PROPOSALS_PENDING =
  /^(Proposals \d+ pending|Propositions \d+ en attente|Vorschläge \d+ ausstehend|Proposte \d+ in attesa)$/;
const COLLAPSE = /^(Collapse menu|Réduire le menu|Menü einklappen|Comprimi menu)$/;
const LOG_OUT = /^(Log out|Déconnexion|Abmelden|Esci)$/;
const LOG_IN = /^(Log in|Connexion|Anmelden|Accedi)$/;
const DRAWER_DIALOG = /^(Menu|Menü)$/;

function sidebar(page: Page) {
  return page.getByRole("navigation", { name: NAV });
}

/** The sidebar's collapse toggle (one fixed name; aria-expanded carries
 *  the state) and its rendered width once the transition has run. */
async function expectSidebar(page: Page, expanded: boolean) {
  const toggle = sidebar(page).getByRole("button", { name: COLLAPSE });
  await expect(toggle).toHaveAttribute("aria-expanded", String(expanded));
  await expect
    .poll(() =>
      sidebar(page).evaluate((el) => Math.round(el.getBoundingClientRect().width)),
    )
    .toBe(expanded ? 224 : 64);
}

test.describe("Epic 14 smoke — the sidebar and the drawer, end to end", () => {
  test("blank browser → sidebar navigation, collapse remembered → drawer, logout", async ({
    page,
    browser,
    baseURL,
  }) => {
    test.setTimeout(120000);

    // --- Setup, in its own context: one pending proposal for the badge.
    const setupContext = await browser.newContext();
    const setup = await setupContext.newPage();
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    const key = await newProposeKey(setup, "epic14-smoke");
    try {
      await setup.context().clearCookies();
      await loginAs(setup, "librarian");
      const titleId = await scanSettledTitle(setup, agent, key.plaintext, SPEC, RUN);
      const version = (await mcpCallTool(agent, key.plaintext, "get_title", { id: titleId }))
        .structuredContent.version;
      const filed = await mcpCallTool(agent, key.plaintext, "propose_title_metadata", {
        title_id: titleId,
        version,
        fields: { subtitle: `Epic 14 smoke ${Date.now()}` },
        sources: ["https://example.org/epic14-smoke"],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);

      // --- Journey: a blank browser context, a real login, at 1280 px.
      expect(await page.context().cookies()).toEqual([]);
      await page.setViewportSize(WIDE);
      await loginAs(page, "librarian");
      await expectSidebar(page, true);

      await sidebar(page).getByRole("link", { name: CATALOG }).click();
      await expect(page).toHaveURL(/\/catalog$/);
      await expect(sidebar(page).getByRole("link", { name: CATALOG })).toHaveAttribute(
        "aria-current",
        "page",
      );

      await sidebar(page).getByRole("link", { name: LOANS }).click();
      await expect(page).toHaveURL(/\/loans$/);

      const proposals = sidebar(page).getByRole("link", { name: PROPOSALS_PENDING });
      await expect(proposals).toBeVisible();
      await expect(proposals.locator("[data-proposals-badge]")).toHaveText(/^(\d+|99\+)$/);
      await proposals.click();
      await expect(page).toHaveURL(/\/proposals$/);
      await expect(proposals).toHaveAttribute("aria-current", "page");

      // Collapse, remembered across a reload.
      await sidebar(page).getByRole("button", { name: COLLAPSE }).click();
      await expectSidebar(page, false);
      await page.reload();
      await expectSidebar(page, false);
      // The badge stays, as a pill on the icon.
      await expect(proposals.locator("[data-proposals-badge]")).toBeVisible();

      // Navigate by an icon: the label is visually hidden, the name stays.
      const series = sidebar(page).getByRole("link", { name: SERIES });
      await series.click();
      await expect(page).toHaveURL(/\/series$/);
      await expectSidebar(page, false);
      await expect(series).toHaveAttribute("aria-current", "page");

      await sidebar(page).getByRole("button", { name: COLLAPSE }).click();
      await expectSidebar(page, true);

      // --- 390 px: the drawer.
      await page.setViewportSize(PHONE);
      await expectDrawerClosed(page);
      await openDrawer(page);
      const dialog = page.getByRole("dialog", { name: DRAWER_DIALOG });
      await expect(dialog.getByRole("navigation", { name: NAV })).toBeVisible();
      await dialog.getByRole("link", { name: LOCATIONS }).click();
      await expect(page).toHaveURL(/\/locations$/);
      await expectDrawerClosed(page);

      await openDrawer(page);
      await page
        .getByRole("dialog", { name: DRAWER_DIALOG })
        .getByRole("button", { name: LOG_OUT })
        .click();
      await page.waitForURL((u) => u.pathname === "/");
      await openDrawer(page);
      const anonymous = page.getByRole("dialog", { name: DRAWER_DIALOG });
      await expect(anonymous.getByRole("button", { name: LOG_OUT })).toHaveCount(0);
      await anonymous.getByRole("link", { name: LOG_IN }).click();
      await expect(page).toHaveURL(/\/login$/);
      await expect(page.locator("#username")).toBeVisible();
      await expect(page.locator("#login-submit")).toBeVisible();
    } finally {
      await agent.dispose();
      // Revoking the key turns its pending proposal stale.
      await revokeProposeKey(setup, key.label);
      await setupContext.close();
    }
  });
});
