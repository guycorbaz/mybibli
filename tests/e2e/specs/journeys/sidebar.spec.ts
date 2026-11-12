/**
 * Story 14-1 E2E — the navigation sidebar on wide screens (#547,
 * UX-DR38), against the approved mock-up
 * `_bmad-output/planning-artifacts/ux-sidebar-mockup.html`.
 *
 *   - collapse / expand, persistence across a reload, applied before the
 *     first paint, blocked storage = expanded;
 *   - collapsed: tooltip on hover AND on keyboard focus, names kept;
 *   - every entry and foot control reachable by Tab, in both states;
 *   - the Proposals badge in both states (pill on the icon collapsed);
 *   - no label overflows at 1024 / 1280 / 1440 px in fr / en / de / it
 *     with an admin's full set (worst-case `99+` badge forced);
 *   - axe in both states, light and dark;
 *   - /catalog: after a toggle (mouse or keyboard) the focus is back in
 *     the scan field, the next scan lands in it, feedback entries stay;
 *   - the theme toggle really switches the `dark:` utilities (review).
 *
 * The state lives in localStorage (per browser): every test starts from
 * a fresh context, so from the expanded default.
 *
 * Spec ID "SD" for ISBNs. The badge test files one metadata proposal on
 * this spec's own title, rejects it and revokes its key (the rejection
 * blocks nothing elsewhere). Creates no series (#528).
 */
import { test, expect, Page, request as playwrightRequest } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { checkAccessibility } from "../../helpers/accessibility";
import { simulateScan, waitForDuplicateScanWindow } from "../../helpers/scanner";
import { specIsbn } from "../../helpers/isbn";
import { mcpCallTool } from "../../helpers/mcp";
import { newProposeKey, revokeProposeKey, scanSettledTitle } from "../../helpers/proposals";

const SIDEBAR = "#sidebar";
const TOGGLE = "[data-sidebar-toggle]";
const EXPANDED_PX = 224;
const COLLAPSED_PX = 64;
const LOCALES = ["fr", "en", "de", "it"] as const;
const EXPAND_NAME = /^(Expand menu|Déployer le menu|Menü ausklappen|Espandi menu)$/;
const COLLAPSE_NAME = /^(Collapse menu|Réduire le menu|Menü einklappen|Comprimi menu)$/;
const ADMIN_HREFS = [
  "/",
  "/catalog",
  "/locations",
  "/series",
  "/borrowers",
  "/loans",
  "/wishlist",
  "/labels",
  "/proposals",
  "/admin",
];

async function sidebarWidth(page: Page): Promise<number> {
  return page.locator(SIDEBAR).evaluate((el) => Math.round(el.getBoundingClientRect().width));
}

/** Width once the 150 ms width transition has run (motion-safe). */
async function expectWidth(page: Page, px: number) {
  await expect.poll(() => sidebarWidth(page)).toBe(px);
}

async function collapse(page: Page) {
  await page.locator(TOGGLE).click();
  await expectWidth(page, COLLAPSED_PX);
  await expect(page.locator(TOGGLE)).toHaveAttribute("aria-expanded", "false");
}

function withLang(path: string, lang: string): string {
  return `${path}${path.includes("?") ? "&" : "?"}lang=${lang}`;
}

/** Tab from the logo through the sidebar; returns what took focus, in
 *  order, until focus leaves it. */
async function tabThroughSidebar(page: Page): Promise<string[]> {
  // Story 14-2 — the logo link sits in the head row, beside the drawer's close button.
  await page.locator(`${SIDEBAR} a[href="/"]`).first().focus();
  const seen: string[] = [];
  for (let i = 0; i < 30; i++) {
    await page.keyboard.press("Tab");
    const id = await page.evaluate(() => {
      const el = document.activeElement as HTMLElement | null;
      if (!el || !document.getElementById("sidebar")!.contains(el)) return null;
      if (el.hasAttribute("data-sidebar-toggle")) return "toggle";
      if (el.id) return `#${el.id}`;
      if (el.closest('form[action="/logout"]')) return "logout";
      return el.getAttribute("href") ?? el.tagName;
    });
    if (id === null) break;
    seen.push(id);
  }
  return seen;
}

test.describe("Story 14-1 — the sidebar on wide screens", () => {
  test("collapse / expand, remembered across a reload, applied before the first paint", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/catalog");
    const toggle = page.locator(TOGGLE);

    // Starts expanded: 224 px, labels visible, no top bar at ≥ lg.
    await expectWidth(page, EXPANDED_PX);
    await expect(page.locator("header")).toBeHidden();
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(toggle).toHaveAttribute("aria-controls", "sidebar");
    await expect(toggle).toHaveAccessibleName(COLLAPSE_NAME);
    await expect(page.locator(`${SIDEBAR} a[href="/catalog"]`)).toHaveAccessibleName(
      /Catalog|Catalogue|Katalog|Catalogo/,
    );

    await collapse(page);
    // Review — one fixed accessible name; aria-expanded carries the state.
    await expect(toggle).toHaveAccessibleName(COLLAPSE_NAME);
    // Icons only, names kept.
    const series = page.locator(`${SIDEBAR} a[href="/series"]`);
    await expect(series).toHaveAccessibleName(/^(Series|Séries|Serien|Serie)$/);
    expect(await page.evaluate(() => localStorage.getItem("mybibli.sidebar"))).toBe("collapsed");

    // The first frame of the next page already has the remembered width:
    // measured in the first animation frame after the sidebar is parsed
    // (rAF callbacks run before the paint).
    await page.addInitScript(() => {
      const w = window as unknown as { __firstSidebarWidth?: number };
      new MutationObserver((_, obs) => {
        const el = document.getElementById("sidebar");
        if (!el) return;
        obs.disconnect();
        requestAnimationFrame(() => {
          w.__firstSidebarWidth = el.getBoundingClientRect().width;
        });
      }).observe(document, { childList: true, subtree: true });
    });
    await page.reload();
    await expect
      .poll(() =>
        page.evaluate(
          () => (window as unknown as { __firstSidebarWidth?: number }).__firstSidebarWidth,
        ),
      )
      .toBe(COLLAPSED_PX);
    await expect(toggle).toHaveAttribute("aria-expanded", "false");

    // The content takes the remaining width.
    const mainLeft = await page
      .locator("#main-content")
      .evaluate((el) => Math.round(el.getBoundingClientRect().left));
    expect(mainLeft).toBe(COLLAPSED_PX);

    await toggle.click();
    await expectWidth(page, EXPANDED_PX);
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await page.reload();
    await expectWidth(page, EXPANDED_PX);
  });

  test("blocked storage means expanded, and both toggles still work", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(e.message));
    await page.addInitScript(() => {
      const deny = () => {
        throw new DOMException("blocked", "SecurityError");
      };
      Storage.prototype.getItem = deny;
      Storage.prototype.setItem = deny;
    });
    await loginAs(page, "librarian");
    await page.goto("/series");
    await expectWidth(page, EXPANDED_PX);
    await collapse(page);
    // Review regression — theme.js used to throw on blocked storage before
    // wiring its toggle.
    const dark = () => page.evaluate(() => document.documentElement.classList.contains("dark"));
    const before = await dark();
    await page.locator("#theme-toggle").click();
    expect(await dark()).toBe(!before);
    await page.reload();
    await expectWidth(page, EXPANDED_PX);
    expect(errors).toEqual([]);
  });

  test("collapsed: a tooltip on hover and on keyboard focus, none when expanded", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/locations");
    const tipOf = (href: string) => page.locator(`${SIDEBAR} a[href="${href}"] [data-nav-tooltip]`);

    // Expanded: the label shows, the tooltip never does.
    await page.locator(`${SIDEBAR} a[href="/loans"]`).hover();
    await expect(tipOf("/loans")).toBeHidden();

    await collapse(page);
    await page.locator(`${SIDEBAR} a[href="/loans"]`).hover();
    await expect(tipOf("/loans")).toBeVisible();
    await expect(tipOf("/loans")).toHaveText(/^(Loans|Prêts|Ausleihen|Prestiti)$/);
    // Beside the rail, not clipped inside it.
    const box = (await tipOf("/loans").boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(COLLAPSED_PX);
    await page.mouse.move(800, 500);
    await expect(tipOf("/loans")).toBeHidden();

    // Keyboard focus: Tab from Home to Catalog.
    await page.locator(`${SIDEBAR} a[href="/"]`).last().focus();
    await page.keyboard.press("Tab");
    await expect(page.locator(`${SIDEBAR} a[href="/catalog"]`)).toBeFocused();
    await expect(tipOf("/catalog")).toBeVisible();
    await page.keyboard.press("Tab");
    await expect(tipOf("/catalog")).toBeHidden();
    await expect(tipOf("/locations")).toBeVisible();

    // Foot controls too: the collapse toggle's tooltip names the next action.
    await page.locator(TOGGLE).focus();
    await page.keyboard.press("Shift+Tab");
    await page.keyboard.press("Tab");
    await expect(page.locator(`${TOGGLE} [data-nav-tooltip]`)).toBeVisible();
    await expect(page.locator(`${TOGGLE} [data-nav-tooltip]`)).toHaveText(EXPAND_NAME);
  });

  test("every entry and foot control is reachable by keyboard, in both states", async ({
    page,
  }) => {
    await loginAs(page, "admin");
    await page.goto("/");
    const expected = [...ADMIN_HREFS, "#lang-select", "#theme-toggle", "logout", "toggle"];
    expect(await tabThroughSidebar(page)).toEqual(expected);
    await collapse(page);
    expect(await tabThroughSidebar(page)).toEqual(expected);
    // Toggling from the keyboard keeps the focus on the toggle.
    await page.locator(TOGGLE).focus();
    await page.keyboard.press("Enter");
    await expectWidth(page, EXPANDED_PX);
    await expect(page.locator(TOGGLE)).toBeFocused();
  });

  test("current page: aria-current plus a left bar", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.goto("/wishlist");
    const current = page.locator(`${SIDEBAR} [aria-current="page"]`);
    await expect(current).toHaveCount(1);
    await expect(current).toHaveAttribute("href", "/wishlist");
    const bar = await current.evaluate((el) => {
      const s = getComputedStyle(el, "::before");
      return { width: s.width, position: s.position, weight: getComputedStyle(el).fontWeight };
    });
    expect(bar.width).toBe("3px");
    expect(bar.position).toBe("absolute");
    expect(Number(bar.weight)).toBeGreaterThanOrEqual(600);
  });

  for (const width of [1024, 1280, 1440]) {
    test(`admin at ${width} px — no entry overflows in fr / en / de / it`, async ({ page }) => {
      test.setTimeout(60000);
      await loginAs(page, "admin");
      await page.setViewportSize({ width, height: 900 });
      for (const lang of LOCALES) {
        await page.goto(withLang("/admin", lang));
        await expect(page.locator("html")).toHaveAttribute("lang", lang);
        const fit = await page.evaluate(() => {
          const side = document.getElementById("sidebar")!;
          // Worst-case badge (`utils::proposals_badge_text` caps at 99+).
          const entry = side.querySelector('a[href="/proposals"]')!;
          let badge = entry.querySelector("[data-proposals-badge]");
          if (!badge) {
            const tpl = document.createElement("span");
            tpl.setAttribute("data-proposals-badge", "");
            tpl.setAttribute("aria-hidden", "true");
            tpl.className =
              "ml-auto inline-flex items-center justify-center min-w-5 h-5 px-1.5 rounded-full bg-amber-100 text-amber-800 text-xs font-semibold tabular-nums";
            entry.insertBefore(tpl, entry.querySelector("[data-nav-tooltip]"));
            badge = tpl;
          }
          badge.textContent = "99+";
          const sideRect = side.getBoundingClientRect();
          const rows = Array.from(
            side.querySelectorAll<HTMLElement>("a, button, form[action='/language']"),
          ).filter((r) => r.getClientRects().length > 0);
          const problems: string[] = [];
          for (const row of rows) {
            const r = row.getBoundingClientRect();
            if (r.right > sideRect.right + 0.5) problems.push(`row past the edge: ${row.textContent!.trim()}`);
            if (row.scrollWidth > row.clientWidth + 1) problems.push(`row overflows: ${row.textContent!.trim()}`);
            for (const child of Array.from(row.children) as HTMLElement[]) {
              if (child.hasAttribute("data-nav-tooltip") || child.tagName === "INPUT") continue;
              const c = child.getBoundingClientRect();
              if (c.width === 0) continue;
              if (c.right > r.right + 0.5) problems.push(`clipped: ${child.textContent!.trim()}`);
              if (c.height > 28 && child.tagName === "SPAN") problems.push(`wraps: ${child.textContent!.trim()}`);
            }
          }
          return {
            problems,
            sideOverflow: side.scrollWidth - side.clientWidth,
            pageOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
            width: Math.round(sideRect.width),
          };
        });
        const label = `${lang} ${width}px`;
        expect(fit.width, label).toBe(EXPANDED_PX);
        expect(fit.problems, label).toEqual([]);
        expect(fit.sideOverflow, `${label}: sidebar overflows`).toBeLessThanOrEqual(0);
        expect(fit.pageOverflow, `${label}: page scrolls sideways`).toBeLessThanOrEqual(1);
      }
    });
  }

  test("axe passes expanded and collapsed, light and dark", async ({ page }) => {
    await loginAs(page, "admin");
    for (const scheme of ["light", "dark"] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.goto("/catalog");
      await expectWidth(page, EXPANDED_PX);
      await checkAccessibility(page);
      await collapse(page);
      await checkAccessibility(page);
      // Show a tooltip too: it must not add a violation.
      await page.locator(`${SIDEBAR} a[href="/series"]`).hover();
      await checkAccessibility(page);
      await page.locator(TOGGLE).click();
      await expectWidth(page, EXPANDED_PX);
    }
  });

  test("on /catalog, toggling keeps the scan field's focus and the feedback", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.goto("/catalog");
    const scan = page.locator("#scan-field");
    // AC8 (review, round 2) — a mouse click never takes the focus; from
    // the keyboard it returns to the scan field only if the scan field
    // held it right before the toggle.
    await simulateScan(page, "#scan-field", specIsbn("SD", 1));
    // The first answer is a skeleton, replaced in place once the metadata
    // arrive: count both.
    const feedback = page.locator(
      "#feedback-list .feedback-skeleton, #feedback-list .feedback-entry",
    );
    await expect(feedback.first()).toBeVisible({ timeout: 10000 });
    const before = await feedback.count();
    await scan.focus();

    // A mouse click on the toggle does not take the focus.
    await page.locator(TOGGLE).click();
    await expectWidth(page, COLLAPSED_PX);
    await expect(scan).toBeFocused();
    await expect(feedback).toHaveCount(before);

    // The next scan lands in the scan field.
    await waitForDuplicateScanWindow(page);
    await page.keyboard.type(specIsbn("SD", 2), { delay: 20 });
    await page.keyboard.press("Enter");
    await expect.poll(() => feedback.count(), { timeout: 10000 }).toBeGreaterThan(before);

    await page.locator(TOGGLE).click();
    await expectWidth(page, EXPANDED_PX);
    await expect(scan).toBeFocused();

    // From the keyboard too: scan field → toggle, Enter, focus back in the
    // field, and the Enter does not reach it as a scan.
    const afterScans = await feedback.count();
    await scan.focus();
    await page.locator(TOGGLE).focus();
    await page.keyboard.press("Enter");
    await expectWidth(page, COLLAPSED_PX);
    await expect(scan).toBeFocused();
    await expect(feedback).toHaveCount(afterScans);
  });

  test("on /catalog, a form input keeps the focus across a toggle", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.goto("/catalog");
    await page.locator("#new-title-btn").click();
    const title = page.locator("#title-form-container #title-field");
    await expect(title).toBeVisible({ timeout: 10000 });
    await title.focus();
    await title.fill("Sidebar focus");

    // Mouse: the input keeps the focus, typing continues in it.
    await page.locator(TOGGLE).click();
    await expectWidth(page, COLLAPSED_PX);
    await expect(title).toBeFocused();
    await page.keyboard.type(" kept");
    await expect(title).toHaveValue("Sidebar focus kept");
    await expect(page.locator("#scan-field")).toHaveValue("");

    // Keyboard, arriving from the form: focus stays on the toggle.
    await page.locator(TOGGLE).focus();
    await page.keyboard.press("Enter");
    await expectWidth(page, EXPANDED_PX);
    await expect(page.locator(TOGGLE)).toBeFocused();
  });

  test("the theme toggle switches the page to dark and back, remembered across a reload", async ({
    page,
  }) => {
    // Review regression — `dark:` utilities followed only the OS scheme,
    // so the toggle changed nothing on most of the page.
    await page.emulateMedia({ colorScheme: "light" });
    await loginAs(page, "librarian");
    await page.goto("/series");
    const bg = () =>
      page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    const sidebarBg = () =>
      page.locator(SIDEBAR).evaluate((el) => getComputedStyle(el).backgroundColor);
    const lightBody = await bg();
    const lightSidebar = await sidebarBg();
    await page.locator("#theme-toggle").click();
    await expect(page.locator("html")).toHaveClass(/(^|\s)dark(\s|$)/);
    await expect.poll(bg).not.toBe(lightBody);
    await expect.poll(sidebarBg).not.toBe(lightSidebar);
    const darkBody = await bg();
    // Native controls follow (round 2).
    expect(
      await page.evaluate(() => getComputedStyle(document.documentElement).colorScheme),
    ).toBe("dark");
    // Printing in the dark theme keeps the light palette (round 2).
    await page.emulateMedia({ media: "print" });
    await expect.poll(bg).toBe(lightBody);
    await page.emulateMedia({ media: "screen" });
    await expect.poll(bg).toBe(darkBody);
    await page.reload();
    await expect.poll(bg).toBe(darkBody);
    // And back to light, still under a light OS preference.
    await page.locator("#theme-toggle").click();
    await expect.poll(bg).toBe(lightBody);
    await page.reload();
    await expect.poll(bg).toBe(lightBody);
  });

  test("the Proposals badge shows in both states, a pill on the icon when collapsed", async ({
    page,
    baseURL,
  }) => {
    test.setTimeout(90000);
    const key = await newProposeKey(page, "sidebar");
    const agent = await playwrightRequest.newContext({ baseURL: baseURL! });
    try {
      await page.context().clearCookies();
      await loginAs(page, "librarian");
      const titleId = await scanSettledTitle(page, agent, key.plaintext, "SD", 10 + test.info().retry);
      const version = (await mcpCallTool(agent, key.plaintext, "get_title", { id: titleId }))
        .structuredContent.version;
      const filed = await mcpCallTool(agent, key.plaintext, "propose_title_metadata", {
        title_id: titleId,
        version,
        fields: { subtitle: `Sidebar ${Date.now()}` },
        sources: ["https://example.org/sidebar"],
      });
      expect(filed.isError, JSON.stringify(filed.content)).toBe(false);
      const proposalId = filed.structuredContent.proposal_id as number;

      await page.goto("/series");
      const entry = page.locator(`${SIDEBAR} a[href="/proposals"]`);
      const badge = entry.locator("[data-proposals-badge]");
      await expect(badge).toBeVisible();
      await expect(badge).toHaveText(/^(\d+|99\+)$/);
      await expect(entry).toHaveAccessibleName(/\d+/);
      // Expanded: right of the label.
      const label = entry.locator("span").first();
      expect((await badge.boundingBox())!.x).toBeGreaterThan((await label.boundingBox())!.x);

      await collapse(page);
      await expect(badge).toBeVisible();
      await expect(entry).toHaveAccessibleName(/\d+/);
      const pill = (await badge.boundingBox())!;
      const icon = (await entry.locator("svg").boundingBox())!;
      expect(pill.x + pill.width).toBeLessThanOrEqual(COLLAPSED_PX);
      // On the icon: the boxes overlap.
      expect(pill.x).toBeLessThan(icon.x + icon.width);
      expect(pill.y).toBeLessThan(icon.y + icon.height);
      await entry.hover();
      await expect(entry.locator("[data-nav-tooltip]")).toHaveText(/\((\d+|99\+)\)$/);

      // Clean up: reject it (blocks only this spec's title).
      await page.goto(`/proposals/${proposalId}`);
      await page.getByRole("button", { name: /^(Reject|Refuser|Ablehnen|Rifiuta)$/ }).click();
      const dialog = page.locator("#modal-slot dialog");
      await expect(dialog).toBeVisible();
      await dialog.locator("[data-modal-confirm]").click();
      await expect(page.locator("#proposal-detail-body")).toHaveAttribute(
        "data-proposal-status",
        "rejected",
      );
    } finally {
      await agent.dispose();
      await revokeProposeKey(page, key.label);
    }
  });
});
