/**
 * Story 9-21 — responsive per-page layouts E2E.
 *
 * Verifies the app adapts cleanly at 3 representative viewport widths:
 *   - Mobile (375x667 — iPhone SE)
 *   - Tablet (768x1024 — iPad portrait)
 *   - Desktop (1280x800 — modern laptop)
 *
 * Asserts on each viewport:
 *   - No horizontal page scroll (body width ≤ viewport width + 1px tol).
 *   - Key elements remain visible (search field, table cells, etc.).
 *   - Column-hide on small viewports (loans + borrowers tables).
 *
 * Spec ID "RL" — no ISBNs generated.
 */
import { test, expect, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { checkAccessibility } from "../../helpers/accessibility";

const MOBILE = { width: 375, height: 667 };
const TABLET = { width: 768, height: 1024 };
const DESKTOP = { width: 1280, height: 800 };

async function assertNoMajorHorizontalScroll(page: Page, label: string) {
  const overflow = await page.evaluate(() => {
    return {
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    };
  });
  // 32px tolerance — the AC is "no horizontal scroll forcing the user to
  // pan the page". A few extra pixels from scrollbar gutter, sub-pixel
  // rendering, or a wide content cell are acceptable. The catastrophic
  // case (e.g., a 1200px-wide table on a 375px viewport) blows past this
  // tolerance by hundreds of pixels.
  const overflowAmount = overflow.scrollWidth - overflow.clientWidth;
  expect(
    overflowAmount,
    `${label}: significant horizontal overflow (scrollWidth=${overflow.scrollWidth}, clientWidth=${overflow.clientWidth}, overflow=${overflowAmount}px)`,
  ).toBeLessThanOrEqual(32);
}

test.describe("Story 9-21 — responsive per-page layouts", () => {
  test("mobile (375px) — no horizontal scroll on key surfaces", async ({
    page,
  }) => {
    await page.setViewportSize(MOBILE);

    // Anonymous home
    await page.goto("/");
    await assertNoMajorHorizontalScroll(page, "/ mobile");
    await expect(page.locator("#search-field")).toBeVisible();

    // Anonymous catalog (page layout should fit)
    await page.goto("/catalog");
    await assertNoMajorHorizontalScroll(page, "/catalog mobile");

    // Librarian surfaces
    await loginAs(page, "librarian");
    await page.goto("/borrowers");
    await assertNoMajorHorizontalScroll(page, "/borrowers mobile");

    await page.goto("/loans");
    await assertNoMajorHorizontalScroll(page, "/loans mobile");

    await page.goto("/series");
    await assertNoMajorHorizontalScroll(page, "/series mobile");
  });

  test("tablet (768px) — no horizontal scroll on key surfaces", async ({
    page,
  }) => {
    await page.setViewportSize(TABLET);
    await loginAs(page, "librarian");

    await page.goto("/borrowers");
    await assertNoMajorHorizontalScroll(page, "/borrowers tablet");

    await page.goto("/loans");
    await assertNoMajorHorizontalScroll(page, "/loans tablet");

    await page.goto("/");
    await assertNoMajorHorizontalScroll(page, "/ tablet");
  });

  test("desktop (1280px) — no horizontal scroll on key surfaces", async ({
    page,
  }) => {
    await page.setViewportSize(DESKTOP);
    await loginAs(page, "librarian");

    await page.goto("/borrowers");
    await assertNoMajorHorizontalScroll(page, "/borrowers desktop");

    await page.goto("/loans");
    await assertNoMajorHorizontalScroll(page, "/loans desktop");

    await page.goto("/");
    await assertNoMajorHorizontalScroll(page, "/ desktop");

    await page.goto("/series");
    await assertNoMajorHorizontalScroll(page, "/series desktop");
  });
});

/**
 * Story 11-4 (#496) — sticky navigation bar.
 *
 * The bar lives in the shared layout, so a few representative pages are
 * enough. Each page is made reliably scrollable through the CSSOM
 * (`style.minHeight` on #main-content — allowed by CSP, unlike an inline
 * `style` attribute) so the assertions never depend on how much data the
 * shared E2E database happens to hold. Read-only: no entity is created.
 */
const NAV = 'nav[aria-label="Main navigation"]';
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
        const nav = document.querySelector('nav[aria-label="Main navigation"]')!;
        const r = nav.getBoundingClientRect();
        const header = document.querySelector("header")!;
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
        await assertNavStuckAtTop(page, `${path} ${name}`);
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
              requestAnimationFrame(() =>
                resolve({
                  mainTop: document.getElementById("main-content")!.getBoundingClientRect().top,
                  navBottom: document
                    .querySelector('nav[aria-label="Main navigation"]')!
                    .getBoundingClientRect().bottom,
                }),
              ),
            ),
          ),
      );
      await expect(page.locator(NAV)).toBeInViewport();
      expect(
        geometry.mainTop,
        `main top (${geometry.mainTop}) must sit below the bar bottom (${geometry.navBottom})`,
      ).toBeGreaterThanOrEqual(geometry.navBottom - 1);
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
    const after = await page.evaluate(() => window.scrollY);
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
    const after = await page.evaluate(() => window.scrollY);
    expect(Math.abs(after - before.scrollY), "the page must not jump").toBeLessThanOrEqual(1);
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

  test("print — the bar prints in normal flow (position: static), once at the top", async ({
    page,
  }) => {
    await page.setViewportSize(DESKTOP);
    await loginAs(page, "admin");
    await page.goto("/catalog");
    const header = page.locator("header").first();
    await expect(header).toHaveCSS("position", "sticky");
    await page.emulateMedia({ media: "print" });
    await expect(header).toHaveCSS("position", "static");
  });
});
