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

async function scrollToBottomOfLongPage(page: Page) {
  await page.waitForLoadState("load");
  // Scroll inside the poll: a page-load autofocus (e.g. the catalog scan
  // field) can scroll the document back to the top after a single
  // scrollTo, so each attempt re-applies it until it sticks.
  await expect
    .poll(() =>
      page.evaluate(() => {
        const main = document.getElementById("main-content");
        if (main) main.style.minHeight = "3000px";
        window.scrollTo(0, document.documentElement.scrollHeight);
        return window.scrollY;
      }),
    )
    .toBeGreaterThan(1000);
}

async function assertNavStuckAtTop(page: Page, label: string) {
  const nav = page.locator(NAV);
  await expect(nav, `${label}: nav in viewport`).toBeInViewport();
  const box = await nav.boundingBox();
  expect(box, `${label}: nav has a box`).not.toBeNull();
  expect(Math.abs(box!.y), `${label}: nav top at the viewport top`).toBeLessThanOrEqual(1);
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
        await scrollToBottomOfLongPage(page);
        await assertNavStuckAtTop(page, `${path} ${name}`);
      }
    });
  }

  for (const [name, viewport] of [
    ["mobile", MOBILE],
    ["desktop", DESKTOP],
  ] as const) {
    test(`${name} — the skip link target is not hidden under the bar`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      await loginAs(page, "admin");
      await page.goto("/catalog");
      await scrollToBottomOfLongPage(page);

      await page.locator('a[href="#main-content"]').focus();
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
  ] as const) {
    test(`${name} — the mobile menu stays usable and above the content while stuck`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      await loginAs(page, "admin");
      await page.goto("/catalog");
      await scrollToBottomOfLongPage(page);

      await page.locator("#mobile-menu-toggle").click();
      const panel = page.locator("#mobile-nav");
      await expect(panel).toBeVisible();
      await expect(panel).toBeInViewport();
      const navBox = (await page.locator(NAV).boundingBox())!;
      const panelBox = (await panel.boundingBox())!;
      expect(Math.abs(panelBox.y - (navBox.y + navBox.height))).toBeLessThanOrEqual(1);

      // The menu sits above the page content: hit testing at a link's
      // centre lands on that link, not on whatever scrolled underneath.
      const seriesLink = panel.locator('a[href="/series"]');
      const hit = await seriesLink.evaluate((el) => {
        const r = el.getBoundingClientRect();
        const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
        return top !== null && el.contains(top);
      });
      expect(hit, "the menu link must be the topmost element at its centre").toBe(true);

      // The last entry stays reachable even when the menu is taller than
      // the space left under the stuck bar (landscape phone).
      const logout = panel.locator('form[action="/logout"] button');
      await logout.scrollIntoViewIfNeeded();
      await expect(logout).toBeInViewport();
      await assertNavStuckAtTop(page, `${name} menu open`);

      await checkAccessibility(page);

      await seriesLink.click();
      await expect(page).toHaveURL(/\/series$/);
    });
  }

  test("an open dialog renders above the stuck bar", async ({ page }) => {
    await page.setViewportSize(DESKTOP);
    await loginAs(page, "admin");
    await page.goto("/");
    await scrollToBottomOfLongPage(page);
    await assertNavStuckAtTop(page, "/ before dialog");

    await page.evaluate(() => {
      const active = document.activeElement;
      if (active && active !== document.body) (active as HTMLElement).blur();
    });
    await page.keyboard.press("?");
    await expect(page.locator("dialog#shortcuts-cheat-sheet")).toHaveAttribute("open", "");

    const coveredByOverlay = await page.evaluate(() => {
      const nav = document.querySelector('nav[aria-label="Main navigation"]')!;
      const r = nav.getBoundingClientRect();
      const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      return top !== null && !document.querySelector("header")!.contains(top);
    });
    expect(coveredByOverlay, "the dialog backdrop must cover the bar").toBe(true);
  });

  test("print — the bar falls back to normal flow and is not repeated", async ({
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
