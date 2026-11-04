/**
 * Story 9-17 — NavBar hamburger menu; rewritten by story 14-2 (#547,
 * UX-DR39) around the navigation drawer: below `lg` the menu button opens
 * the sidebar's `<nav>` as a drawer from the left, over a scrim, as a
 * modal dialog.
 *
 *   1. The menu button shows below `lg` only; the drawer is closed until
 *      opened; growing to `lg` turns it back into the sidebar.
 *   2. Each way out — close button, scrim, Escape, following a link —
 *      closes it; focus returns to the menu button except after a link.
 *   3. On open focus moves into the drawer (close button); Tab and
 *      Shift+Tab stay inside; the page behind is inert and does not
 *      scroll.
 *   4. /catalog: a scanner burst while the drawer is open reaches neither
 *      `#scan-field` nor a drawer control; a human Enter on a focused link
 *      still follows it.
 *   5. Following an entry navigates and the next page shows it closed;
 *      Log out from the drawer.
 *   6. Stacking: above the sticky bar and the content, below the
 *      connection-lost overlay; reduced motion: no slide.
 *   7. axe with the drawer open and closed, light and dark.
 *
 * Spec ID "NH" — no ISBNs generated.
 */
import { test, expect, Page } from "@playwright/test";
import { loginAs } from "../../helpers/auth";
import { checkAccessibility } from "../../helpers/accessibility";
import {
  DRAWER,
  DRAWER_CLOSE,
  DRAWER_DIALOG,
  MENU_BUTTON,
  SCRIM,
  expectDrawerClosed,
  expectDrawerOpen,
  openDrawer,
} from "../../helpers/drawer";

const PHONE = { width: 390, height: 844 };
const TABLET = { width: 768, height: 1024 };
const DESKTOP = { width: 1280, height: 720 };

/** The `href` (or id) of the focused element, for readable assertions. */
async function focused(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const a = document.activeElement as HTMLElement | null;
    if (!a || a === document.body) return null;
    return a.getAttribute("href") ?? (a.id || a.tagName.toLowerCase());
  });
}

async function focusInsideDrawer(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const nav = document.getElementById("sidebar");
    return !!nav && nav.contains(document.activeElement);
  });
}

test.describe("Story 14-2 — the navigation drawer", () => {
  test("the menu button shows below lg only; growing to lg closes the drawer", async ({
    page,
  }) => {
    await page.setViewportSize(TABLET);
    await page.goto("/catalog");
    const button = page.locator(MENU_BUTTON);
    await expect(button).toBeVisible();
    await expectDrawerClosed(page);

    await openDrawer(page);
    await page.setViewportSize(DESKTOP);
    // The sidebar is back in place, no longer a dialog, the page live.
    await expect(button).toBeHidden();
    await expect(page.locator(DRAWER)).toBeVisible();
    await expect(page.locator(DRAWER_DIALOG)).not.toHaveAttribute("role", "dialog");
    await expect(page.locator(DRAWER_DIALOG)).not.toHaveAttribute("aria-modal", "true");
    await expect(page.locator("#page-column")).not.toHaveAttribute("inert", "");
    await expect(page.locator("html")).not.toHaveClass(/nav-drawer-open/);
    await expect(page.locator(SCRIM)).toBeHidden();

    await page.setViewportSize(TABLET);
    await expectDrawerClosed(page);
  });

  test("close button, scrim and Escape close it and give the focus back to the menu button", async ({
    page,
  }) => {
    await page.setViewportSize(PHONE);
    await page.goto("/catalog");
    const button = page.locator(MENU_BUTTON);

    await openDrawer(page);
    await expect(
      page.getByRole("dialog", { name: /^(Menu|Menü)$/ }).getByRole("navigation", {
        name: "Main navigation",
      }),
    ).toBeVisible();
    await page.locator(DRAWER_CLOSE).click();
    await expectDrawerClosed(page);
    await expect(button).toBeFocused();

    await openDrawer(page);
    // The scrim, right of the 280 px drawer.
    await page.mouse.click(PHONE.width - 20, PHONE.height / 2);
    await expectDrawerClosed(page);
    await expect(button).toBeFocused();

    await openDrawer(page);
    await page.keyboard.press("Escape");
    await expectDrawerClosed(page);
    await expect(button).toBeFocused();
  });

  test("following a link closes it without taking the focus back", async ({ page }) => {
    await page.setViewportSize(PHONE);
    await page.goto("/catalog");
    const drawer = await openDrawer(page);
    // Cancel the navigation: the only thing that can close the drawer is
    // nav.js's link handler (every page starts with it closed).
    await page.evaluate(() => {
      document.querySelectorAll("#sidebar a[href]").forEach((a) => {
        a.addEventListener("click", (e) => e.preventDefault(), { once: true });
      });
    });
    await drawer.locator("a[href='/locations']").click();
    await expectDrawerClosed(page);
    await expect(page.locator(MENU_BUTTON)).not.toBeFocused();
  });

  test("focus moves into the drawer, Tab stays inside, the page behind is inert and still", async ({
    page,
  }) => {
    await page.setViewportSize(PHONE);
    await loginAs(page, "admin");
    await page.goto("/series");
    // Scroll down first so a page scroll would show.
    await page.evaluate(() => {
      document.getElementById("main-content")!.style.minHeight = "3000px";
      window.scrollTo(0, 600);
    });
    const scrollBefore = await page.evaluate(() => window.scrollY);
    expect(scrollBefore).toBeGreaterThan(500);

    await openDrawer(page);
    await expect(page.locator(DRAWER_CLOSE)).toBeFocused();
    await expect(page.locator("#page-column")).toHaveAttribute("inert", "");
    await expect(page.locator("html")).toHaveCSS("overflow-y", "hidden");

    // Every focusable control of the drawer, in order.
    const count = await page
      .locator(`${DRAWER} a[href]:visible, ${DRAWER} button:visible, ${DRAWER} select:visible`)
      .count();
    expect(count).toBeGreaterThan(10);
    const seen = new Set<string | null>();
    for (let i = 0; i < count + 2; i++) {
      await page.keyboard.press("Tab");
      expect(await focusInsideDrawer(page), `Tab ${i + 1} stays in the drawer`).toBe(true);
      seen.add(await focused(page));
    }
    expect(seen.has("/admin") && seen.has("lang-select") && seen.has("theme-toggle")).toBe(true);
    // Shift+Tab from the first control (the logo link) wraps to the last.
    await page.locator(`${DRAWER} a[href='/']`).first().focus();
    await page.keyboard.press("Shift+Tab");
    expect(await focusInsideDrawer(page)).toBe(true);
    expect(await focused(page)).toBe("button");

    // The wheel over the scrim does not scroll the page.
    await page.mouse.move(PHONE.width - 20, PHONE.height / 2);
    await page.mouse.wheel(0, 800);
    await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(scrollBefore);

    await page.keyboard.press("Escape");
    await expectDrawerClosed(page);
    await expect(page.locator("#page-column")).not.toHaveAttribute("inert", "");
    expect(await page.evaluate(() => window.scrollY)).toBe(scrollBefore);
  });

  test("on /catalog a scanner burst reaches neither the scan field nor a drawer control", async ({
    page,
  }) => {
    await loginAs(page, "librarian");
    await page.setViewportSize(PHONE);
    await page.goto("/catalog");
    const scan = page.locator("#scan-field");
    await expect(scan).toBeAttached();
    const feedbackBefore = await page.locator(".feedback-entry").count();

    const drawer = await openDrawer(page);
    // A burst while the close button holds the focus …
    await page.keyboard.type("9782070360246\n", { delay: 5 });
    // … and one while an entry holds it (its terminating Enter must not
    // follow the link).
    await drawer.locator("a[href='/series']").focus();
    await page.keyboard.type("V0001\n", { delay: 5 });

    await expectDrawerOpen(page);
    await expect(page).toHaveURL(/\/catalog$/);
    await expect(scan).toHaveValue("");
    await expect(page.locator(".feedback-entry")).toHaveCount(feedbackBefore);

    await page.keyboard.press("Escape");
    await expectDrawerClosed(page);
  });

  test("a human Enter on a focused entry follows it, and the next page shows the drawer closed", async ({
    page,
  }) => {
    await page.setViewportSize(PHONE);
    await page.goto("/catalog");
    const drawer = await openDrawer(page);
    // No keydown since the menu button was clicked: a human-paced Enter.
    await drawer.locator("a[href='/locations']").focus();
    await page.keyboard.press("Enter");
    await expect(page).toHaveURL(/\/locations$/);
    await expectDrawerClosed(page);
    // The current entry is marked in the drawer.
    const reopened = await openDrawer(page);
    await expect(reopened.locator("a[href='/locations']")).toHaveAttribute("aria-current", "page");

    // A tap on an entry navigates too.
    await reopened.locator("a[href='/series']").click();
    await expect(page).toHaveURL(/\/series$/);
    await expectDrawerClosed(page);
  });

  // Issue #150 — log out on a phone; story 14-2: from the drawer's foot.
  test("logout from the drawer signs the user out", async ({ page }) => {
    await loginAs(page, "librarian");
    await page.setViewportSize(PHONE);
    await page.goto("/catalog");

    const drawer = await openDrawer(page);
    const logout = drawer.locator('form[action="/logout"][method="POST"] button[type="submit"]');
    await expect(logout).toBeVisible();
    await logout.click();

    await page.waitForURL((u) => u.pathname === "/");
    const anonymous = await openDrawer(page);
    await expect(anonymous.locator('a[href="/login"]')).toBeVisible();
    await expect(anonymous.locator('form[action="/logout"]')).toHaveCount(0);
  });

  test("stacking: above the bar and the content, below the connection-lost overlay", async ({
    page,
  }) => {
    await page.setViewportSize(PHONE);
    await loginAs(page, "admin");
    await page.goto("/catalog");
    await openDrawer(page);

    const hit = (x: number, y: number) =>
      page.evaluate(
        ({ px, py }) => {
          const el = document.elementFromPoint(px, py);
          if (!el) return "none";
          if (el.closest("#connection-lost-overlay")) return "overlay";
          if (el.closest("#sidebar")) return "drawer";
          if (el.closest("#nav-scrim")) return "scrim";
          return "page";
        },
        { px: x, py: y },
      );
    // Over the sticky bar's area (top-left) the drawer paints; right of it,
    // the scrim covers the bar and the content.
    expect(await hit(20, 20)).toBe("drawer");
    expect(await hit(PHONE.width - 20, 20)).toBe("scrim");
    expect(await hit(PHONE.width - 20, PHONE.height / 2)).toBe("scrim");

    await page.evaluate(() =>
      document.getElementById("connection-lost-overlay")!.classList.remove("hidden"),
    );
    await expect(page.locator("#connection-lost-overlay")).toBeVisible();
    expect(await hit(20, PHONE.height / 2)).toBe("overlay");
  });

  test("prefers-reduced-motion: no slide", async ({ page }) => {
    await page.setViewportSize(PHONE);
    await page.goto("/catalog");
    // Longest transition of the drawer, in seconds. (The global
    // reduced-motion rule leaves a near-zero duration, not exactly 0s.)
    const duration = () =>
      page
        .locator(DRAWER)
        .evaluate((el) =>
          Math.max(...getComputedStyle(el).transitionDuration.split(",").map((d) => parseFloat(d))),
        );
    await page.emulateMedia({ reducedMotion: "no-preference" });
    expect(await duration()).toBeGreaterThanOrEqual(0.1);
    await page.emulateMedia({ reducedMotion: "reduce" });
    expect(await duration()).toBeLessThan(0.01);
    await openDrawer(page);
    await page.keyboard.press("Escape");
    await expectDrawerClosed(page);
  });

  test("axe passes with the drawer open and closed, light and dark", async ({ page }) => {
    await loginAs(page, "admin");
    await page.setViewportSize(PHONE);
    for (const scheme of ["light", "dark"] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.goto("/catalog");
      await checkAccessibility(page);
      await openDrawer(page);
      await checkAccessibility(page);
      await page.keyboard.press("Escape");
      await expectDrawerClosed(page);
    }
  });
});
