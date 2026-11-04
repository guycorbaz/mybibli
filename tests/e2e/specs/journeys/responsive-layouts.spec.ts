/**
 * Story 9-21 — responsive per-page layouts E2E; tightened by story 11-21,
 * re-cut by story 14-1.
 *
 * Story 11-21 (D-11-21-1) moved the nav bar's desktop-strip / hamburger
 * switch from `lg` to `xl` (1280 px). Story 14-1 (#547, UX-DR38) replaced
 * the strip with the sidebar from `lg` (1024 px): below `lg` the small
 * bar (logo, language, theme, hamburger) shows; from `lg` the sidebar and
 * no bar. As an Admin (the longest link set, worst-case `99+` badge), in
 * all four locales, at 360, 1023, 1024 and 1920 px:
 *   - no horizontal page overflow beyond 1 px (was 32 px, which hid
 *     #545's 18 px bar overflow at phone width);
 *   - below `lg` the bar shows and does not overflow, its right-hand
 *     group stays on screen, the hamburger shows, the sidebar is hidden;
 *     from `lg` the reverse (the sidebar's own fit at 1024 / 1280 / 1440
 *     is measured entry by entry in sidebar.spec.ts).
 * Plus the tablet (768 px, librarian) and anonymous phone checks of 9-21.
 *
 * The locale comes from `?lang=` (highest-priority source, no write):
 * switching through the toggle would store the seeded admin's
 * `preferred_language`, shared with every parallel spec.
 *
 * Spec ID "RL" — no ISBNs generated.
 */
import { test, expect, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { checkAccessibility } from "../../helpers/accessibility";

const MOBILE = { width: 375, height: 667 };
const TABLET = { width: 768, height: 1024 };
const DESKTOP = { width: 1280, height: 800 };
const PHONE = { width: 360, height: 740 };

/** Widths around the `lg` switch (1024 px) plus a phone and a wide screen. */
const NAV_WIDTHS = [360, 1023, 1024, 1920] as const;
const LG = 1024;
const LOCALES = ["fr", "en", "de", "it"] as const;
const ADMIN_PAGES = ["/", "/catalog", "/series", "/proposals", "/loans", "/borrowers"];

async function assertNoHorizontalScroll(page: Page, label: string) {
  const overflow = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    clientWidth: document.documentElement.clientWidth,
  }));
  // 1 px for sub-pixel rounding only (story 11-21; was 32 px).
  const overflowAmount = overflow.scrollWidth - overflow.clientWidth;
  expect(
    overflowAmount,
    `${label}: horizontal overflow (scrollWidth=${overflow.scrollWidth}, clientWidth=${overflow.clientWidth})`,
  ).toBeLessThanOrEqual(1);
}

interface NavFit {
  barShown: boolean;
  sidebarShown: boolean;
  toggleShown: boolean;
  navOverflow: number;
  rightGroupOverflow: number;
}

/** Measures the small-screen bar and which navigation shows. */
async function measureNav(page: Page): Promise<NavFit> {
  await page.waitForLoadState("load");
  return page.evaluate(() => {
    const header = document.querySelector("header")!;
    const bar = header.querySelector('nav[aria-label="Main navigation"]')!;
    const sidebar = document.getElementById("sidebar")!;
    const toggle = document.getElementById("mobile-menu-toggle")!;
    const barShown = getComputedStyle(header).display !== "none";
    const right = (bar.querySelector(":scope > div.ml-auto") as HTMLElement).getBoundingClientRect();
    return {
      barShown,
      sidebarShown: getComputedStyle(sidebar).display !== "none",
      toggleShown: barShown && getComputedStyle(toggle).display !== "none",
      navOverflow: barShown ? bar.scrollWidth - bar.clientWidth : 0,
      rightGroupOverflow: barShown ? right.right - window.innerWidth : 0,
    };
  });
}

function withLang(path: string, lang: string): string {
  return `${path}${path.includes("?") ? "&" : "?"}lang=${lang}`;
}

test.describe("Story 9-21 / 11-21 / 14-1 — responsive layouts and the navigation", () => {
  for (const lang of LOCALES) {
    test(`admin, ${lang} — the navigation fits and no page scrolls sideways at 360 / 1023 / 1024 / 1920 px`, async ({
      page,
    }) => {
      test.setTimeout(90000);
      await loginAs(page, "admin");
      for (const width of NAV_WIDTHS) {
        await page.setViewportSize({ width, height: 800 });
        for (const path of ADMIN_PAGES) {
          await page.goto(withLang(path, lang));
          await expect(page.locator("html")).toHaveAttribute("lang", lang);
          const label = `${path} ${lang} ${width}px`;
          const fit = await measureNav(page);
          const wide = width >= LG;
          expect(fit.sidebarShown, `${label}: sidebar shown`).toBe(wide);
          expect(fit.barShown, `${label}: small-screen bar shown`).toBe(!wide);
          expect(fit.toggleShown, `${label}: hamburger shown`).toBe(!wide);
          expect(fit.navOverflow, `${label}: the bar overflows`).toBeLessThanOrEqual(1);
          expect(fit.rightGroupOverflow, `${label}: right-hand group off screen`).toBeLessThanOrEqual(1);
          await assertNoHorizontalScroll(page, label);
        }
      }
    });
  }

  test("anonymous, phone (360 / 375 px) — home and catalog fit", async ({ page }) => {
    for (const viewport of [PHONE, MOBILE]) {
      await page.setViewportSize(viewport);
      await page.goto("/");
      await assertNoHorizontalScroll(page, `/ anonymous ${viewport.width}px`);
      await expect(page.locator("#search-field")).toBeVisible();
      const fit = await measureNav(page);
      expect(fit.navOverflow, "anonymous bar overflows").toBeLessThanOrEqual(1);
      expect(fit.rightGroupOverflow).toBeLessThanOrEqual(1);
      await page.goto("/catalog");
      await assertNoHorizontalScroll(page, `/catalog anonymous ${viewport.width}px`);
    }
  });

  test("tablet (768px) — no horizontal scroll on key surfaces", async ({ page }) => {
    await page.setViewportSize(TABLET);
    await loginAs(page, "librarian");
    for (const path of ["/borrowers", "/loans", "/", "/proposals"]) {
      await page.goto(path);
      await assertNoHorizontalScroll(page, `${path} tablet`);
    }
  });
});

/**
 * Story 11-4 (#496) — sticky navigation bar. Story 14-1 (#547): from `lg`
 * the navigation is the sidebar (sticky, full height, beside the content),
 * so the "desktop" cases measure the sidebar — the landmark named "Main
 * navigation" that is displayed — and expect no offset above the content.
 *
 * The bar lives in the shared layout, so a few representative pages are
 * enough. Each page is made reliably scrollable through the CSSOM
 * (`style.minHeight` on #main-content — allowed by CSP, unlike an inline
 * `style` attribute) so the assertions never depend on how much data the
 * shared E2E database happens to hold. Read-only: no entity is created.
 */
/** The displayed navigation landmark: the sidebar from `lg`, the bar below. */
const NAV = 'nav[aria-label="Main navigation"]:visible';

/** scrollY after two animation frames, so any scroll triggered by the last
 *  action (focus, Escape) has been applied before it is read. */
async function settledScrollY(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      new Promise<number>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve(window.scrollY))),
      ),
  );
}
const LANDSCAPE_PHONE = { width: 667, height: 375 };

interface StuckSample {
  scrollY: number;
  navTop: number;
  navBottom: number;
  panelTop: number | null;
  panelOverflows: boolean;
  menuLinkOnTop: boolean;
  headerCoveredAtCentre: boolean;
  lostOverlayOnTopOfBar: boolean;
}

/**
 * Scrolls a (forcibly) long page to the bottom and measures the bar in the
 * SAME synchronous task. A late native `autofocus` (e.g. the catalog scan
 * field under parallel load) can scroll the document back to the top at
 * any moment, and at scrollY 0 a non-sticky bar also sits at y 0 — so a
 * measurement taken in a separate call could pass vacuously. Polls until
 * a sample is genuinely scrolled, then returns that sample.
 */
async function scrollToBottomAndMeasure(page: Page): Promise<StuckSample> {
  await page.waitForLoadState("load");
  let sample: StuckSample | undefined;
  await expect
    .poll(async () => {
      sample = await page.evaluate((): StuckSample => {
        const main = document.getElementById("main-content");
        if (main) main.style.minHeight = "3000px";
        window.scrollTo(0, document.documentElement.scrollHeight);
        // Story 14-1 — the displayed one: sidebar from `lg`, bar below.
        const nav = Array.from(
          document.querySelectorAll('nav[aria-label="Main navigation"]'),
        ).find((n) => n.getClientRects().length > 0)!;
        const r = nav.getBoundingClientRect();
        const header = nav.closest("header") ?? nav;
        const panel = document.getElementById("mobile-nav")!;
        const panelOpen = !panel.classList.contains("hidden");
        const link = panel.querySelector('a[href="/series"]');
        let menuLinkOnTop = false;
        if (panelOpen && link) {
          const l = link.getBoundingClientRect();
          const hit = document.elementFromPoint(l.left + l.width / 2, l.top + l.height / 2);
          menuLinkOnTop = hit !== null && link.contains(hit);
        }
        const centreHit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
        const edgeHit = document.elementFromPoint(r.left + 8, r.top + r.height / 2);
        const lost = document.getElementById("connection-lost-overlay");
        return {
          scrollY: window.scrollY,
          navTop: r.top,
          navBottom: r.bottom,
          panelTop: panelOpen ? panel.getBoundingClientRect().top : null,
          panelOverflows: panelOpen && panel.scrollHeight > panel.clientHeight,
          menuLinkOnTop,
          headerCoveredAtCentre: centreHit !== null && !header.contains(centreHit),
          lostOverlayOnTopOfBar: lost !== null && edgeHit !== null && lost.contains(edgeHit),
        };
      });
      return sample.scrollY;
    })
    .toBeGreaterThan(1000);
  return sample!;
}

async function assertNavStuckAtTop(page: Page, label: string): Promise<StuckSample> {
  const sample = await scrollToBottomAndMeasure(page);
  expect(Math.abs(sample.navTop), `${label}: nav top at the viewport top`).toBeLessThanOrEqual(1);
  return sample;
}

test.describe("Story 11-4 — sticky navigation bar", () => {
  for (const [name, viewport] of [
    ["mobile", MOBILE],
    ["tablet", TABLET],
    ["desktop", DESKTOP],
  ] as const) {
    test(`${name} (${viewport.width}px) — the bar stays at the top after scrolling to the bottom`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      await loginAs(page, "admin");
      for (const path of ["/", "/catalog", "/series", "/admin"]) {
        await page.goto(path);
        const sample = await assertNavStuckAtTop(page, `${path} ${name}`);
        // … and painted on top: hit testing at its centre lands in <header>.
        expect(sample.headerCoveredAtCentre, `${path} ${name}: bar on top`).toBe(false);
      }
    });
  }

  for (const [name, viewport] of [
    ["mobile", MOBILE],
    ["tablet", TABLET],
    ["desktop", DESKTOP],
  ] as const) {
    test(`${name} — the skip link target is not hidden under the bar`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      await loginAs(page, "admin");
      await page.goto("/catalog");
      await assertNavStuckAtTop(page, `/catalog ${name} before skip`);

      const skip = page.locator('a[href="#main-content"]');
      await skip.focus();
      // The focused link itself is visible (z-50, above the bar).
      await expect(skip).toBeInViewport();
      const skipOnTop = await skip.evaluate((el) => {
        const r = el.getBoundingClientRect();
        const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
        return top !== null && el.contains(top);
      });
      expect(skipOnTop, "the focused skip link must not be covered by the bar").toBe(true);
      await page.keyboard.press("Enter");
      await expect.poll(() => page.evaluate(() => location.hash)).toBe("#main-content");
      // Let the fragment scroll settle (two frames), then measure once.
      const geometry = await page.evaluate(
        () =>
          new Promise<{ mainTop: number; navBottom: number }>((resolve) =>
            requestAnimationFrame(() =>
              requestAnimationFrame(() => {
                // Story 14-1 — from `lg` the sidebar sits beside the
                // content: nothing above it, the target lands at the top.
                const header = document.querySelector("header")!;
                const barShown = getComputedStyle(header).display !== "none";
                resolve({
                  mainTop: document.getElementById("main-content")!.getBoundingClientRect().top,
                  navBottom: barShown ? header.getBoundingClientRect().bottom : 0,
                });
              }),
            ),
          ),
      );
      await expect(page.locator(NAV)).toBeInViewport();
      expect(
        geometry.mainTop,
        `main top (${geometry.mainTop}) must sit below the bar bottom (${geometry.navBottom})`,
      ).toBeGreaterThanOrEqual(geometry.navBottom - 1);
      if (geometry.navBottom === 0) {
        expect(Math.abs(geometry.mainTop), "no bar: the target lands at the top").toBeLessThanOrEqual(
          1,
        );
      }
    });
  }

  for (const [name, viewport] of [
    ["portrait phone", MOBILE],
    ["landscape phone", LANDSCAPE_PHONE],
    ["tablet", TABLET],
  ] as const) {
    test(`${name} — the mobile menu stays usable and above the content while stuck`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      await loginAs(page, "admin");
      await page.goto("/catalog");
      await assertNavStuckAtTop(page, `${name} before menu`);

      await page.locator("#mobile-menu-toggle").click();
      const panel = page.locator("#mobile-nav");
      await expect(panel).toBeVisible();
      const sample = await assertNavStuckAtTop(page, `${name} menu open`);
      // The panel hangs right below the stuck bar …
      expect(sample.panelTop).not.toBeNull();
      expect(Math.abs(sample.panelTop! - sample.navBottom)).toBeLessThanOrEqual(1);
      // … above the page content: hit testing at a link's centre lands on
      // that link, not on whatever scrolled underneath.
      expect(sample.menuLinkOnTop, "the menu link must be the topmost element").toBe(true);
      // On a landscape phone the admin menu is taller than the space left
      // under the bar: the panel must really overflow and scroll on its own.
      if (viewport === LANDSCAPE_PHONE) {
        expect(sample.panelOverflows, "the admin menu must overflow a 375 px-high viewport").toBe(
          true,
        );
      }
      const logout = panel.locator('form[action="/logout"] button');
      await logout.scrollIntoViewIfNeeded();
      await expect(logout).toBeInViewport();

      await checkAccessibility(page);

      await panel.locator('a[href="/series"]').click();
      await expect(page).toHaveURL(/\/series$/);
    });
  }

  // D-11-4-8 — a root `scroll-padding-top` made Chromium scroll the page up
  // whenever a control inside the stuck bar took focus. The reader must
  // keep their place.
  test("desktop — focusing a link in the stuck bar does not move the page", async ({
    page,
  }) => {
    await page.setViewportSize(DESKTOP);
    await loginAs(page, "admin");
    await page.goto("/series");
    const before = await assertNavStuckAtTop(page, "/series before focus");
    await page.locator(`${NAV} a[href="/catalog"]`).focus();
    await page.keyboard.press("Shift+Tab");
    const after = await settledScrollY(page);
    expect(Math.abs(after - before.scrollY), "the page must not jump").toBeLessThanOrEqual(1);
  });

  test("mobile — closing the stuck menu with Escape does not move the page", async ({
    page,
  }) => {
    await page.setViewportSize(MOBILE);
    await loginAs(page, "admin");
    await page.goto("/series");
    // Baseline taken BEFORE opening: the open panel is in the header's flow,
    // so re-scrolling to the bottom with it open would make the closing
    // shrink clamp scrollY and look like a jump.
    const before = await assertNavStuckAtTop(page, "/series before menu");
    await page.locator("#mobile-menu-toggle").click();
    await expect(page.locator("#mobile-nav")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator("#mobile-nav")).toBeHidden();
    await expect(page.locator("#mobile-menu-toggle")).toBeFocused();
    const after = await settledScrollY(page);
    expect(Math.abs(after - before.scrollY), "the page must not jump").toBeLessThanOrEqual(1);
  });

  // D-11-4-8 — the bar offset is a scroll-margin on every element inside
  // <main>, and scroll-margin applies to EVERY scroll container: inside an
  // inner vertical scroller it would keep a focused row 48 px below the
  // scroller's own top. Inner scrollers carry `**:scroll-mt-0!`. Real
  // compiled classes; geometry through the CSSOM. The control box (no
  // reset) proves the inherited margin is there in the first place.
  // Story 14-1 — measured at tablet width: from `lg` the inherited margin
  // is 0 (no bar above the content), so the control box would prove nothing.
  test("inner scrollers inside <main> keep their rows in place on focus", async ({
    page,
  }) => {
    await page.setViewportSize(TABLET);
    await loginAs(page, "admin");
    await page.goto("/series");
    await page.waitForLoadState("load");
    const scrollTops = await page.evaluate(async () => {
      const frames = () =>
        new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())));
      const main = document.getElementById("main-content")!;
      const make = (reset: boolean) => {
        const box = document.createElement("div");
        if (reset) box.className = "**:scroll-mt-0!";
        box.style.maxHeight = "288px";
        box.style.overflowY = "auto";
        for (let i = 0; i < 20; i++) {
          const b = document.createElement("button");
          b.type = "button";
          b.textContent = `row ${i}`;
          b.style.display = "block";
          b.style.height = "40px";
          box.appendChild(b);
        }
        main.prepend(box);
        return box;
      };
      const result: Record<string, number> = {};
      for (const reset of [false, true]) {
        window.scrollTo(0, 0);
        const box = make(reset);
        box.scrollTop = 200; // row 5 sits exactly at the box's top edge
        await frames();
        (box.children[5] as HTMLElement).focus();
        await frames();
        result[reset ? "reset" : "control"] = box.scrollTop;
        box.remove();
      }
      return result;
    });
    expect(scrollTops.control, "without the reset the row is pushed 56 px down").toBeLessThan(200);
    expect(scrollTops.reset, "with the reset the list stays put").toBe(200);
  });

  test("an open dialog renders above the stuck bar", async ({ page }) => {
    await page.setViewportSize(DESKTOP);
    await loginAs(page, "admin");
    await page.goto("/");
    await assertNavStuckAtTop(page, "/ before dialog");

    await page.evaluate(() => {
      const active = document.activeElement;
      if (active && active !== document.body) (active as HTMLElement).blur();
    });
    await page.keyboard.press("?");
    await expect(page.locator("dialog#shortcuts-cheat-sheet")).toHaveAttribute("open", "");

    const sample = await assertNavStuckAtTop(page, "/ dialog open");
    expect(sample.headerCoveredAtCentre, "the dialog backdrop must cover the bar").toBe(true);
  });

  test("a fixed z-50 overlay (not in the top layer) renders above the stuck bar", async ({
    page,
  }) => {
    // The cheat sheet above is promoted to the top layer, which beats any
    // z-index; the connection-lost overlay is a plain `fixed inset-0 z-50`
    // div, so it exercises the bar's z-40 for real.
    await page.setViewportSize(DESKTOP);
    await loginAs(page, "admin");
    await page.goto("/catalog");
    await assertNavStuckAtTop(page, "/catalog before overlay");

    await page.evaluate(() =>
      document.getElementById("connection-lost-overlay")!.classList.remove("hidden"),
    );
    await expect(page.locator("#connection-lost-overlay")).toBeVisible();
    const sample = await assertNavStuckAtTop(page, "/catalog overlay shown");
    expect(sample.lostOverlayOnTopOfBar, "the overlay must cover the bar").toBe(true);
  });

  test("print — the bar is in normal flow (position: static), the sidebar hidden", async ({
    page,
  }) => {
    await page.setViewportSize(DESKTOP);
    await loginAs(page, "admin");
    await page.goto("/catalog");
    const header = page.locator("header").first();
    await expect(header).toHaveCSS("position", "sticky");
    await expect(page.locator("#sidebar")).toBeVisible();
    await page.emulateMedia({ media: "print" });
    await expect(header).toHaveCSS("position", "static");
    // Story 14-1 — a printed page carries no navigation.
    await expect(page.locator("#sidebar")).toBeHidden();
  });
});
